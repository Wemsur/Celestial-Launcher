//! What was installed from a content source, remembered per instance.
//!
//! A file placed on disk is not enough to keep it current: an update check needs
//! to know which source it came from, which project and version it is, and — for
//! a source whose download target is matched by a rule, such as a GitHub release
//! asset — the rule it was matched under. So an install writes a record here.
//!
//! The record lives in the instance folder (`source_content.json`) rather than
//! in the launcher's database, following the same principle as `instance.json`:
//! instance data travels with the instance. Skins are deliberately not recorded
//! — they belong to the launcher, and a skin has no version to update.

use super::manifest::ContentType;
use crate::State;
use crate::util::io;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use tokio::sync::{Mutex, OwnedMutexGuard};

const FILE_NAME: &str = "source_content.json";

/// Per-instance-directory locks, so a read-modify-write of `source_content.json`
/// cannot interleave with another.
///
/// The write is atomic on its own (temp file plus rename), but `record_install`
/// and `forget_install` each read the whole file, change one entry, and write it
/// back. Two of those running at once — two installs finishing together, or an
/// install racing a "stop tracking" — would each write a list based on the same
/// starting point, and one change would be silently lost. The lock is per
/// directory rather than global so installs into different instances still run
/// in parallel.
static STORE_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

async fn lock_store(instance_dir: &Path) -> OwnedMutexGuard<()> {
    let lock = {
        let mut locks = STORE_LOCKS.lock().await;
        locks
            .entry(instance_dir.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    lock.lock_owned().await
}

/// One installed file, and everything needed to update or switch its version.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceContentRecord {
    /// Stable id for this record, used by the UI to address it.
    pub id: String,
    pub source_id: String,
    pub project_id: String,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub version_name: Option<String>,
    pub content_type: ContentType,
    /// Relative to the instance folder, e.g. `mods/example.jar`.
    pub relative_path: String,
    pub name: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    /// Opaque rule the source matched its download with, so a later check can
    /// match the same kind of asset again.
    #[serde(default)]
    pub install_rule: Option<serde_json::Value>,
    #[serde(default)]
    pub installed_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SourceContentFile {
    #[serde(default = "current_version")]
    version: u32,
    #[serde(default)]
    records: Vec<SourceContentRecord>,
}

fn current_version() -> u32 {
    1
}

impl Default for SourceContentFile {
    fn default() -> Self {
        Self {
            version: current_version(),
            records: Vec::new(),
        }
    }
}

fn store_path(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join(FILE_NAME)
}

async fn read_store(
    instance_dir: &std::path::Path,
) -> crate::Result<SourceContentFile> {
    let path = store_path(instance_dir);
    let bytes = match io::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(SourceContentFile::default()),
    };
    match serde_json::from_slice(&bytes) {
        Ok(file) => Ok(file),
        Err(error) => {
            // Losing this only means the next update check has nothing to work
            // from, which is a far better outcome than failing to browse.
            tracing::warn!(
                path = %path.display(),
                "Content source install record is unreadable, starting empty: \
                 {error}"
            );
            Ok(SourceContentFile::default())
        }
    }
}

async fn write_store(
    instance_dir: &std::path::Path,
    file: &SourceContentFile,
) -> crate::Result<()> {
    io::create_dir_all(instance_dir).await?;
    io::write(
        store_path(instance_dir),
        serde_json::to_vec_pretty(file)?,
    )
    .await?;
    Ok(())
}

/// The instance folder a record belongs to.
async fn instance_dir_of(
    instance_id: &str,
    state: &State,
) -> crate::Result<PathBuf> {
    let scope =
        crate::state::instances::commands::resolve_content_scope(
            instance_id, None, state,
        )
        .await?;
    let path = scope.instance.path.clone();
    Ok(crate::state::libraries::resolve_instance_dir(state, &path))
}

/// Remember an install, replacing any earlier record for the same file.
///
/// Replacing by path is the point: installing a newer version of the same mod
/// writes to the same path, and it should look like one item, not two.
pub async fn record_install(
    instance_id: &str,
    mut record: SourceContentRecord,
    state: &State,
) -> crate::Result<SourceContentRecord> {
    if record.id.trim().is_empty() {
        record.id = format!(
            "{}:{}:{}",
            record.source_id,
            record.project_id,
            record.relative_path
        );
    }
    record.installed_at = Some(chrono::Utc::now().timestamp());

    let dir = instance_dir_of(instance_id, state).await?;
    let _guard = lock_store(&dir).await;
    let mut store = read_store(&dir).await?;
    store
        .records
        .retain(|existing| existing.relative_path != record.relative_path);
    store.records.push(record.clone());
    write_store(&dir, &store).await?;
    Ok(record)
}

/// Everything installed from a content source into an instance.
pub async fn list_installed(
    instance_id: &str,
    state: &State,
) -> crate::Result<Vec<SourceContentRecord>> {
    let dir = instance_dir_of(instance_id, state).await?;
    let mut records = read_store(&dir).await?.records;
    records.retain(|record| {
        let path = dir.join(&record.relative_path);
        path.is_file() || path.is_dir()
    });
    records.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(records)
}

/// Forget a record, without touching the file on disk.
pub async fn forget_install(
    instance_id: &str,
    record_id: &str,
    state: &State,
) -> crate::Result<()> {
    let dir = instance_dir_of(instance_id, state).await?;
    let _guard = lock_store(&dir).await;
    let mut store = read_store(&dir).await?;
    store.records.retain(|record| record.id != record_id);
    write_store(&dir, &store).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_store_is_empty() {
        let file: SourceContentFile =
            serde_json::from_slice(b"{}").unwrap();
        assert!(file.records.is_empty());
        assert_eq!(file.version, current_version());
    }

    #[test]
    fn a_record_round_trips() {
        let file = SourceContentFile {
            version: 1,
            records: vec![SourceContentRecord {
                id: "id".to_string(),
                source_id: "pixelmap".to_string(),
                project_id: "239".to_string(),
                version_id: Some("26.2".to_string()),
                version_name: None,
                content_type: ContentType::World,
                relative_path: "saves/City".to_string(),
                name: "City".to_string(),
                icon_url: None,
                install_rule: None,
                installed_at: None,
            }],
        };
        let encoded = serde_json::to_vec(&file).unwrap();
        let decoded: SourceContentFile = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.records.len(), 1);
        assert_eq!(decoded.records[0].content_type, ContentType::World);
    }
}

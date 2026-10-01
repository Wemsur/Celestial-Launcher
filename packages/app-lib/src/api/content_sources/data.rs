//! Per-source storage, settings values, and the crash log that disables a
//! source that threw.
//!
//! All three live under the source's own data directory, the only place a
//! source is ever allowed to write. Unlike a UI plugin, a content source is not
//! gated by a permission record: it is installed deliberately and its only
//! reach is its own data plus the hosts it declared, so its storage needs no
//! separate grant.

use super::store::source_data_dir;
use crate::State;
use crate::util::io;
use std::collections::BTreeMap;
use std::path::PathBuf;

const STORAGE_FILE: &str = "storage.json";
const CRASH_LOG_FILE: &str = "crash.log";

/// Cap on one source's stored data. A source is third-party code, and without a
/// limit a runaway source can fill the user's disk.
const MAX_STORAGE_BYTES: usize = 50 * 1024 * 1024;

/// The crash log is for the user to read, not a history, so it is capped and
/// trimmed from the front rather than growing without bound.
const MAX_CRASH_LOG_BYTES: u64 = 64 * 1024;

const MAX_KEY_BYTES: usize = 256;

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
struct Storage {
    #[serde(default)]
    values: BTreeMap<String, String>,
}

pub async fn storage_get(
    source_id: &str,
    key: &str,
) -> crate::Result<Option<String>> {
    let state = State::get().await?;
    Ok(load(&state, source_id).await?.values.get(key).cloned())
}

pub async fn storage_set(
    source_id: &str,
    key: &str,
    value: String,
) -> crate::Result<()> {
    validate_key(key)?;
    let state = State::get().await?;
    let mut storage = load(&state, source_id).await?;
    storage.values.insert(key.to_string(), value);
    save(&state, source_id, &storage).await
}

pub async fn storage_remove(
    source_id: &str,
    key: &str,
) -> crate::Result<()> {
    validate_key(key)?;
    let state = State::get().await?;
    let mut storage = load(&state, source_id).await?;
    if storage.values.remove(key).is_none() {
        return Ok(());
    }
    save(&state, source_id, &storage).await
}

pub async fn storage_keys(source_id: &str) -> crate::Result<Vec<String>> {
    let state = State::get().await?;
    Ok(load(&state, source_id)
        .await?
        .values
        .keys()
        .cloned()
        .collect())
}

/// Read a source's settings values, for the form the source manager renders.
pub async fn settings_get_all(
    source_id: &str,
) -> crate::Result<std::collections::HashMap<String, String>> {
    let state = State::get().await?;
    Ok(load(&state, source_id).await?.values.into_iter().collect())
}

/// Write one settings value on the user's behalf (from the source manager).
pub async fn settings_set(
    source_id: &str,
    key: &str,
    value: String,
) -> crate::Result<()> {
    validate_key(key)?;
    let state = State::get().await?;
    let mut storage = load(&state, source_id).await?;
    storage.values.insert(key.to_string(), value);
    save(&state, source_id, &storage).await
}

/// Record a source crash and switch the source off.
///
/// Reaching a crash at all means the source did something the loader could not
/// contain, so the launcher stops running it instead of throwing again on every
/// browse. The log is what the user is shown when they ask why it stopped.
pub async fn record_crash(
    source_id: &str,
    message: &str,
    stack: Option<&str>,
) -> crate::Result<()> {
    let state = State::get().await?;
    let dir = source_data_dir(&state, source_id)?;
    io::create_dir_all(&dir).await?;
    let path = dir.join(CRASH_LOG_FILE);

    let entry = format!(
        "[{}] {}\n{}\n\n",
        chrono::Utc::now().to_rfc3339(),
        message,
        stack.unwrap_or("(no stack trace)")
    );

    let existing = io::read(&path).await.unwrap_or_default();
    let mut contents = String::from_utf8_lossy(&existing).into_owned();
    contents.push_str(&entry);
    if contents.len() as u64 > MAX_CRASH_LOG_BYTES {
        // Keep the newest entry: an unreadable wall of history is worse than
        // losing the oldest crash.
        contents = entry;
    }
    io::write(&path, contents.as_bytes()).await?;

    tracing::error!(
        source = %source_id,
        "Content source crashed and has been disabled: {message}"
    );

    super::store::set_enabled(source_id, false).await?;
    Ok(())
}

/// Read the crash log the source manager shows for a source that stopped itself.
pub async fn crash_log(source_id: &str) -> crate::Result<Option<String>> {
    let state = State::get().await?;
    let dir = source_data_dir(&state, source_id)?;
    let path = dir.join(CRASH_LOG_FILE);
    match io::read(&path).await {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(_) => Ok(None),
    }
}

async fn load(state: &State, source_id: &str) -> crate::Result<Storage> {
    let path = storage_path(state, source_id).await?;
    let bytes = match io::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(Storage::default()),
    };
    match serde_json::from_slice(&bytes) {
        Ok(storage) => Ok(storage),
        Err(error) => {
            // A source's own scratch data is not worth refusing to start over.
            tracing::warn!(
                source = %source_id,
                "Content source storage is unreadable, starting from empty: \
                 {error}"
            );
            Ok(Storage::default())
        }
    }
}

async fn save(
    state: &State,
    source_id: &str,
    storage: &Storage,
) -> crate::Result<()> {
    let bytes = serde_json::to_vec_pretty(storage)?;
    if bytes.len() > MAX_STORAGE_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source '{source_id}' has reached its {} MiB storage limit",
            MAX_STORAGE_BYTES / (1024 * 1024)
        ))
        .into());
    }
    io::write(storage_path(state, source_id).await?, bytes).await?;
    Ok(())
}

async fn storage_path(
    state: &State,
    source_id: &str,
) -> crate::Result<PathBuf> {
    let dir = source_data_dir(state, source_id)?;
    if !dir.exists() {
        io::create_dir_all(&dir).await?;
    }
    Ok(dir.join(STORAGE_FILE))
}

fn validate_key(key: &str) -> crate::Result<()> {
    if key.is_empty() {
        return Err(crate::ErrorKind::InputError(
            "Storage key must not be empty".to_string(),
        )
        .into());
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Storage key is longer than {MAX_KEY_BYTES} bytes"
        ))
        .into());
    }
    if key.chars().any(char::is_control) {
        return Err(crate::ErrorKind::InputError(
            "Storage key must not contain control characters".to_string(),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_validated() {
        assert!(validate_key("").is_err());
        assert!(validate_key("a\u{0}b").is_err());
        assert!(validate_key(&"k".repeat(MAX_KEY_BYTES + 1)).is_err());
        assert!(validate_key("github.token").is_ok());
    }
}

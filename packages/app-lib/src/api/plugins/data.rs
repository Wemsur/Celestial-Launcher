//! Per-plugin storage, and the crash log that disables a plugin that threw.
//!
//! Both live under the plugin's own data directory, which is the only place a
//! plugin is ever allowed to write.

use super::store::{ensure_granted, plugin_data_dir};
use crate::State;
use crate::util::io;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const STORAGE_FILE: &str = "storage.json";
const CRASH_LOG_FILE: &str = "crash.log";

/// Cap on one plugin's stored data. A plugin is third-party code, and without
/// a limit a runaway plugin can fill the user's disk.
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
    plugin_id: &str,
    key: &str,
) -> crate::Result<Option<String>> {
    ensure_granted(plugin_id, "storage").await?;
    let state = State::get().await?;
    Ok(load(&state, plugin_id).await?.values.get(key).cloned())
}

pub async fn storage_set(
    plugin_id: &str,
    key: &str,
    value: String,
) -> crate::Result<()> {
    ensure_granted(plugin_id, "storage").await?;
    validate_key(key)?;
    let state = State::get().await?;
    let mut storage = load(&state, plugin_id).await?;
    storage.values.insert(key.to_string(), value);
    save(&state, plugin_id, &storage).await
}

pub async fn storage_remove(
    plugin_id: &str,
    key: &str,
) -> crate::Result<()> {
    ensure_granted(plugin_id, "storage").await?;
    validate_key(key)?;
    let state = State::get().await?;
    let mut storage = load(&state, plugin_id).await?;
    if storage.values.remove(key).is_none() {
        return Ok(());
    }
    save(&state, plugin_id, &storage).await
}

pub async fn storage_keys(plugin_id: &str) -> crate::Result<Vec<String>> {
    ensure_granted(plugin_id, "storage").await?;
    let state = State::get().await?;
    Ok(load(&state, plugin_id)
        .await?
        .values
        .keys()
        .cloned()
        .collect())
}

/// Read a plugin's settings values, ungated.
///
/// Declared settings are configured by the *user* in the launcher's own plugin
/// page, so reading and writing them is the launcher's action, not the plugin's
/// — it does not require the plugin to hold `storage`. Values live in the same
/// per-plugin file, so a plugin with `storage` sees them through `api.storage`
/// too, but a plugin that only declares settings need not ask for storage at all.
pub async fn settings_get_all(
    plugin_id: &str,
) -> crate::Result<std::collections::HashMap<String, String>> {
    let state = State::get().await?;
    Ok(load(&state, plugin_id).await?.values.into_iter().collect())
}

/// Write one settings value on the user's behalf (from the plugin page).
pub async fn settings_set(
    plugin_id: &str,
    key: &str,
    value: String,
) -> crate::Result<()> {
    validate_key(key)?;
    let state = State::get().await?;
    let mut storage = load(&state, plugin_id).await?;
    storage.values.insert(key.to_string(), value);
    save(&state, plugin_id, &storage).await
}

/// Record a plugin crash and switch the plugin off.
///
/// Reaching a crash at all means the plugin did something the loader could not
/// contain, so the launcher stops running it instead of throwing again on every
/// render. The log is what the user is shown when they ask why it stopped.
///
/// Deliberately not gated on any permission: writing this is the launcher's own
/// action, and a plugin that crashed is exactly the one least likely to hold a
/// grant.
pub async fn record_crash(
    plugin_id: &str,
    message: &str,
    stack: Option<&str>,
) -> crate::Result<()> {
    let state = State::get().await?;
    let dir = plugin_data_dir(&state, plugin_id)?;
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
        plugin = %plugin_id,
        "Plugin crashed and has been disabled: {message}"
    );

    super::store::set_enabled(plugin_id, false).await?;
    Ok(())
}

/// Read the crash log the plugin page shows for a plugin that stopped itself.
pub async fn crash_log(plugin_id: &str) -> crate::Result<Option<String>> {
    let state = State::get().await?;
    let dir = plugin_data_dir(&state, plugin_id)?;
    let path = dir.join(CRASH_LOG_FILE);
    match io::read(&path).await {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(_) => Ok(None),
    }
}

async fn load(state: &State, plugin_id: &str) -> crate::Result<Storage> {
    let path = storage_path(state, plugin_id).await?;
    let bytes = match io::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(Storage::default()),
    };
    match serde_json::from_slice(&bytes) {
        Ok(storage) => Ok(storage),
        Err(error) => {
            // A plugin's own scratch data is not worth refusing to start over;
            // starting empty is recoverable, a hard error is not.
            tracing::warn!(
                plugin = %plugin_id,
                "Plugin storage is unreadable, starting from empty: {error}"
            );
            Ok(Storage::default())
        }
    }
}

async fn save(
    state: &State,
    plugin_id: &str,
    storage: &Storage,
) -> crate::Result<()> {
    let bytes = serde_json::to_vec_pretty(storage)?;
    if bytes.len() > MAX_STORAGE_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin '{plugin_id}' has reached its \
             {} MiB storage limit",
            MAX_STORAGE_BYTES / (1024 * 1024)
        ))
        .into());
    }
    io::write(storage_path(state, plugin_id).await?, bytes).await?;
    Ok(())
}

async fn storage_path(
    state: &State,
    plugin_id: &str,
) -> crate::Result<PathBuf> {
    let dir = plugin_data_dir(state, plugin_id)?;
    ensure_dir(&dir).await?;
    Ok(dir.join(STORAGE_FILE))
}

async fn ensure_dir(dir: &Path) -> crate::Result<()> {
    if !dir.exists() {
        io::create_dir_all(dir).await?;
    }
    Ok(())
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
        assert!(validate_key("hello.world/with:chars").is_ok());
    }

    #[test]
    fn storage_round_trips_through_json() {
        let mut storage = Storage::default();
        storage.values.insert("a".to_string(), "1".to_string());
        storage.values.insert("b".to_string(), "2".to_string());
        let encoded = serde_json::to_vec(&storage).unwrap();
        let decoded: Storage = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.values.len(), 2);
        assert_eq!(decoded.values.get("a").map(String::as_str), Some("1"));
    }

    #[test]
    fn an_absent_storage_file_is_an_empty_store() {
        let decoded: Storage = serde_json::from_slice(b"{}").unwrap();
        assert!(decoded.values.is_empty());
    }
}

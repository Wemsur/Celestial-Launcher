//! Translation cache rows and service settings, preserving the frontend JSON contract.

use super::CelestialDb;
use super::ui_settings::{has_import_flag, insert_missing, mark_imported, read_optional_json};
use crate::state::State;
use serde_json::{Map, Value};
use sqlx::SqliteConnection;
use std::path::Path;

const IMPORT_FLAG: &str = "import_translation_done";
const TRANSLATION_DIR: &str = "translation";
const SETTINGS_FILE: &str = "settings.json";
const CACHE_FILE: &str = "cache.json";

/// `ui_settings` key holding the translation service settings document.
pub const KEY_TRANSLATION_SETTINGS: &str = "translation_settings";

pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	import_from_dir(&state.celestial, &state.directories.settings_dir.join(TRANSLATION_DIR)).await
}

async fn import_from_dir(db: &CelestialDb, dir: &Path) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	if has_import_flag(&mut tx, IMPORT_FLAG).await? {
		return Ok(());
	}
	if let Some(value) = read_optional_json(&dir.join(SETTINGS_FILE)).await? {
		insert_missing(&mut tx, KEY_TRANSLATION_SETTINGS, &value).await?;
	}
	if let Some(document) = read_optional_json(&dir.join(CACHE_FILE)).await? {
		write_entries(&mut tx, cache_entries(&document)?).await?;
	}
	mark_imported(&mut tx, IMPORT_FLAG).await?;
	tx.commit().await?;
	Ok(())
}

fn cache_entries(document: &Value) -> crate::Result<&Map<String, Value>> {
	if document.get("version").and_then(Value::as_u64) != Some(1) {
		return Err(crate::ErrorKind::InputError(
			"Translation cache must have version 1".into(),
		).into());
	}
	document.get("entries").and_then(Value::as_object).ok_or_else(|| {
		crate::ErrorKind::InputError("Translation cache entries must be an object".into()).into()
	})
}

async fn write_entries(
	connection: &mut SqliteConnection,
	entries: &Map<String, Value>,
) -> crate::Result<()> {
	for (key, entry) in entries {
		let created_at = entry.get("at").and_then(Value::as_i64).unwrap_or_default();
		sqlx::query(
			"INSERT INTO translation_cache (cache_key, entry_json, created_at)
			 VALUES (?, ?, ?) ON CONFLICT(cache_key) DO NOTHING",
		)
		.bind(key)
		.bind(entry.to_string())
		.bind(created_at)
		.execute(&mut *connection)
		.await?;
	}
	Ok(())
}

/// Rebuilds the frontend's version 1 cache document; entries round-trip verbatim.
pub async fn load_json(
	pool: &sqlx::Pool<sqlx::Sqlite>,
) -> crate::Result<String> {
	let rows: Vec<(String, String)> =
		sqlx::query_as("SELECT cache_key, entry_json FROM translation_cache")
			.fetch_all(pool)
			.await?;

	let mut entries = Map::new();
	for (key, entry_json) in rows {
		match serde_json::from_str::<Value>(&entry_json) {
			Ok(entry) => {
				entries.insert(key, entry);
			}
			Err(error) => {
				tracing::warn!(
					"Dropping unreadable translation cache entry {key}: {error}"
				);
			}
		}
	}
	Ok(serde_json::json!({"version": 1, "entries": entries}).to_string())
}

/// Validates the wrapper before replacing the entire cache atomically.
pub async fn replace_json(
	pool: &sqlx::Pool<sqlx::Sqlite>,
	contents: &str,
) -> crate::Result<()> {
	let document: Value = serde_json::from_str(contents).map_err(|error| {
		crate::ErrorKind::InputError(format!(
			"Translation cache is not valid JSON: {error}"
		))
	})?;
	let entries = cache_entries(&document)?;
	let mut tx = pool.begin().await?;
	sqlx::query("DELETE FROM translation_cache")
		.execute(&mut *tx)
		.await?;
	write_entries(&mut tx, entries).await?;
	tx.commit().await?;
	Ok(())
}

pub async fn clear(pool: &sqlx::Pool<sqlx::Sqlite>) -> crate::Result<()> {
	sqlx::query("DELETE FROM translation_cache")
		.execute(pool)
		.await?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn invalid_cache_does_not_clear_saved_entries() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		let original = r#"{"version":1,"entries":{"saved":{"t":"new","at":1}}}"#;
		replace_json(db.pool(), original).await.unwrap();
		for invalid in ["{", "{}", r#"{"version":1,"entries":[]}"#, r#"{"version":2,"entries":{}}"#] {
			assert!(replace_json(db.pool(), invalid).await.is_err());
			assert_eq!(
				serde_json::from_str::<Value>(&load_json(db.pool()).await.unwrap()).unwrap(),
				serde_json::from_str::<Value>(original).unwrap(),
			);
		}
		sqlx::query("CREATE TRIGGER fail_replace BEFORE INSERT ON translation_cache BEGIN SELECT RAISE(ABORT, 'injected'); END")
			.execute(db.pool()).await.unwrap();
		assert!(replace_json(db.pool(), original).await.is_err());
		assert_eq!(
			serde_json::from_str::<Value>(&load_json(db.pool()).await.unwrap()).unwrap(),
			serde_json::from_str::<Value>(original).unwrap(),
		);
	}

	#[tokio::test]
	async fn import_failure_rolls_back_settings_and_retries() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		let settings = dir.path().join(SETTINGS_FILE);
		let cache = dir.path().join(CACHE_FILE);
		tokio::fs::write(&settings, r#"{"enabled":true}"#).await.unwrap();
		tokio::fs::write(&cache, "{").await.unwrap();
		assert!(import_from_dir(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		assert!(super::super::ui_settings::get_json(db.pool(), KEY_TRANSLATION_SETTINGS).await.unwrap().is_none());
		let document = r#"{"version":1,"entries":{"saved":{"t":"old","at":1}}}"#;
		tokio::fs::write(&cache, document).await.unwrap();
		sqlx::query("CREATE TRIGGER fail_cache BEFORE INSERT ON translation_cache BEGIN SELECT RAISE(ABORT, 'injected'); END")
			.execute(db.pool()).await.unwrap();
		assert!(import_from_dir(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		assert_eq!(
			serde_json::from_str::<Value>(&load_json(db.pool()).await.unwrap()).unwrap(),
			serde_json::json!({"version": 1, "entries": {}}),
		);
		assert!(super::super::ui_settings::get_json(db.pool(), KEY_TRANSLATION_SETTINGS).await.unwrap().is_none());
		sqlx::query("DROP TRIGGER fail_cache").execute(db.pool()).await.unwrap();
		import_from_dir(&db, dir.path()).await.unwrap();
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_some());
		let updated = r#"{"version":1,"entries":{"saved":{"t":"new","at":2}}}"#;
		replace_json(db.pool(), updated).await.unwrap();
		import_from_dir(&db, dir.path()).await.unwrap();
		let loaded: Value = serde_json::from_str(&load_json(db.pool()).await.unwrap()).unwrap();
		assert_eq!(loaded["entries"]["saved"]["t"], "new");
	}
}

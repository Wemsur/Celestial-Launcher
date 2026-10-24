//! `translation_cache` table, replacing `translation/cache.json`, plus the
//! translation service settings which move into `ui_settings`.
//!
//! The cache table stores the frontend's entry JSON verbatim under each key,
//! so entry schema evolution stays owned by the frontend — the database only
//! keys rows and records `created_at` for future TTL work.

use super::CelestialDb;
use crate::state::State;
use crate::util::io;
use serde_json::{Map, Value};

const IMPORT_FLAG: &str = "import_translation_done";
const TRANSLATION_DIR: &str = "translation";
const SETTINGS_FILE: &str = "settings.json";
const CACHE_FILE: &str = "cache.json";

/// `ui_settings` key holding the translation service settings document.
pub const KEY_TRANSLATION_SETTINGS: &str = "translation_settings";

pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	let db: &CelestialDb = &state.celestial;
	if db.get_meta(IMPORT_FLAG).await?.is_some() {
		return Ok(());
	}

	let dir = state.directories.settings_dir.join(TRANSLATION_DIR);

	// Service settings become one opaque `ui_settings` row.
	match io::read(&dir.join(SETTINGS_FILE)).await {
		Ok(content) => {
			if let Ok(value) = serde_json::from_slice::<Value>(&content) {
				super::ui_settings::set_json(
					db.pool(),
					KEY_TRANSLATION_SETTINGS,
					&value.to_string(),
				)
				.await?;
			}
		}
		Err(error) => {
			tracing::debug!("No legacy translation settings: {error}");
		}
	}

	// Cache entries go in row by row.
	match io::read(&dir.join(CACHE_FILE)).await {
		Ok(content) => {
			match serde_json::from_slice::<Value>(&content) {
				Ok(Value::Object(document)) => {
					if let Some(Value::Object(entries)) =
						document.get("entries")
					{
						import_entries(db, entries).await?;
					}
				}
				_ => {
					tracing::warn!("Skipping corrupt legacy translation cache");
				}
			}
		}
		Err(error) => {
			tracing::debug!("No legacy translation cache: {error}");
		}
	}

	db.set_meta(IMPORT_FLAG, "done").await?;
	Ok(())
}

async fn import_entries(
	db: &CelestialDb,
	entries: &Map<String, Value>,
) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	for (key, entry) in entries {
		let entry_json = entry.to_string();
		let created_at = entry
			.get("at")
			.and_then(Value::as_i64)
			.unwrap_or_default();
		sqlx::query(
			"INSERT INTO translation_cache (cache_key, entry_json, created_at)
			 VALUES (?, ?, ?)
			 ON CONFLICT(cache_key) DO UPDATE SET
			   entry_json = excluded.entry_json,
			   created_at = excluded.created_at",
		)
		.bind(key)
		.bind(&entry_json)
		.bind(created_at)
		.execute(&mut *tx)
		.await?;
	}
	tx.commit().await?;
	Ok(())
}

/// Rebuilds the frontend's cache document from the table. The wrapper's
/// `version` matches the format the frontend currently owns; entry contents
/// round-trip verbatim.
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

	let document = serde_json::json!({
		"version": 1,
		"entries": entries,
	});
	Ok(document.to_string())
}

/// Replaces the whole table with the frontend's cache document. Saving the
/// full map keeps the frontend contract unchanged; the table is small enough
/// that a transactional rewrite is cheap, and a later per-entry API can slim
/// it further without touching the frontend.
pub async fn replace_json(
	pool: &sqlx::Pool<sqlx::Sqlite>,
	contents: &str,
) -> crate::Result<()> {
	let document: Value = serde_json::from_str(contents).map_err(|error| {
		crate::ErrorKind::InputError(format!(
			"Translation cache is not valid JSON: {error}"
		))
	})?;
	let entries = match document.get("entries") {
		Some(Value::Object(entries)) => entries.clone(),
		_ => Map::new(),
	};

	let mut tx = pool.begin().await?;
	sqlx::query("DELETE FROM translation_cache")
		.execute(&mut *tx)
		.await?;
	for (key, entry) in &entries {
		let created_at =
			entry.get("at").and_then(Value::as_i64).unwrap_or_default();
		sqlx::query(
			"INSERT INTO translation_cache (cache_key, entry_json, created_at)
			 VALUES (?, ?, ?)",
		)
		.bind(key)
		.bind(entry.to_string())
		.bind(created_at)
		.execute(&mut *tx)
		.await?;
	}
	tx.commit().await?;
	Ok(())
}

pub async fn clear(pool: &sqlx::Pool<sqlx::Sqlite>) -> crate::Result<()> {
	sqlx::query("DELETE FROM translation_cache")
		.execute(pool)
		.await?;
	Ok(())
}

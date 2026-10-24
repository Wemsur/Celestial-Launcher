//! `ui_settings` table: a small key/value store for fork UI preferences,
//! replacing the `celestial_settings.json`, `interface/split-view.json` and
//! `interface/ui-preferences.json` files. Values are opaque strings owned by
//! the call site; the database never interprets them.

use super::CelestialDb;
use crate::state::State;
use crate::util::io;
use serde_json::Value;

const IMPORT_FLAG: &str = "import_ui_done";
const SETTINGS_FILE: &str = "celestial_settings.json";
const SPLIT_VIEW_FILE: &str = "split-view.json";
const UI_PREFERENCES_FILE: &str = "ui-preferences.json";

/// Key for the shared celestial settings document
/// (`custom_backgrounds/celestial_settings.json`).
pub const KEY_CELESTIAL_SETTINGS: &str = "celestial_settings";
pub const KEY_SPLIT_VIEW: &str = "split_view_settings";
pub const KEY_UI_PREFERENCES: &str = "ui_preferences";

pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	let db: &CelestialDb = &state.celestial;
	if db.get_meta(IMPORT_FLAG).await?.is_some() {
		return Ok(());
	}

	import_file(
		db,
		&state
			.directories
			.settings_dir
			.join("custom_backgrounds")
			.join(SETTINGS_FILE),
		KEY_CELESTIAL_SETTINGS,
	)
	.await;
	import_file(
		db,
		&state
			.directories
			.settings_dir
			.join("interface")
			.join(SPLIT_VIEW_FILE),
		KEY_SPLIT_VIEW,
	)
	.await;
	import_file(
		db,
		&state
			.directories
			.settings_dir
			.join("interface")
			.join(UI_PREFERENCES_FILE),
		KEY_UI_PREFERENCES,
	)
	.await;

	db.set_meta(IMPORT_FLAG, "done").await?;
	Ok(())
}

async fn import_file(
	db: &CelestialDb,
	path: &std::path::Path,
	key: &str,
) {
	match io::read(path).await {
		Ok(content) => {
			// Only valid JSON imports; a corrupt file must not wedge the
			// feature on its defaults forever.
			if let Ok(value) = serde_json::from_slice::<Value>(&content) {
				let raw = value.to_string();
				if let Err(error) = set_json(db.pool(), key, &raw).await {
					tracing::warn!("Failed to import UI settings for {key}: {error}");
				}
			} else {
				tracing::warn!(
					"Skipping corrupt legacy UI settings file {}",
					path.display()
				);
			}
		}
		Err(error) => {
			tracing::debug!(
				"No legacy UI settings at {}: {error}",
				path.display()
			);
		}
	}
}

pub async fn get_json(
	pool: &sqlx::Pool<sqlx::Sqlite>,
	key: &str,
) -> crate::Result<Option<String>> {
	let value: Option<(String,)> =
		sqlx::query_as("SELECT value FROM ui_settings WHERE key = ?")
			.bind(key)
			.fetch_optional(pool)
			.await?;
	Ok(value.map(|(value,)| value))
}

pub async fn set_json(
	pool: &sqlx::Pool<sqlx::Sqlite>,
	key: &str,
	value: &str,
) -> crate::Result<()> {
	sqlx::query(
		"INSERT INTO ui_settings (key, value, updated_at)
		 VALUES (?, ?, unixepoch())
		 ON CONFLICT(key) DO UPDATE SET
		   value = excluded.value, updated_at = excluded.updated_at",
	)
	.bind(key)
	.bind(value)
	.execute(pool)
	.await?;
	Ok(())
}

pub async fn delete(
	pool: &sqlx::Pool<sqlx::Sqlite>,
	key: &str,
) -> crate::Result<()> {
	sqlx::query("DELETE FROM ui_settings WHERE key = ?")
		.bind(key)
		.execute(pool)
		.await?;
	Ok(())
}

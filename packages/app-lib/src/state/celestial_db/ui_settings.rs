//! `ui_settings` table: fork UI preferences imported from legacy JSON files.
//! Values remain JSON-serialized strings owned by the call sites.

use super::CelestialDb;
use crate::state::State;
use crate::util::io;
use serde_json::Value;
use sqlx::SqliteConnection;
use std::path::Path;

const IMPORT_FLAG: &str = "import_ui_done";
const SPLIT_IMPORT_FLAG: &str = "import_ui_independent_keys_done";
const INDEPENDENT_KEYS: &[&str] = &[
	"blur_enabled",
	"instance_card_icon_bg",
	"hue_value",
	"library_sort_directions",
	"new_import_banner",
];
const SETTINGS_FILE: &str = "celestial_settings.json";
const SPLIT_VIEW_FILE: &str = "split-view.json";
const UI_PREFERENCES_FILE: &str = "ui-preferences.json";

/// Key for the shared celestial settings document
/// (`custom_backgrounds/celestial_settings.json`).
pub const KEY_CELESTIAL_SETTINGS: &str = "celestial_settings";
pub const KEY_SPLIT_VIEW: &str = "split_view_settings";
pub const KEY_UI_PREFERENCES: &str = "ui_preferences";

pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	import_from_dir(&state.celestial, &state.directories.settings_dir).await
}

async fn import_from_dir(db: &CelestialDb, dir: &Path) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	if !has_import_flag(&mut tx, IMPORT_FLAG).await? {
		for (path, key) in [
			(dir.join("custom_backgrounds").join(SETTINGS_FILE), KEY_CELESTIAL_SETTINGS),
			(dir.join("interface").join(SPLIT_VIEW_FILE), KEY_SPLIT_VIEW),
			(dir.join("interface").join(UI_PREFERENCES_FILE), KEY_UI_PREFERENCES),
		] {
			if let Some(value) = read_optional_json(&path).await? {
				insert_missing(&mut tx, key, &value).await?;
			}
		}
		mark_imported(&mut tx, IMPORT_FLAG).await?;
	}

	if !has_import_flag(&mut tx, SPLIT_IMPORT_FLAG).await? {
		let raw: Option<String> = sqlx::query_scalar(
			"SELECT value FROM ui_settings WHERE key = ?",
		)
		.bind(KEY_CELESTIAL_SETTINGS)
		.fetch_optional(&mut *tx)
		.await?;
		if let Some(raw) = raw {
			let value: Value = serde_json::from_str(&raw)?;
			let fields = value.as_object().ok_or_else(|| {
				crate::ErrorKind::InputError("Celestial settings must be an object".into())
			})?;
			for &key in INDEPENDENT_KEYS {
				if let Some(value) = fields.get(key) {
					insert_missing(&mut tx, key, value).await?;
				}
			}
		}
		mark_imported(&mut tx, SPLIT_IMPORT_FLAG).await?;
	}
	tx.commit().await?;
	Ok(())
}

pub(super) async fn read_optional_json(path: &Path) -> crate::Result<Option<Value>> {
	match io::read(path).await {
		Ok(content) => Ok(Some(serde_json::from_slice(&content)?)),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}

pub(super) async fn insert_missing(
	connection: &mut SqliteConnection,
	key: &str,
	value: &Value,
) -> crate::Result<()> {
	sqlx::query(
		"INSERT INTO ui_settings (key, value, updated_at)
		 VALUES (?, ?, unixepoch()) ON CONFLICT(key) DO NOTHING",
	)
	.bind(key)
	.bind(value.to_string())
	.execute(connection)
	.await?;
	Ok(())
}

pub(super) async fn has_import_flag(
	connection: &mut SqliteConnection,
	key: &str,
) -> crate::Result<bool> {
	let value: Option<String> = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
		.bind(key)
		.fetch_optional(connection)
		.await?;
	Ok(value.is_some())
}

pub(super) async fn mark_imported(
	connection: &mut SqliteConnection,
	key: &str,
) -> crate::Result<()> {
	sqlx::query(
		"INSERT INTO meta (key, value) VALUES (?, 'done')
		 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
	)
	.bind(key)
	.execute(connection)
	.await?;
	Ok(())
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

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn imports_independent_fields_and_preserves_source() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		let backgrounds = dir.path().join("custom_backgrounds");
		tokio::fs::create_dir(&backgrounds).await.unwrap();
		let path = backgrounds.join(SETTINGS_FILE);
		let document = serde_json::json!({
			"blur_enabled": false,
			"instance_card_icon_bg": false,
			"hue_value": 120,
			"library_sort_directions": {"name": "desc"},
			"new_import_banner": true,
		});
		let raw = document.to_string();
		tokio::fs::write(&path, &raw).await.unwrap();
		import_from_dir(&db, dir.path()).await.unwrap();
		for &key in INDEPENDENT_KEYS {
			assert_eq!(get_json(db.pool(), key).await.unwrap(), Some(document[key].to_string()));
		}
		assert_eq!(get_json(db.pool(), KEY_CELESTIAL_SETTINGS).await.unwrap(), Some(raw.clone()));
		assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), raw);
	}

	#[tokio::test]
	async fn repairs_old_import_without_overwriting_saved_values() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		db.set_meta(IMPORT_FLAG, "done").await.unwrap();
		set_json(db.pool(), KEY_CELESTIAL_SETTINGS, r#"{"blur_enabled":false,"hue_value":120}"#).await.unwrap();
		set_json(db.pool(), "hue_value", "240").await.unwrap();
		import_from_dir(&db, dir.path()).await.unwrap();
		assert_eq!(get_json(db.pool(), "blur_enabled").await.unwrap().as_deref(), Some("false"));
		assert_eq!(get_json(db.pool(), "hue_value").await.unwrap().as_deref(), Some("240"));
		assert!(db.get_meta(SPLIT_IMPORT_FLAG).await.unwrap().is_some());
		set_json(db.pool(), "blur_enabled", "true").await.unwrap();
		delete(db.pool(), "hue_value").await.unwrap();
		import_from_dir(&db, dir.path()).await.unwrap();
		assert_eq!(get_json(db.pool(), "blur_enabled").await.unwrap().as_deref(), Some("true"));
		assert!(get_json(db.pool(), "hue_value").await.unwrap().is_none());
	}

	#[tokio::test]
	async fn import_errors_leave_no_data_or_markers() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		let backgrounds = dir.path().join("custom_backgrounds");
		let interface = dir.path().join("interface");
		tokio::fs::create_dir(&backgrounds).await.unwrap();
		tokio::fs::create_dir(&interface).await.unwrap();
		tokio::fs::write(backgrounds.join(SETTINGS_FILE), r#"{"blur_enabled":false}"#).await.unwrap();
		let split = interface.join(SPLIT_VIEW_FILE);
		tokio::fs::write(&split, "{").await.unwrap();
		assert!(import_from_dir(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		assert!(get_json(db.pool(), KEY_CELESTIAL_SETTINGS).await.unwrap().is_none());
		tokio::fs::remove_file(&split).await.unwrap();
		tokio::fs::create_dir(&split).await.unwrap();
		assert!(import_from_dir(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		tokio::fs::remove_dir(&split).await.unwrap();
		sqlx::query("CREATE TRIGGER fail_split BEFORE INSERT ON ui_settings WHEN NEW.key = 'blur_enabled' BEGIN SELECT RAISE(ABORT, 'injected'); END")
			.execute(db.pool()).await.unwrap();
		assert!(import_from_dir(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		assert!(db.get_meta(SPLIT_IMPORT_FLAG).await.unwrap().is_none());
		assert!(get_json(db.pool(), KEY_CELESTIAL_SETTINGS).await.unwrap().is_none());
		sqlx::query("DROP TRIGGER fail_split").execute(db.pool()).await.unwrap();
		import_from_dir(&db, dir.path()).await.unwrap();
		assert_eq!(get_json(db.pool(), "blur_enabled").await.unwrap().as_deref(), Some("false"));
	}
}

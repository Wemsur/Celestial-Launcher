//! `libraries` table: the multi-library registry, replacing `libraries.json`.

use super::CelestialDb;
use crate::state::libraries::{
	InstanceFormat, LibrariesConfig, LibraryInfo, LIBRARIES_FILE_NAME,
};
use crate::state::State;
use sqlx::SqliteConnection;
use std::io::ErrorKind;
use std::path::Path;

const IMPORT_FLAG: &str = "import_libraries_done";
const META_MIGRATED: &str = "libraries_migrated";
const META_SCHEMA_VERSION: &str = "libraries_schema_version";
const META_ACTIVE: &str = "libraries_active_path";

pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	let path = state.directories.settings_dir.join(LIBRARIES_FILE_NAME);
	import_from_path(&state.celestial, &path).await
}

async fn import_from_path(db: &CelestialDb, path: &Path) -> crate::Result<()> {
	if db.get_meta(IMPORT_FLAG).await?.is_some() {
		return Ok(());
	}
	let config = match tokio::fs::read(path).await {
		Ok(content) => Some(serde_json::from_slice::<LibrariesConfig>(&content)?),
		Err(error) if error.kind() == ErrorKind::NotFound => None,
		Err(error) => return Err(error.into()),
	};

	let mut tx = db.pool().begin().await?;
	// Claim within the transaction so concurrent imports cannot overwrite data.
	let claimed = sqlx::query(
		"INSERT INTO meta (key, value) VALUES (?, 'done')
		 ON CONFLICT(key) DO NOTHING",
	)
	.bind(IMPORT_FLAG)
	.execute(&mut *tx)
	.await?
	.rows_affected();
	if claimed != 0 {
		if let Some(config) = config {
			write_to_connection(&mut tx, &config).await?;
		}
	}
	tx.commit().await?;
	Ok(())
}

pub(crate) async fn load(
	db: &CelestialDb,
) -> crate::Result<LibrariesConfig> {
	let mut tx = db.pool().begin().await?;
	let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
		"SELECT path, name, format, display_order FROM libraries
		 ORDER BY display_order, path",
	)
	.fetch_all(&mut *tx)
	.await?;

	let libraries = rows
		.into_iter()
		.map(|(path, name, format, _)| LibraryInfo {
			name,
			path,
			format: InstanceFormat::from(format.as_str()),
		})
		.collect();
	let metadata: Vec<(String, String)> = sqlx::query_as(
		"SELECT key, value FROM meta WHERE key IN (?, ?, ?)",
	)
	.bind(META_MIGRATED)
	.bind(META_SCHEMA_VERSION)
	.bind(META_ACTIVE)
	.fetch_all(&mut *tx)
	.await?;
	let mut config = LibrariesConfig {
		libraries,
		migrated: false,
		schema_version: 0,
		active_library_path: None,
	};
	for (key, value) in metadata {
		match key.as_str() {
			META_MIGRATED => config.migrated = value == "true",
			META_SCHEMA_VERSION => config.schema_version = value.parse().unwrap_or(0),
			META_ACTIVE => config.active_library_path = Some(value).filter(|v| !v.is_empty()),
			_ => {}
		}
	}
	tx.commit().await?;
	Ok(config)
}

pub(crate) async fn save(
	db: &CelestialDb,
	config: &LibrariesConfig,
) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	write_to_connection(&mut tx, config).await?;
	tx.commit().await?;
	Ok(())
}

async fn write_to_connection(
	connection: &mut SqliteConnection,
	config: &LibrariesConfig,
) -> crate::Result<()> {
	sqlx::query("DELETE FROM libraries").execute(&mut *connection).await?;
	for (index, library) in config.libraries.iter().enumerate() {
		sqlx::query(
			"INSERT INTO libraries (path, name, format, display_order)
			 VALUES (?, ?, ?, ?)",
		)
		.bind(&library.path)
		.bind(&library.name)
		.bind(<&str>::from(library.format.clone()))
		.bind(index as i64)
		.execute(&mut *connection)
		.await?;
	}
	let schema_version = config.schema_version.to_string();
	for (key, value) in [
		(META_MIGRATED, if config.migrated { "true" } else { "false" }),
		(META_SCHEMA_VERSION, schema_version.as_str()),
		(META_ACTIVE, config.active_library_path.as_deref().unwrap_or("")),
	] {
		sqlx::query(
			"INSERT INTO meta (key, value) VALUES (?, ?)
			 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
		)
		.bind(key)
		.bind(value)
		.execute(&mut *connection)
		.await?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	async fn test_db() -> (tempfile::TempDir, CelestialDb) {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		(dir, db)
	}

	fn config(path: Option<&str>) -> LibrariesConfig {
		LibrariesConfig {
			libraries: path.into_iter().map(|path| LibraryInfo {
				path: path.into(),
				name: path.into(),
				format: InstanceFormat::Minecraft,
			}).collect(),
			migrated: true,
			schema_version: 7,
			active_library_path: Some("active".into()),
		}
	}

	async fn write_legacy(path: &Path, config: &LibrariesConfig) {
		tokio::fs::write(path, serde_json::to_vec(config).unwrap()).await.unwrap();
	}

	async fn assert_config(db: &CelestialDb, expected: &LibrariesConfig) {
		let actual = load(db).await.unwrap();
		assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(expected).unwrap());
	}

	#[tokio::test]
	async fn empty_import_preserves_metadata_and_is_not_repeated() {
		let (dir, db) = test_db().await;
		let path = dir.path().join(LIBRARIES_FILE_NAME);
		let empty = config(None);
		write_legacy(&path, &empty).await;
		import_from_path(&db, &path).await.unwrap();
		assert_config(&db, &empty).await;
		assert_eq!(db.get_meta(IMPORT_FLAG).await.unwrap().as_deref(), Some("done"));

		let updated = config(Some("new"));
		save(&db, &updated).await.unwrap();
		tokio::fs::write(&path, b"invalid").await.unwrap();
		import_from_path(&db, &path).await.unwrap();
		assert_config(&db, &updated).await;
		save(&db, &empty).await.unwrap();
		write_legacy(&path, &updated).await;
		import_from_path(&db, &path).await.unwrap();
		assert_config(&db, &empty).await;
	}

	#[tokio::test]
	async fn invalid_json_and_io_errors_remain_retryable() {
		let (dir, db) = test_db().await;
		let path = dir.path().join(LIBRARIES_FILE_NAME);
		for content in [b"invalid".as_slice(), b"".as_slice()] {
			tokio::fs::write(&path, content).await.unwrap();
			assert!(import_from_path(&db, &path).await.is_err());
			assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		}
		assert!(import_from_path(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		let expected = config(Some("legacy"));
		write_legacy(&path, &expected).await;
		import_from_path(&db, &path).await.unwrap();
		assert_config(&db, &expected).await;
	}

	#[tokio::test]
	async fn missing_file_marks_done_without_changing_data() {
		let (dir, db) = test_db().await;
		let expected = config(Some("existing"));
		save(&db, &expected).await.unwrap();
		let path = dir.path().join(LIBRARIES_FILE_NAME);
		import_from_path(&db, &path).await.unwrap();
		write_legacy(&path, &config(Some("stale"))).await;
		import_from_path(&db, &path).await.unwrap();
		assert_config(&db, &expected).await;
		assert_eq!(db.get_meta(IMPORT_FLAG).await.unwrap().as_deref(), Some("done"));
	}

	#[tokio::test]
	async fn import_and_save_roll_back_on_write_failures() {
		let (dir, db) = test_db().await;
		let path = dir.path().join(LIBRARIES_FILE_NAME);
		let original = config(Some("original"));
		save(&db, &original).await.unwrap();
		let mut replacement = config(Some("replacement"));
		replacement.migrated = false;
		replacement.schema_version = 9;
		replacement.active_library_path = None;
		write_legacy(&path, &replacement).await;

		for trigger in [
			"CREATE TRIGGER fail_write BEFORE INSERT ON libraries BEGIN SELECT RAISE(ABORT, 'fail'); END",
			"CREATE TRIGGER fail_write BEFORE UPDATE ON meta WHEN NEW.key = 'libraries_active_path' BEGIN SELECT RAISE(ABORT, 'fail'); END",
			"CREATE TRIGGER fail_write BEFORE INSERT ON meta WHEN NEW.key = 'import_libraries_done' BEGIN SELECT RAISE(ABORT, 'fail'); END",
		] {
			sqlx::query(trigger).execute(db.pool()).await.unwrap();
			assert!(import_from_path(&db, &path).await.is_err());
			assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
			assert_config(&db, &original).await;
			if !trigger.contains("import_libraries_done") {
				assert!(save(&db, &replacement).await.is_err());
				assert_config(&db, &original).await;
			}
			sqlx::query("DROP TRIGGER fail_write").execute(db.pool()).await.unwrap();
		}
		let mut duplicate = config(Some("duplicate"));
		duplicate.libraries.push(duplicate.libraries[0].clone());
		write_legacy(&path, &duplicate).await;
		assert!(import_from_path(&db, &path).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		assert_config(&db, &original).await;

		write_legacy(&path, &replacement).await;
		import_from_path(&db, &path).await.unwrap();
		assert_config(&db, &replacement).await;
		assert_eq!(db.get_meta(IMPORT_FLAG).await.unwrap().as_deref(), Some("done"));
	}
}

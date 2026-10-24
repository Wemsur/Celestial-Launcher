//! `libraries` table: the multi-library registry, replacing `libraries.json`.

use super::CelestialDb;
use crate::state::libraries::{
	InstanceFormat, LibrariesConfig, LibraryInfo, LIBRARIES_FILE_NAME,
};
use crate::state::State;
use crate::util::io;

const IMPORT_FLAG: &str = "import_libraries_done";
const META_MIGRATED: &str = "libraries_migrated";
const META_SCHEMA_VERSION: &str = "libraries_schema_version";
const META_ACTIVE: &str = "libraries_active_path";

pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	let db = &state.celestial;
	if db.get_meta(IMPORT_FLAG).await?.is_some() {
		return Ok(());
	}

	let path = state.directories.settings_dir.join(LIBRARIES_FILE_NAME);
	if path.exists() {
		match io::read(&path).await {
			Ok(content) => {
				match serde_json::from_slice::<LibrariesConfig>(&content) {
					// A corrupt or empty legacy file means there is nothing to
					// carry over; the registry starts fresh either way.
					Ok(config) if !config.libraries.is_empty() => {
						save(db, &config).await?;
					}
					_ => {}
				}
			}
			Err(error) => {
				tracing::warn!(
					"Failed to read legacy {LIBRARIES_FILE_NAME} for import: {error}"
				);
			}
		}
	}

	db.set_meta(IMPORT_FLAG, "done").await?;
	Ok(())
}

pub(crate) async fn load(
	db: &CelestialDb,
) -> crate::Result<LibrariesConfig> {
	let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
		"SELECT path, name, format, display_order FROM libraries
		 ORDER BY display_order, path",
	)
	.fetch_all(db.pool())
	.await?;

	let libraries = rows
		.into_iter()
		.map(|(path, name, format, _)| LibraryInfo {
			name,
			path,
			format: InstanceFormat::from(format.as_str()),
		})
		.collect();

	let migrated = db
		.get_meta(META_MIGRATED)
		.await?
		.map(|value| value == "true")
		.unwrap_or(false);
	let schema_version = db
		.get_meta(META_SCHEMA_VERSION)
		.await?
		.and_then(|value| value.parse().ok())
		.unwrap_or(0);
	let active_library_path = db
		.get_meta(META_ACTIVE)
		.await?
		.filter(|value| !value.is_empty());

	Ok(LibrariesConfig {
		libraries,
		migrated,
		schema_version,
		active_library_path,
	})
}

pub(crate) async fn save(
	db: &CelestialDb,
	config: &LibrariesConfig,
) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	sqlx::query("DELETE FROM libraries").execute(&mut *tx).await?;
	for (index, library) in config.libraries.iter().enumerate() {
		sqlx::query(
			"INSERT INTO libraries (path, name, format, display_order)
			 VALUES (?, ?, ?, ?)",
		)
		.bind(&library.path)
		.bind(&library.name)
		.bind(<&str>::from(library.format.clone()))
		.bind(index as i64)
		.execute(&mut *tx)
		.await?;
	}
	tx.commit().await?;

	db.set_meta(
		META_MIGRATED,
		if config.migrated { "true" } else { "false" },
	)
	.await?;
	db.set_meta(META_SCHEMA_VERSION, &config.schema_version.to_string())
		.await?;
	db.set_meta(
		META_ACTIVE,
		config.active_library_path.as_deref().unwrap_or(""),
	)
	.await?;
	Ok(())
}

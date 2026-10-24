//! `content_sources` table, replacing `content-sources.json`.

use super::CelestialDb;
use crate::api::content_sources::store::SourceRecord;
use crate::state::State;
use crate::util::io;
use serde::Deserialize;

const IMPORT_FLAG: &str = "import_content_sources_done";
const STATE_FILE: &str = "content-sources.json";

#[derive(Deserialize)]
struct LegacyStateFile {
	#[serde(default)]
	sources: Vec<SourceRecord>,
}

pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	let db = &state.celestial;
	if db.get_meta(IMPORT_FLAG).await?.is_some() {
		return Ok(());
	}

	let path = state.directories.settings_dir.join(STATE_FILE);
	if path.exists() {
		match io::read(&path).await {
			Ok(content) => {
				match serde_json::from_slice::<LegacyStateFile>(&content) {
					Ok(legacy) if !legacy.sources.is_empty() => {
						save(db, &legacy.sources).await?;
					}
					_ => {}
				}
			}
			Err(error) => {
				tracing::warn!(
					"Failed to read legacy {STATE_FILE} for import: {error}"
				);
			}
		}
	}

	db.set_meta(IMPORT_FLAG, "done").await?;
	Ok(())
}

pub(crate) async fn load(
	db: &CelestialDb,
) -> crate::Result<Vec<SourceRecord>> {
	let rows: Vec<(String, i64, Option<i64>, Option<String>)> = sqlx::query_as(
		"SELECT id, enabled, installed_at, source FROM content_sources
		 ORDER BY id",
	)
	.fetch_all(db.pool())
	.await?;

	Ok(rows
		.into_iter()
		.map(|(id, enabled, installed_at, source)| SourceRecord {
			id,
			enabled: enabled != 0,
			installed_at,
			source,
		})
		.collect())
}

pub(crate) async fn save(
	db: &CelestialDb,
	records: &[SourceRecord],
) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	sqlx::query("DELETE FROM content_sources")
		.execute(&mut *tx)
		.await?;
	for record in records {
		sqlx::query(
			"INSERT INTO content_sources (id, enabled, installed_at, source)
			 VALUES (?, ?, ?, ?)",
		)
		.bind(&record.id)
		.bind(record.enabled)
		.bind(record.installed_at)
		.bind(record.source.as_deref())
		.execute(&mut *tx)
		.await?;
	}
	tx.commit().await?;
	Ok(())
}

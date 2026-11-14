//! `content_sources` table, replacing `content-sources.json`.

use super::CelestialDb;
use crate::api::content_sources::store::SourceRecord;
use crate::state::State;
use serde::Deserialize;
use sqlx::SqliteConnection;
use std::io::ErrorKind;
use std::path::Path;

const IMPORT_FLAG: &str = "import_content_sources_done";
const STATE_FILE: &str = "content-sources.json";

#[derive(Deserialize)]
struct LegacyStateFile {
	#[serde(default)]
	sources: Vec<SourceRecord>,
}

pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	let path = state.directories.settings_dir.join(STATE_FILE);
	import_from_path(&state.celestial, &path).await
}

async fn import_from_path(db: &CelestialDb, path: &Path) -> crate::Result<()> {
	if db.get_meta(IMPORT_FLAG).await?.is_some() {
		return Ok(());
	}
	let legacy = match tokio::fs::read(path).await {
		Ok(content) => Some(serde_json::from_slice::<LegacyStateFile>(&content)?),
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
		if let Some(legacy) = legacy {
			write_to_connection(&mut tx, &legacy.sources).await?;
		}
	}
	tx.commit().await?;
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
	write_to_connection(&mut tx, records).await?;
	tx.commit().await?;
	Ok(())
}

async fn write_to_connection(
	connection: &mut SqliteConnection,
	records: &[SourceRecord],
) -> crate::Result<()> {
	sqlx::query("DELETE FROM content_sources")
		.execute(&mut *connection)
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

	fn record(id: &str) -> SourceRecord {
		SourceRecord {
			id: id.into(),
			enabled: true,
			installed_at: Some(42),
			source: Some("test".into()),
		}
	}

	async fn write_legacy(path: &Path, records: &[SourceRecord]) {
		let content = serde_json::json!({ "sources": records });
		tokio::fs::write(path, serde_json::to_vec(&content).unwrap()).await.unwrap();
	}

	async fn assert_records(db: &CelestialDb, expected: &[SourceRecord]) {
		let actual = load(db).await.unwrap();
		assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(expected).unwrap());
	}

	#[tokio::test]
	async fn invalid_json_and_io_errors_remain_retryable() {
		let (dir, db) = test_db().await;
		let path = dir.path().join(STATE_FILE);
		for content in [b"invalid".as_slice(), b"".as_slice()] {
			tokio::fs::write(&path, content).await.unwrap();
			assert!(import_from_path(&db, &path).await.is_err());
			assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		}
		assert!(import_from_path(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		let records = vec![record("legacy")];
		write_legacy(&path, &records).await;
		import_from_path(&db, &path).await.unwrap();
		assert_records(&db, &records).await;
	}

	#[tokio::test]
	async fn empty_import_replaces_records_and_does_not_run_twice() {
		let (dir, db) = test_db().await;
		let path = dir.path().join(STATE_FILE);
		save(&db, &[record("existing")]).await.unwrap();
		write_legacy(&path, &[]).await;
		import_from_path(&db, &path).await.unwrap();
		assert_records(&db, &[]).await;
		assert_eq!(db.get_meta(IMPORT_FLAG).await.unwrap().as_deref(), Some("done"));

		let records = vec![record("new")];
		save(&db, &records).await.unwrap();
		tokio::fs::write(&path, b"invalid").await.unwrap();
		import_from_path(&db, &path).await.unwrap();
		assert_records(&db, &records).await;
		save(&db, &[]).await.unwrap();
		write_legacy(&path, &records).await;
		import_from_path(&db, &path).await.unwrap();
		assert_records(&db, &[]).await;
	}

	#[tokio::test]
	async fn missing_file_marks_done_without_changing_records() {
		let (dir, db) = test_db().await;
		let path = dir.path().join(STATE_FILE);
		let records = vec![record("existing")];
		save(&db, &records).await.unwrap();
		import_from_path(&db, &path).await.unwrap();
		write_legacy(&path, &[record("stale")]).await;
		import_from_path(&db, &path).await.unwrap();
		assert_records(&db, &records).await;
		assert_eq!(db.get_meta(IMPORT_FLAG).await.unwrap().as_deref(), Some("done"));
	}

	#[tokio::test]
	async fn import_write_failures_roll_back_and_allow_retry() {
		let (dir, db) = test_db().await;
		let path = dir.path().join(STATE_FILE);
		let original = vec![record("original")];
		let replacement = vec![record("replacement")];
		save(&db, &original).await.unwrap();
		write_legacy(&path, &replacement).await;
		for trigger in [
			"CREATE TRIGGER fail_write BEFORE INSERT ON content_sources BEGIN SELECT RAISE(ABORT, 'fail'); END",
			"CREATE TRIGGER fail_write BEFORE INSERT ON meta WHEN NEW.key = 'import_content_sources_done' BEGIN SELECT RAISE(ABORT, 'fail'); END",
		] {
			sqlx::query(trigger).execute(db.pool()).await.unwrap();
			assert!(import_from_path(&db, &path).await.is_err());
			assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
			assert_records(&db, &original).await;
			sqlx::query("DROP TRIGGER fail_write").execute(db.pool()).await.unwrap();
		}

		let duplicate = vec![record("duplicate"), record("duplicate")];
		write_legacy(&path, &duplicate).await;
		assert!(import_from_path(&db, &path).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		assert_records(&db, &original).await;

		write_legacy(&path, &replacement).await;
		import_from_path(&db, &path).await.unwrap();
		assert_records(&db, &replacement).await;
		assert_eq!(db.get_meta(IMPORT_FLAG).await.unwrap().as_deref(), Some("done"));
	}
}

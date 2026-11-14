//! `instance_groups` / `group_memberships` tables, replacing
//! `instance_groups.json`.

use super::CelestialDb;
use crate::state::instance_groups::{
	GroupDefinition, InstanceGroupsFile, INSTANCE_GROUPS_FILE_NAME,
};
use crate::util::io;
use std::collections::HashMap;
use std::path::Path;

pub(crate) const IMPORT_FLAG: &str = "import_groups_done";

/// Runs inside the database initializer, without accessing global State.
/// Only a missing JSON file permits the later app.db fallback.
pub(crate) async fn ensure_imported(
	db: &CelestialDb,
	settings_dir: &Path,
) -> crate::Result<()> {
	if db.get_meta(IMPORT_FLAG).await?.is_some() {
		return Ok(());
	}

	let path = settings_dir.join(INSTANCE_GROUPS_FILE_NAME);
	let content = match io::read(&path).await {
		Ok(content) => content,
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
		Err(error) => return Err(error.into()),
	};
	let mut file = serde_json::from_slice::<InstanceGroupsFile>(&content)?;
	file.normalize();
	save_imported(db, &file).await
}

pub(crate) async fn load(
	db: &CelestialDb,
) -> crate::Result<InstanceGroupsFile> {
	let mut tx = db.pool().begin().await?;
	let group_rows: Vec<(String, String, i64)> = sqlx::query_as(
		"SELECT id, name, display_order FROM instance_groups
		 ORDER BY display_order, id",
	)
	.fetch_all(&mut *tx)
	.await?;

	let membership_rows: Vec<(String, String, i64)> = sqlx::query_as(
		"SELECT instance_id, group_id, position FROM group_memberships
		 ORDER BY instance_id, position",
	)
	.fetch_all(&mut *tx)
	.await?;
	tx.commit().await?;

	let mut memberships: HashMap<String, Vec<String>> = HashMap::new();
	for (instance_id, group_id, _) in membership_rows {
		memberships.entry(instance_id).or_default().push(group_id);
	}

	Ok(InstanceGroupsFile {
		schema_version: crate::state::instance_groups::INSTANCE_GROUPS_SCHEMA_VERSION,
		groups: group_rows
			.into_iter()
			.map(|(id, name, _)| GroupDefinition { id, name })
			.collect(),
		memberships,
	})
}

pub(crate) async fn save(
	db: &CelestialDb,
	file: &InstanceGroupsFile,
) -> crate::Result<()> {
	save_snapshot(db, file, false).await
}

/// The imported snapshot and its completion marker become visible together.
pub(crate) async fn save_imported(
	db: &CelestialDb,
	file: &InstanceGroupsFile,
) -> crate::Result<()> {
	save_snapshot(db, file, true).await
}

async fn save_snapshot(
	db: &CelestialDb,
	file: &InstanceGroupsFile,
	imported: bool,
) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	sqlx::query("DELETE FROM instance_groups")
		.execute(&mut *tx)
		.await?;
	for (index, group) in file.groups.iter().enumerate() {
		sqlx::query(
			"INSERT INTO instance_groups (id, name, display_order)
			 VALUES (?, ?, ?)",
		)
		.bind(&group.id)
		.bind(&group.name)
		.bind(index as i64)
		.execute(&mut *tx)
		.await?;
	}
	for (instance_id, group_ids) in &file.memberships {
		for (position, group_id) in group_ids.iter().enumerate() {
			sqlx::query(
				"INSERT INTO group_memberships
				 (instance_id, group_id, position) VALUES (?, ?, ?)",
			)
			.bind(instance_id)
			.bind(group_id)
			.bind(position as i64)
			.execute(&mut *tx)
			.await?;
		}
	}
	if imported {
		sqlx::query(
			"INSERT INTO meta (key, value) VALUES (?, 'done')
			 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
		)
		.bind(IMPORT_FLAG)
		.execute(&mut *tx)
		.await?;
	}
	tx.commit().await?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test(flavor = "current_thread")]
	async fn json_import_only_allows_missing_file_fallback() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		ensure_imported(&db, dir.path()).await.unwrap();
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		let path = dir.path().join(INSTANCE_GROUPS_FILE_NAME);
		std::fs::write(&path, b"not json").unwrap();
		assert!(ensure_imported(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
		std::fs::remove_file(&path).unwrap();
		std::fs::create_dir(&path).unwrap();
		assert!(ensure_imported(&db, dir.path()).await.is_err());
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
	}

	#[tokio::test(flavor = "current_thread")]
	async fn json_import_normalizes_before_persisting() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		std::fs::write(dir.path().join(INSTANCE_GROUPS_FILE_NAME), br#"{
			"groups": [{"id":"a","name":"A"}],
			"memberships": {"local:test":["a","missing","a"],"empty":["missing"]}
		}"#).unwrap();
		ensure_imported(&db, dir.path()).await.unwrap();
		let file = load(&db).await.unwrap();
		assert_eq!(file.groups[0].id, crate::state::instance_groups::FAVORITES_GROUP_ID);
		assert_eq!(file.memberships["local:test"], vec!["a".to_string()]);
		assert!(!file.memberships.contains_key("empty"));
		assert_eq!(db.get_meta(IMPORT_FLAG).await.unwrap().as_deref(), Some("done"));
	}

	#[tokio::test(flavor = "current_thread")]
	async fn import_marker_failure_rolls_back_data() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		let original = InstanceGroupsFile::default();
		save(&db, &original).await.unwrap();
		sqlx::query("CREATE TRIGGER reject_import BEFORE INSERT ON meta BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
			.execute(db.pool()).await.unwrap();
		let mut changed = original.clone();
		changed.groups[0].name = "changed".into();
		assert!(save_imported(&db, &changed).await.is_err());
		assert_eq!(load(&db).await.unwrap().groups[0].name, original.groups[0].name);
		assert!(db.get_meta(IMPORT_FLAG).await.unwrap().is_none());
	}
}

//! `instance_groups` / `group_memberships` tables, replacing
//! `instance_groups.json`.

use super::CelestialDb;
use crate::state::instance_groups::{
	GroupDefinition, InstanceGroupsFile, INSTANCE_GROUPS_FILE_NAME,
};
use crate::state::State;
use crate::util::io;
use std::collections::HashMap;

pub(crate) const IMPORT_FLAG: &str = "import_groups_done";

/// Imports `instance_groups.json` when it exists. The flag is only set on a
/// successful import: a missing file must leave it unset so the legacy SQLite
/// fallback in `instance_groups::ensure_imported` still gets its chance.
pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	let db = &state.celestial;
	if db.get_meta(IMPORT_FLAG).await?.is_some() {
		return Ok(());
	}

	let path = state.directories.settings_dir.join(INSTANCE_GROUPS_FILE_NAME);
	if !path.exists() {
		return Ok(());
	}
	match io::read(&path).await {
		Ok(content) => {
			match serde_json::from_slice::<InstanceGroupsFile>(&content) {
				Ok(file) => {
					save(db, &file).await?;
					db.set_meta(IMPORT_FLAG, "done").await?;
				}
				Err(error) => {
					tracing::warn!(
						"Failed to parse legacy {INSTANCE_GROUPS_FILE_NAME} for import: {error}"
					);
				}
			}
		}
		Err(error) => {
			tracing::warn!(
				"Failed to read legacy {INSTANCE_GROUPS_FILE_NAME} for import: {error}"
			);
		}
	}

	Ok(())
}

pub(crate) async fn load(
	db: &CelestialDb,
) -> crate::Result<InstanceGroupsFile> {
	let group_rows: Vec<(String, String, i64)> = sqlx::query_as(
		"SELECT id, name, display_order FROM instance_groups
		 ORDER BY display_order, id",
	)
	.fetch_all(db.pool())
	.await?;

	let membership_rows: Vec<(String, String, i64)> = sqlx::query_as(
		"SELECT instance_id, group_id, position FROM group_memberships
		 ORDER BY instance_id, position",
	)
	.fetch_all(db.pool())
	.await?;

	let mut memberships: HashMap<String, Vec<String>> = HashMap::new();
	for (instance_id, group_id, _) in membership_rows {
		memberships
			.entry(instance_id)
			.or_default()
			.push(group_id);
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
	let mut tx = db.pool().begin().await?;
	// Groups cascade their memberships, so one delete clears both tables.
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
	tx.commit().await?;
	Ok(())
}

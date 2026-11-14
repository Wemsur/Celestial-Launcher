//! Instance grouping backed by the `instance_groups` / `group_memberships`
//! tables in celestial.db.
//!
//! Groups used to live in SQLite (`instance_groups` /
//! `instance_group_memberships` in app.db). That layout could not express
//! membership for JSON-backed instances (`local:` ids, i.e. every
//! `.minecraft` and multi-library instance) because they have no row in the
//! `instances` table, so they moved to `instance_groups.json`, and now to
//! celestial.db. The old app.db tables are left untouched but are never read
//! after import, except for the pre-initialization legacy launcher converter.
//!
//! Reads use a synchronous cache. Async mutations serialize candidate snapshots
//! and publish them only after celestial.db accepts the write.

use crate::state::State;
use crate::state::celestial_db;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};
use tracing::info;
use tokio::sync::Mutex;

pub const INSTANCE_GROUPS_FILE_NAME: &str = "instance_groups.json";

/// Current `instance_groups.json` schema version.
pub const INSTANCE_GROUPS_SCHEMA_VERSION: u32 = 1;

pub const FAVORITES_GROUP_ID: &str = "group:favorites";
const FAVORITES_GROUP_NAME: &str = "Favorites";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupDefinition {
	pub id: String,
	pub name: String,
}

/// On-disk shape. The order of `groups` *is* the display order, so no
/// `display_order` field is needed: the public API only ever communicates
/// ordering through the order of `list()` and `set_order()`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstanceGroupsFile {
	#[serde(default)]
	pub schema_version: u32,
	#[serde(default)]
	pub groups: Vec<GroupDefinition>,
	/// instance id -> group ids.
	#[serde(default)]
	pub memberships: HashMap<String, Vec<String>>,
}

impl Default for InstanceGroupsFile {
	fn default() -> Self {
		Self {
			schema_version: INSTANCE_GROUPS_SCHEMA_VERSION,
			groups: vec![GroupDefinition {
				id: FAVORITES_GROUP_ID.to_string(),
				name: FAVORITES_GROUP_NAME.to_string(),
			}],
			memberships: HashMap::new(),
		}
	}
}

impl InstanceGroupsFile {
	fn has_group(&self, id: &str) -> bool {
		self.groups.iter().any(|group| group.id == id)
	}

	/// Guarantees the favorites sentinel exists and sits first, drops
	/// memberships pointing at groups that no longer exist, and drops instances
	/// left with no groups at all.
	pub(crate) fn normalize(&mut self) {
		self.schema_version = INSTANCE_GROUPS_SCHEMA_VERSION;

		match self.groups.iter().position(|g| g.id == FAVORITES_GROUP_ID) {
			Some(0) => {}
			Some(index) => {
				let favorites = self.groups.remove(index);
				self.groups.insert(0, favorites);
			}
			None => self.groups.insert(
				0,
				GroupDefinition {
					id: FAVORITES_GROUP_ID.to_string(),
					name: FAVORITES_GROUP_NAME.to_string(),
				},
			),
		}

		let known = self
			.groups
			.iter()
			.map(|group| group.id.clone())
			.collect::<HashSet<_>>();

		for group_ids in self.memberships.values_mut() {
			let mut seen = HashSet::new();
			group_ids.retain(|id| known.contains(id) && seen.insert(id.clone()));
		}
		self.memberships.retain(|_, group_ids| !group_ids.is_empty());
	}
}

static STORE: OnceLock<RwLock<InstanceGroupsFile>> = OnceLock::new();
static STORE_READY: OnceLock<()> = OnceLock::new();
static WRITE_LOCK: Mutex<()> = Mutex::const_new(());

/// The caller holds WRITE_LOCK through loading and publishing the snapshot.
async fn seed_store_from_db(state: &State) -> crate::Result<()> {
	seed_cache_from_db(store(), &state.celestial).await?;
	STORE_READY.get_or_init(|| ());
	Ok(())
}

async fn seed_cache_from_db(
	cache: &RwLock<InstanceGroupsFile>,
	db: &celestial_db::CelestialDb,
) -> crate::Result<()> {
	let mut file = celestial_db::groups::load(db).await?;
	file.normalize();
	*cache.write().unwrap_or_else(|e| e.into_inner()) = file;
	Ok(())
}

fn store() -> &'static RwLock<InstanceGroupsFile> {
	STORE.get_or_init(|| RwLock::new(InstanceGroupsFile::default()))
}

fn with_read<T>(f: impl FnOnce(&InstanceGroupsFile) -> T) -> T {
	let guard = store().read().unwrap_or_else(|e| e.into_inner());
	f(&guard)
}

/// Serializes writers without holding the synchronous cache lock across await.
async fn with_write<T>(f: impl FnOnce(&mut InstanceGroupsFile) -> T) -> crate::Result<T> {
	STORE_READY.get().ok_or_else(|| {
		crate::ErrorKind::InputError("Instance groups have not finished loading".to_string())
	})?;
	let state = State::get_if_initialized().ok_or_else(|| {
		crate::ErrorKind::InputError("Instance groups require initialized state".to_string())
	})?;
	persist_candidate(store(), &WRITE_LOCK, &state.celestial, f).await
}

async fn persist_candidate<T>(
	cache: &RwLock<InstanceGroupsFile>,
	writers: &Mutex<()>,
	db: &celestial_db::CelestialDb,
	f: impl FnOnce(&mut InstanceGroupsFile) -> T,
) -> crate::Result<T> {
	let _writer = writers.lock().await;
	let mut candidate = cache.read().unwrap_or_else(|e| e.into_inner()).clone();
	let result = f(&mut candidate);
	candidate.normalize();
	celestial_db::groups::save(db, &candidate).await?;
	*cache.write().unwrap_or_else(|e| e.into_inner()) = candidate;
	Ok(result)
}

// ── Reads ───────────────────────────────────────────────────────────────────

pub fn list() -> Vec<GroupDefinition> {
	with_read(|file| file.groups.clone())
}

pub fn group_exists(id: &str) -> bool {
	with_read(|file| file.has_group(id))
}

/// Group ids for one instance, in the order they were assigned. Synchronous so
/// that `instance_metadata_from_instance` can call it.
pub fn group_ids_for(instance_id: &str) -> Vec<String> {
	with_read(|file| {
		file.memberships.get(instance_id).cloned().unwrap_or_default()
	})
}

fn instances_in_group(file: &InstanceGroupsFile, group_id: &str) -> Vec<String> {
	file.memberships
		.iter()
		.filter(|(_, group_ids)| group_ids.iter().any(|id| id == group_id))
		.map(|(instance_id, _)| instance_id.clone())
		.collect()
}

// ── Writes ──────────────────────────────────────────────────────────────────

/// Adds a group at the top of the list, matching the previous SQL behavior
/// where new groups got the lowest `display_order`. Favorites stays first.
pub async fn create(id: &str, name: &str) -> crate::Result<()> {
	with_write(|file| {
		let definition = GroupDefinition {
			id: id.to_string(),
			name: name.to_string(),
		};
		// `normalize` moves favorites back to index 0 afterwards.
		file.groups.insert(0, definition);
	}).await
}

/// Renames a group, returning the affected instance ids, or `None` when the
/// group does not exist.
pub async fn rename(id: &str, name: &str) -> crate::Result<Option<Vec<String>>> {
	with_write(|file| {
		let group = file.groups.iter_mut().find(|group| group.id == id)?;
		group.name = name.to_string();
		Some(instances_in_group(file, id))
	}).await
}

/// Deletes a group and strips it from every instance, returning the affected
/// instance ids, or `None` when the group does not exist.
pub async fn delete(id: &str) -> crate::Result<Option<Vec<String>>> {
	with_write(|file| {
		let index = file.groups.iter().position(|group| group.id == id)?;
		let affected = instances_in_group(file, id);
		file.groups.remove(index);
		// `normalize` prunes the now-dangling ids out of `memberships`.
		Some(affected)
	}).await
}

/// Reorders groups to match `group_ids`. Groups missing from the argument keep
/// their relative order at the end, so a stale frontend list can never drop a
/// group. Favorites is pinned first regardless.
pub async fn set_order(group_ids: &[String]) -> crate::Result<()> {
	with_write(|file| {
		let mut remaining = std::mem::take(&mut file.groups);
		let mut ordered = Vec::with_capacity(remaining.len());
		for id in group_ids {
			if id == FAVORITES_GROUP_ID {
				continue;
			}
			if let Some(index) = remaining.iter().position(|g| &g.id == id) {
				ordered.push(remaining.remove(index));
			}
		}
		ordered.extend(remaining);
		file.groups = ordered;
	}).await
}

/// Replaces the group list of each given instance wholesale.
pub async fn set_memberships(updates: &[(String, Vec<String>)]) -> crate::Result<()> {
	with_write(|file| {
		for (instance_id, group_ids) in updates {
			if group_ids.is_empty() {
				file.memberships.remove(instance_id);
			} else {
				file.memberships
					.insert(instance_id.clone(), group_ids.clone());
			}
		}
	}).await
}

/// Replaces the group list of a single instance.
pub async fn set_instance_groups(instance_id: &str, group_ids: &[String]) -> crate::Result<()> {
	set_memberships(&[(instance_id.to_string(), group_ids.to_vec())]).await
}

/// Looks a group up by name, creating it if absent, and returns its id.
/// Used by the legacy launcher import, which only knows group names.
pub async fn find_or_create_by_name(name: &str) -> crate::Result<String> {
	with_write(|file| {
		if let Some(group) =
			file.groups.iter().find(|group| group.name == name)
		{
			return group.id.clone();
		}
		let id = uuid::Uuid::new_v4().to_string();
		file.groups.push(GroupDefinition {
			id: id.clone(),
			name: name.to_string(),
		});
		id
	}).await
}

/// Adds one group to an instance, keeping its existing groups.
pub async fn add_instance_to_group(instance_id: &str, group_id: &str) -> crate::Result<()> {
	with_write(|file| {
		let entry = file
			.memberships
			.entry(instance_id.to_string())
			.or_default();
		if !entry.iter().any(|id| id == group_id) {
			entry.push(group_id.to_string());
		}
	}).await
}

/// Drops an instance's membership entry, e.g. after the instance is deleted.
pub async fn forget_instance(instance_id: &str) -> crate::Result<()> {
	with_write(|file| {
		file.memberships.remove(instance_id);
	}).await
}

/// Re-keys an instance's membership, e.g. when a `local:` id changes because the
/// instance directory moved.
pub async fn rename_instance(old_id: &str, new_id: &str) -> crate::Result<()> {
	if old_id == new_id {
		return Ok(());
	}
	with_write(|file| {
		if let Some(group_ids) = file.memberships.remove(old_id) {
			file.memberships.insert(new_id.to_string(), group_ids);
		}
	}).await
}

// ── One-time import ─────────────────────────────────────────────────────────

/// Imports the pre-existing SQLite tables and
/// instance sidecars into celestial.db on first startup.
///
/// The SQL access here is read-only and happens exactly once; afterwards the
/// tables are never touched again (they are kept as-is so no DB migration is
/// needed).
///
/// Groups now live in celestial.db. When its import marker is already set
/// (groups were carried over from the JSON sidecar, or imported here before),
/// the in-memory store is just seeded from there; the legacy SQLite path below
/// only runs for installs that never had `instance_groups.json` at all.
pub async fn ensure_imported(state: &State) -> crate::Result<()> {
	let _writer = WRITE_LOCK.lock().await;
	let already_in_db = state
		.celestial
		.get_meta(celestial_db::groups::IMPORT_FLAG)
		.await?
		.is_some();
	if already_in_db {
		return seed_store_from_db(state).await;
	}

	let mut file = InstanceGroupsFile {
		schema_version: INSTANCE_GROUPS_SCHEMA_VERSION,
		groups: Vec::new(),
		memberships: HashMap::new(),
	};

	// Deliberately the runtime API rather than the `query!` macro: these
	// statements exist only for this one-time import, and the macro would
	// require regenerating the whole `.sqlx` offline cache.
	let mut tx = state.pool.begin().await?;
	let rows = sqlx::query_as::<_, (String, String)>(
		"SELECT id, name FROM instance_groups ORDER BY display_order, name, id",
	)
	.fetch_all(&mut *tx)
	.await?;
	for (id, name) in rows {
		file.groups.push(GroupDefinition { id, name });
	}

	let memberships = sqlx::query_as::<_, (String, String)>(
		"SELECT instance_id, group_id FROM instance_group_memberships",
	)
	.fetch_all(&mut *tx)
	.await?;
	tx.commit().await?;
	for (instance_id, group_id) in memberships {
		file.memberships.entry(instance_id).or_default().push(group_id);
	}

	// JSON-backed instances kept their membership in the sidecars, which the
	// SQL tables never knew about.
	{
		let instances = crate::state::libraries::list_instances_from_json(state).await?;
		for instance in &instances {
			let dir = crate::state::libraries::resolve_instance_dir(
				state,
				&instance.path,
			);
			let groups = match crate::state::libraries::InstanceJson::read_from_dir(&dir)? {
				Some(json) => json.groups().to_vec(),
				// `.minecraft` instances without an `instance.json` use the
				// lighter `celestial.json` sidecar instead.
				_ => match crate::state::libraries::CelestialJson::read_from_dir(&dir)? {
					Some(celestial) => celestial.groups().to_vec(),
					_ => Vec::new(),
				},
			};
			if groups.is_empty() {
				continue;
			}
			let entry = file.memberships.entry(instance.id.clone()).or_default();
			for group_id in groups {
				if !entry.contains(&group_id) {
					entry.push(group_id);
				}
			}
		}
	}

	file.normalize();
	let group_count = file.groups.len();
	let instance_count = file.memberships.len();

	celestial_db::groups::save_imported(&state.celestial, &file).await?;
	*store().write().unwrap_or_else(|e| e.into_inner()) = file;
	STORE_READY.get_or_init(|| ());

	info!(
		"Imported {group_count} instance groups covering {instance_count} instances into celestial.db"
	);

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn file_with_groups(ids: &[&str]) -> InstanceGroupsFile {
		InstanceGroupsFile {
			schema_version: INSTANCE_GROUPS_SCHEMA_VERSION,
			groups: ids
				.iter()
				.map(|id| GroupDefinition {
					id: id.to_string(),
					name: id.to_string(),
				})
				.collect(),
			memberships: HashMap::new(),
		}
	}

	#[test]
	fn normalize_inserts_favorites_first() {
		let mut file = file_with_groups(&["a", "b"]);
		file.normalize();
		assert_eq!(file.groups[0].id, FAVORITES_GROUP_ID);
		assert_eq!(file.groups.len(), 3);
	}

	#[test]
	fn normalize_moves_favorites_to_front() {
		let mut file = file_with_groups(&["a", FAVORITES_GROUP_ID, "b"]);
		file.normalize();
		assert_eq!(
			file.groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
			vec![FAVORITES_GROUP_ID, "a", "b"]
		);
	}

	#[test]
	fn normalize_prunes_unknown_and_duplicate_memberships() {
		let mut file = file_with_groups(&["a"]);
		file.memberships.insert(
			"instance".to_string(),
			vec!["a".to_string(), "gone".to_string(), "a".to_string()],
		);
		file.memberships
			.insert("empty".to_string(), vec!["gone".to_string()]);
		file.normalize();
		assert_eq!(file.memberships["instance"], vec!["a".to_string()]);
		assert!(!file.memberships.contains_key("empty"));
	}

	#[tokio::test(flavor = "current_thread")]
	async fn failed_load_preserves_cache() {
		let dir = tempfile::tempdir().unwrap();
		let db = celestial_db::CelestialDb::connect(dir.path()).await.unwrap();
		let cache = RwLock::new(file_with_groups(&["keep"]));
		db.pool().close().await;
		assert!(seed_cache_from_db(&cache, &db).await.is_err());
		assert_eq!(cache.read().unwrap().groups[0].id, "keep");
	}

	#[tokio::test(flavor = "current_thread")]
	async fn failed_write_preserves_cache_and_database() {
		let dir = tempfile::tempdir().unwrap();
		let db = celestial_db::CelestialDb::connect(dir.path()).await.unwrap();
		let mut original = file_with_groups(&["keep"]);
		original.normalize();
		celestial_db::groups::save(&db, &original).await.unwrap();
		let cache = RwLock::new(original);
		let writers = Mutex::new(());
		sqlx::query("CREATE TRIGGER reject_groups BEFORE INSERT ON instance_groups BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
			.execute(db.pool()).await.unwrap();
		let result = persist_candidate(&cache, &writers, &db, |file| {
			file.groups[1].name = "changed".to_string();
		}).await;
		assert!(result.is_err());
		assert_eq!(cache.read().unwrap().groups[1].name, "keep");
		assert_eq!(celestial_db::groups::load(&db).await.unwrap().groups[1].name, "keep");
	}

	#[tokio::test(flavor = "current_thread")]
	async fn concurrent_writes_keep_both_changes_and_normalize() {
		let dir = tempfile::tempdir().unwrap();
		let db = celestial_db::CelestialDb::connect(dir.path()).await.unwrap();
		let cache = RwLock::new(file_with_groups(&[]));
		let writers = Mutex::new(());
		let add = |id: &str, file: &mut InstanceGroupsFile| {
			file.groups.push(GroupDefinition { id: id.into(), name: id.into() });
			file.memberships.insert(id.into(), vec![id.into(), "missing".into(), id.into()]);
		};
		let (first, second) = tokio::join!(
			persist_candidate(&cache, &writers, &db, |file| add("a", file)),
			persist_candidate(&cache, &writers, &db, |file| add("b", file)),
		);
		first.unwrap();
		second.unwrap();
		let loaded = celestial_db::groups::load(&db).await.unwrap();
		assert_eq!(loaded.groups[0].id, FAVORITES_GROUP_ID);
		assert_eq!(loaded.groups.len(), 3);
		assert_eq!(loaded.memberships["a"], vec!["a".to_string()]);
		assert_eq!(loaded.memberships["b"], vec!["b".to_string()]);
		assert_eq!(serde_json::to_value(&loaded).unwrap(), serde_json::to_value(&*cache.read().unwrap()).unwrap());
	}

}

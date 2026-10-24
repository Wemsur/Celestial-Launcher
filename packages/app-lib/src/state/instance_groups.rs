//! Instance grouping backed by the `instance_groups` / `group_memberships`
//! tables in celestial.db.
//!
//! Groups used to live in SQLite (`instance_groups` /
//! `instance_group_memberships` in app.db). That layout could not express
//! membership for JSON-backed instances (`local:` ids, i.e. every
//! `.minecraft` and multi-library instance) because they have no row in the
//! `instances` table, so they moved to `instance_groups.json`, and now to
//! celestial.db. The old app.db tables are left untouched but are never read
//! or written again (they are only read once, at import time).
//!
//! The public API stays synchronous: an in-memory cache is the runtime source
//! of truth and every mutation writes through to celestial.db.

use crate::state::State;
use crate::state::celestial_db;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};
use tracing::{info, warn};

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
    fn normalize(&mut self) {
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

/// Seeds the in-memory store from celestial.db. Called once per startup from
/// `ensure_imported`, before any read can observe an empty store (the lazy
/// default in `store()` only covers calls that race ahead of that).
async fn seed_store_from_db(state: &State) -> crate::Result<()> {
	let file = match celestial_db::groups::load(&state.celestial).await {
		Ok(file) => file,
		Err(error) => {
			warn!("Failed to load instance groups from celestial.db: {error}");
			InstanceGroupsFile::default()
		}
	};
	let lock = store();
	let mut guard = lock.write().unwrap_or_else(|e| e.into_inner());
	*guard = file;
	Ok(())
}

/// Persists the store to celestial.db. Runs synchronously via
/// `block_in_place` so a mutation is durable before its caller returns — a
/// spawned task would risk persisting two rapid mutations out of order.
fn write_to_disk(file: &InstanceGroupsFile) {
	let Some(state) = State::get_if_initialized() else {
		return;
	};
	let file = file.clone();
	let result = tokio::runtime::Handle::try_current().ok().and_then(|handle| {
		tokio::task::block_in_place(|| {
			handle
				.block_on(celestial_db::groups::save(&state.celestial, &file))
				.ok()
		})
	});
	if result.is_none() {
		warn!("Failed to save instance groups to celestial.db");
	}
}

fn store() -> &'static RwLock<InstanceGroupsFile> {
    STORE.get_or_init(|| RwLock::new(InstanceGroupsFile::default()))
}

fn with_read<T>(f: impl FnOnce(&InstanceGroupsFile) -> T) -> T {
    let guard = store().read().unwrap_or_else(|e| e.into_inner());
    f(&guard)
}

/// Mutates the store and persists the whole file. `f` returns the instance ids
/// whose membership changed, so callers can emit the change event.
fn with_write<T>(f: impl FnOnce(&mut InstanceGroupsFile) -> T) -> T {
    let mut guard = store().write().unwrap_or_else(|e| e.into_inner());
    let result = f(&mut guard);
    guard.normalize();
    write_to_disk(&guard);
    result
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
pub fn create(id: &str, name: &str) {
    with_write(|file| {
        let definition = GroupDefinition {
            id: id.to_string(),
            name: name.to_string(),
        };
        // `normalize` moves favorites back to index 0 afterwards.
        file.groups.insert(0, definition);
    });
}

/// Renames a group, returning the affected instance ids, or `None` when the
/// group does not exist.
pub fn rename(id: &str, name: &str) -> Option<Vec<String>> {
    with_write(|file| {
        let group = file.groups.iter_mut().find(|group| group.id == id)?;
        group.name = name.to_string();
        Some(instances_in_group(file, id))
    })
}

/// Deletes a group and strips it from every instance, returning the affected
/// instance ids, or `None` when the group does not exist.
pub fn delete(id: &str) -> Option<Vec<String>> {
    with_write(|file| {
        let index = file.groups.iter().position(|group| group.id == id)?;
        let affected = instances_in_group(file, id);
        file.groups.remove(index);
        // `normalize` prunes the now-dangling ids out of `memberships`.
        Some(affected)
    })
}

/// Reorders groups to match `group_ids`. Groups missing from the argument keep
/// their relative order at the end, so a stale frontend list can never drop a
/// group. Favorites is pinned first regardless.
pub fn set_order(group_ids: &[String]) {
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
    });
}

/// Replaces the group list of each given instance wholesale.
pub fn set_memberships(updates: &[(String, Vec<String>)]) {
    with_write(|file| {
        for (instance_id, group_ids) in updates {
            if group_ids.is_empty() {
                file.memberships.remove(instance_id);
            } else {
                file.memberships
                    .insert(instance_id.clone(), group_ids.clone());
            }
        }
    });
}

/// Replaces the group list of a single instance.
pub fn set_instance_groups(instance_id: &str, group_ids: &[String]) {
    set_memberships(&[(instance_id.to_string(), group_ids.to_vec())]);
}

/// Looks a group up by name, creating it if absent, and returns its id.
/// Used by the legacy launcher import, which only knows group names.
pub fn find_or_create_by_name(name: &str) -> String {
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
    })
}

/// Adds one group to an instance, keeping its existing groups.
pub fn add_instance_to_group(instance_id: &str, group_id: &str) {
    with_write(|file| {
        let entry = file
            .memberships
            .entry(instance_id.to_string())
            .or_default();
        if !entry.iter().any(|id| id == group_id) {
            entry.push(group_id.to_string());
        }
    });
}

/// Drops an instance's membership entry, e.g. after the instance is deleted.
pub fn forget_instance(instance_id: &str) {
    with_write(|file| {
        file.memberships.remove(instance_id);
    });
}

/// Re-keys an instance's membership, e.g. when a `local:` id changes because the
/// instance directory moved.
pub fn rename_instance(old_id: &str, new_id: &str) {
    if old_id == new_id {
        return;
    }
    with_write(|file| {
        if let Some(group_ids) = file.memberships.remove(old_id) {
            file.memberships.insert(new_id.to_string(), group_ids);
        }
    });
}

// ── One-time import ─────────────────────────────────────────────────────────

/// Creates `instance_groups.json` from the pre-existing SQLite tables and
/// instance sidecars, the first time the app runs after this change.
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
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, name FROM instance_groups ORDER BY display_order, name, id",
    )
    .fetch_all(&state.pool)
    .await?;
    for (id, name) in rows {
        file.groups.push(GroupDefinition { id, name });
    }

    let memberships = sqlx::query_as::<_, (String, String)>(
        "SELECT instance_id, group_id FROM instance_group_memberships",
    )
    .fetch_all(&state.pool)
    .await?;
    for (instance_id, group_id) in memberships {
        file.memberships.entry(instance_id).or_default().push(group_id);
    }

    // JSON-backed instances kept their membership in the sidecars, which the
    // SQL tables never knew about.
    if let Ok(instances) = crate::state::libraries::list_instances_from_json(state).await {
        for instance in &instances {
            let dir = crate::state::libraries::resolve_instance_dir(
                state,
                &instance.path,
            );
            let groups = match crate::state::libraries::InstanceJson::read_from_dir(&dir) {
                Ok(Some(json)) => json.groups().to_vec(),
                // `.minecraft` instances without an `instance.json` use the
                // lighter `celestial.json` sidecar instead.
                _ => match crate::state::libraries::CelestialJson::read_from_dir(&dir) {
                    Ok(Some(celestial)) => celestial.groups().to_vec(),
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

    // Seed the cache with the imported data, then persist to celestial.db and
    // mark the import done so this path never runs again.
    {
        let lock = store();
        let mut guard = lock.write().unwrap_or_else(|e| e.into_inner());
        *guard = file.clone();
    }
    celestial_db::groups::save(&state.celestial, &file).await?;
    state
        .celestial
        .set_meta(celestial_db::groups::IMPORT_FLAG, "done")
        .await?;

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
}

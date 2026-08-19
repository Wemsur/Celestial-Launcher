use crate::State;
use crate::state::instances::adapters::{filesystem, sqlite};
use crate::state::instances::{Instance, InstanceFile};
use crate::state::{libraries, ProjectType};
use chrono::Utc;
use sha1_smol::Sha1;
use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::PathBuf;
use tokio::task::spawn_blocking;
use uuid::Uuid;

const CONTENT_CACHE_VERSION: &str = "v1";
const CONTENT_CACHE_FILE_NAME: &str = "content_cache.json";

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ContentCacheFile {
    version: String,
    scanned_at: u64,
    files: Vec<ContentCacheEntry>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ContentCacheEntry {
    relative_path: String,
    file_name: String,
    enabled: bool,
    size: u64,
    hash_cache_key: String,
    sha1: String,
}

pub(crate) async fn sync_content_files(
    instance_id: &str,
    state: &State,
) -> crate::Result<Vec<InstanceFile>> {
    tracing::info!("sync_content_files called for instance '{}'", instance_id);
    let instance =
        sqlite::instance_rows::get_instance_by_id(instance_id, &state.pool)
            .await?;

    let instance = match instance {
        Some(inst) => inst,
        None => {
            // JSON-backed instances: find the Instance struct and sync it directly
            let json_instances =
                libraries::list_instances_from_json(state).await?;
            if let Some(inst) = json_instances
                .into_iter()
                .find(|i| i.id == instance_id)
            {
                tracing::info!(
                    "sync_content_files: found JSON instance, syncing {} files",
                    inst.id
                );
                return sync_instance_content_files(&inst, state).await;
            }
            tracing::warn!(
                "sync_content_files: instance '{}' not found in DB or JSON",
                instance_id
            );
            return Err(
                crate::ErrorKind::InputError("Unknown instance".to_string())
                    .into(),
            );
        }
    };

    sync_instance_content_files(&instance, state).await
}

pub(crate) async fn sync_instance_content_files(
    instance: &Instance,
    state: &State,
) -> crate::Result<Vec<InstanceFile>> {
    if instance.is_json_backed() {
        sync_json_instance_content_files(instance, state).await
    } else {
        sync_db_instance_content_files(instance, state).await
    }
}

// ─── JSON-backed path: file-based cache ──────────────────────────────────────

/// Loads content for a JSON-backed instance from its local JSON cache, falling
/// back to a full filesystem scan if no cache exists or it is stale.
async fn sync_json_instance_content_files(
    instance: &Instance,
    state: &State,
) -> crate::Result<Vec<InstanceFile>> {
    let instance_dir =
        libraries::resolve_instance_dir(state, &instance.path);
    let cache_path = instance_dir.join(CONTENT_CACHE_FILE_NAME);

    // Try loading cached data first — this is near-instant.
    if let Some(cache) = load_content_cache(&cache_path)? {
        let cached_len = cache.files.len();
        let instance_files: Vec<InstanceFile> = cache
            .files
            .into_iter()
            .map(|entry| InstanceFile {
                id: cache_instance_file_id(&entry),
                instance_id: instance.id.clone(),
                relative_path: entry.relative_path,
                file_name: entry.file_name,
                enabled: entry.enabled,
                sha1: entry.sha1,
                size: entry.size,
                missing: false,
                added_at: Utc::now(),
                modified_at: Utc::now(),
            })
            .collect();

        if instance_files.len() == cached_len {
            tracing::info!(
                "sync_json_content_files: loaded {} files from cache for '{}'",
                instance_files.len(),
                instance.id,
            );
            // Fire-and-forget background refresh so subsequent calls get fresh
            // data without blocking the UI.
            spawn_content_refresh(instance.id.clone(), cache_path);
            return Ok(instance_files);
        }
        // Stale cache — fall through to full rescan below.
    }

    // Full filesystem scan (slow path, result is cached for next time).
    sync_json_instance_content_files_internal(instance, state, cache_path)
        .await
}

/// Background task: re-scan the filesystem and overwrite the cache.
fn spawn_content_refresh(instance_id: String, cache_path: PathBuf) {
    tokio::spawn(async move {
        let state = match State::get().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "Background content refresh failed to get state for '{}': {}",
                    instance_id,
                    e
                );
                return;
            }
        };
        if let Err(e) =
            sync_json_instance_content_files_internal_from_id(&instance_id, &state, cache_path).await
        {
            tracing::warn!(
                "Background content refresh failed for '{}': {}",
                instance_id,
                e
            );
        }
    });
}

async fn sync_json_instance_content_files_internal_from_id(
    instance_id: &str,
    state: &State,
    cache_path: PathBuf,
) -> crate::Result<Vec<InstanceFile>> {
    let json_instances = libraries::list_instances_from_json(state).await?;
    let instance = json_instances
        .into_iter()
        .find(|i| i.id == instance_id)
        .ok_or_else(|| {
            crate::ErrorKind::InputError(format!(
                "Instance '{}' not found for background refresh",
                instance_id
            ))
        })?;
    sync_json_instance_content_files_internal(&instance, state, cache_path).await
}

async fn sync_json_instance_content_files_internal(
    instance: &Instance,
    state: &State,
    cache_path: PathBuf,
) -> crate::Result<Vec<InstanceFile>> {
    let _content_lock = state.lock_instance_content(&instance.id).await;
    let instance_dir =
        libraries::resolve_instance_dir(state, &instance.path);
    tracing::info!(
        "sync_json_content_files: scanning directory '{}'",
        instance_dir.display()
    );
    let scanned = filesystem::scan_content_files(&instance_dir)?;
    tracing::info!(
        "sync_json_content_files: found {} files for instance '{}'",
        scanned.len(),
        instance.id
    );

    let hashes_by_key: HashMap<String, String> = spawn_blocking({
        let scanned = scanned.clone();
        let instance_dir = instance_dir.clone();
        move || {
            let mut result = HashMap::with_capacity(scanned.len());
            for file in &scanned {
                let path = instance_dir.join(&file.relative_path);
                match std::fs::File::open(&path) {
                    Ok(mut f) => {
                        let mut hasher = Sha1::new();
                        let mut buf = Vec::new();
                        if f.read_to_end(&mut buf).is_ok() {
                            hasher.update(&buf);
                            result.insert(
                                file.hash_cache_key.clone(),
                                hasher.digest().to_string(),
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "sync: failed to open file for hashing: {} ({})",
                            path.display(),
                            e
                        );
                    }
                }
            }
            result
        }
    })
    .await?;
    tracing::info!(
        "sync_json_content_files: computed {} hashes out of {} scanned files",
        hashes_by_key.len(),
        scanned.len()
    );

    let now = Utc::now();
    let mut cache_entries = Vec::with_capacity(scanned.len());
    for file in scanned {
        let hash_key =
            file.hash_cache_key.trim_end_matches(".disabled");
        let Some(hash) = hashes_by_key.get(hash_key) else {
            continue;
        };
        cache_entries.push(ContentCacheEntry {
            relative_path: file.relative_path,
            file_name: file.file_name,
            enabled: file.enabled,
            size: file.size,
            hash_cache_key: hash_key.to_string(),
            sha1: hash.clone(),
        });
    }

    // Persist cache so next open is instant.
    let cache = ContentCacheFile {
        version: CONTENT_CACHE_VERSION.to_string(),
        scanned_at: now.timestamp_millis() as u64,
        files: cache_entries.clone(),
    };
    let cache_json =
        serde_json::to_string_pretty(&cache).map_err(|e| {
            crate::ErrorKind::OtherError(format!(
                "Failed to serialize content cache: {e}"
            ))
        })?;
    if let Err(e) = std::fs::write(&cache_path, cache_json) {
        tracing::warn!(
            "sync_json_content_files: failed to write cache at '{}': {}",
            cache_path.display(),
            e
        );
    }

    let files = cache_entries
        .into_iter()
        .map(|entry| InstanceFile {
            id: cache_instance_file_id(&entry),
            instance_id: instance.id.clone(),
            relative_path: entry.relative_path,
            file_name: entry.file_name,
            enabled: entry.enabled,
            sha1: entry.sha1,
            size: entry.size,
            missing: false,
            added_at: now,
            modified_at: now,
        })
        .collect();

    Ok(files)
}

// ─── DB-backed path: keeps existing SQLite logic ─────────────────────────────

async fn sync_db_instance_content_files(
    instance: &Instance,
    state: &State,
) -> crate::Result<Vec<InstanceFile>> {
    let _content_lock = state.lock_instance_content(&instance.id).await;
    let instance_dir =
        libraries::resolve_instance_dir(state, &instance.path);
    tracing::info!(
        "sync_db_content_files: scanning directory '{}'",
        instance_dir.display()
    );
    let scanned = filesystem::scan_content_files(&instance_dir)?;
    tracing::info!(
        "sync_db_content_files: found {} files for instance '{}'",
        scanned.len(),
        instance.id
    );
    let hashes_by_key: HashMap<String, String> = spawn_blocking({
        let scanned = scanned.clone();
        let instance_dir = instance_dir.clone();
        move || {
            let mut result = HashMap::with_capacity(scanned.len());
            for file in &scanned {
                let path = instance_dir.join(&file.relative_path);
                match std::fs::File::open(&path) {
                    Ok(mut f) => {
                        let mut hasher = Sha1::new();
                        let mut buf = Vec::new();
                        if f.read_to_end(&mut buf).is_ok() {
                            hasher.update(&buf);
                            result.insert(
                                file.hash_cache_key.clone(),
                                hasher.digest().to_string(),
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "sync: failed to open file for hashing: {} ({})",
                            path.display(),
                            e
                        );
                    }
                }
            }
            result
        }
    })
    .await?;
    tracing::info!(
        "sync_db_content_files: computed {} hashes out of {} scanned files",
        hashes_by_key.len(),
        scanned.len()
    );
    let existing_files =
        sqlite::content_rows::get_instance_files(&instance.id, &state.pool)
            .await?;
    let scanned_paths = scanned
        .iter()
        .map(|file| file.relative_path.clone())
        .collect::<HashSet<_>>();
    let missing_file_ids = existing_files
        .iter()
        .filter(|file| {
            !file.missing
                && !scanned_paths.contains(&file.relative_path)
        })
        .map(|file| file.id.clone())
        .collect::<Vec<_>>();
    let existing_files_by_path = existing_files
        .into_iter()
        .map(|file| (file.relative_path.clone(), file))
        .collect::<HashMap<_, _>>();

    let now = Utc::now();
    let mut files = Vec::new();
    let mut present_without_hash_ids = Vec::new();
    let mut restored_without_hash = false;

    for file in scanned {
        let hash_key =
            file.hash_cache_key.trim_end_matches(".disabled");
        let existing_file = existing_files_by_path.get(&file.relative_path);
        let Some(hash) = hashes_by_key.get(hash_key) else {
            if let Some(existing_file) = existing_file {
                present_without_hash_ids
                    .push(existing_file.id.clone());
                restored_without_hash |= existing_file.missing;
            }
            continue;
        };

        files.push(InstanceFile {
            id: existing_file
                .map(|file| file.id.clone())
                .unwrap_or_else(instance_file_id),
            instance_id: instance.id.clone(),
            relative_path: file.relative_path,
            file_name: file.file_name,
            enabled: file.enabled,
            sha1: hash.clone(),
            size: file.size,
            missing: false,
            added_at: existing_file
                .map(|file| file.added_at)
                .unwrap_or(now),
            modified_at: now,
        });
    }

    let content_changed = !missing_file_ids.is_empty()
        || restored_without_hash
        || files.iter().any(|file| {
            existing_files_by_path
                .get(&file.relative_path)
                .is_none_or(|existing| {
                    existing.missing
                        || existing.enabled != file.enabled
                        || existing.sha1 != file.sha1
                        || existing.size != file.size
                })
        });

    let mut tx = state.pool.begin().await?;
    for file_id in missing_file_ids {
        sqlite::content_rows::set_instance_file_missing(
            &file_id, true, &mut tx,
        )
        .await?;
    }

    let mut stored_files = Vec::with_capacity(
        files.len() + present_without_hash_ids.len(),
    );
    for file_id in present_without_hash_ids {
        if let Some(file) =
            sqlite::content_rows::set_instance_file_missing(
                &file_id, false, &mut tx,
            )
            .await?
        {
            stored_files.push(file);
        }
    }
    for file in &files {
        stored_files.push(
            sqlite::content_rows::upsert_instance_file(
                file, &mut tx,
            )
            .await?,
        );
    }

    tx.commit().await?;

    if content_changed {
        super::mark_shared_instance_stale(&instance.id, &state.pool).await?;
    }

    Ok(stored_files)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn load_content_cache(
    path: &std::path::Path,
) -> crate::Result<Option<ContentCacheFile>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| {
            crate::ErrorKind::OtherError(format!(
                "Failed to read content cache at '{}': {}",
                path.display(),
                e
            ))
        })?;
    let cache: ContentCacheFile = serde_json::from_str(&content)
        .map_err(|e| {
            crate::ErrorKind::OtherError(format!(
                "Failed to parse content cache at '{}': {}",
                path.display(),
                e
            ))
        })?;
    if cache.version != CONTENT_CACHE_VERSION {
        tracing::info!(
            "content cache at '{}' has incompatible version '{}', ignoring",
            path.display(),
            cache.version,
        );
        return Ok(None);
    }
    Ok(Some(cache))
}

/// Stable per-file ID derived from relative path so the frontend always sees
/// the same entry for the same file.
fn cache_instance_file_id(entry: &ContentCacheEntry) -> String {
    let mut hasher = sha1_smol::Sha1::new();
    hasher.update(entry.relative_path.as_bytes());
    format!(
        "json-instance-file:{}",
        hasher.digest().to_string()
    )
}

fn instance_file_id() -> String {
    format!("instance-file:{}", Uuid::new_v4())
}

pub(crate) fn project_type_for_file(
    file: &InstanceFile,
) -> Option<ProjectType> {
    filesystem::project_type_from_relative_path(&file.relative_path)
}

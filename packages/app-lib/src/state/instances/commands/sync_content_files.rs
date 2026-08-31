use crate::State;
use crate::state::instances::adapters::{filesystem, sqlite};
use crate::state::instances::{ContentItem, Instance, InstanceFile};
use crate::state::{libraries, ProjectType};
use chrono::Utc;
use sha1_smol::Sha1;
use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::task::spawn_blocking;
use uuid::Uuid;

const CONTENT_CACHE_VERSION: &str = "v1";
const CONTENT_CACHE_FILE_NAME: &str = "content_cache.json";

/// Read buffer for streaming SHA1. Files are hashed incrementally rather than
/// slurped: a modpack instance can hold 500 MiB of jars, and `read_to_end` on
/// every one of them is both a memory spike and slower than a fixed buffer that
/// stays in cache.
const HASH_READ_BUFFER: usize = 512 * 1024;

/// Upper bound on concurrent hashing tasks. SHA1 of a large jar is CPU bound
/// once the bytes are in the page cache, so this scales with cores, but there is
/// no point saturating every core of a 32-thread machine for a background job.
const MAX_HASH_TASKS: usize = 8;

/// Minimum gap between two background refreshes of the same instance. Without
/// it, every navigation to an instance page (and every query the page fires)
/// kicks off its own full rescan of the same directory.
const BACKGROUND_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// When each instance last had a background content refresh started.
static LAST_BACKGROUND_REFRESH: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Schema of the resolved-items side of the content cache. Bump when the shape
/// of `ContentItem` changes in a way that makes older entries wrong; a mismatch
/// is treated as a miss, never as an error.
const CONTENT_ITEMS_SCHEMA: u32 = 1;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ContentCacheFile {
    version: String,
    scanned_at: u64,
    files: Vec<ContentCacheEntry>,
    /// Set when the file list is known to be out of date but the recorded
    /// hashes are still worth keeping. Readers must rescan; hashers may still
    /// reuse `sha1` for files whose size and mtime are unchanged.
    #[serde(default)]
    stale: bool,
    /// Fully resolved content items for the file list in `files`, so a second
    /// open needs neither SQLite nor the network. Kept honest by a fingerprint
    /// rather than a timestamp — see [`load_cached_content_items`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    items: Option<ContentItemsCache>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ContentItemsCache {
    schema: u32,
    /// Fingerprint of the file list these items were derived from. The items are
    /// only reusable while it still matches the cached file list.
    files_fingerprint: String,
    items: Vec<ContentItem>,
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

/// Whether a content sync may answer from the on-disk cache.
///
/// The cache lives next to the instance and is only consulted for JSON-backed
/// instances. `Bypass` is required whenever the caller must observe writes that
/// just happened — reading the cache would hand back the pre-write file list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContentSyncFreshness {
    UseCache,
    Bypass,
}

impl ContentSyncFreshness {
    /// `MustRevalidate` means "ignore every cache", including this one.
    pub(crate) fn from_cache_behaviour(
        cache_behaviour: Option<crate::state::CacheBehaviour>,
    ) -> Self {
        if matches!(
            cache_behaviour,
            Some(crate::state::CacheBehaviour::MustRevalidate)
        ) {
            Self::Bypass
        } else {
            Self::UseCache
        }
    }
}

pub(crate) async fn sync_content_files(
    instance_id: &str,
    state: &State,
) -> crate::Result<Vec<InstanceFile>> {
    sync_content_files_with_freshness(
        instance_id,
        ContentSyncFreshness::UseCache,
        state,
    )
    .await
}

/// Force a filesystem rescan of an instance, ignoring the on-disk cache.
///
/// The file watcher must use this: it fires *because* the directory changed, so
/// answering from the cache written before that change hands back the old list.
/// Rescanning is cheap now that unchanged files reuse their recorded SHA1.
pub(crate) async fn resync_content_files(
    instance_id: &str,
    state: &State,
) -> crate::Result<Vec<InstanceFile>> {
    sync_content_files_with_freshness(
        instance_id,
        ContentSyncFreshness::Bypass,
        state,
    )
    .await
}

async fn sync_content_files_with_freshness(
    instance_id: &str,
    freshness: ContentSyncFreshness,
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
                return sync_instance_content_files_with_freshness(
                    &inst, freshness, state,
                )
                .await;
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

    sync_instance_content_files_with_freshness(&instance, freshness, state).await
}

pub(crate) async fn sync_instance_content_files_with_freshness(
    instance: &Instance,
    freshness: ContentSyncFreshness,
    state: &State,
) -> crate::Result<Vec<InstanceFile>> {
    if instance.is_json_backed() {
        sync_json_instance_content_files(instance, freshness, state).await
    } else {
        sync_db_instance_content_files(instance, state).await
    }
}

/// Path of the JSON-backed content cache for an instance.
fn content_cache_path(instance: &Instance, state: &State) -> PathBuf {
    libraries::resolve_instance_dir(state, &instance.path)
        .join(CONTENT_CACHE_FILE_NAME)
}

/// Mark the cached content list as out of date so the next sync rescans.
///
/// Every code path that adds, removes, or renames a content file must call this,
/// otherwise readers keep seeing the pre-write list until a background refresh
/// happens to land.
///
/// The file is flagged rather than deleted. Deleting it would also throw away
/// every recorded SHA1, so installing a single mod would force a full re-hash of
/// the whole instance on the next open; keeping the entries lets the rescan reuse
/// the hashes of the files that did not change. The resolved item list is
/// dropped, since it describes a file list that no longer exists.
pub(crate) async fn invalidate_content_cache(
    instance: &Instance,
    state: &State,
) {
    if !instance.is_json_backed() {
        return;
    }
    forget_background_refresh(&instance.id);
    let path = content_cache_path(instance, state);
    match load_content_cache(&path) {
        Ok(Some(mut cache)) => {
            cache.stale = true;
            cache.items = None;
            if let Err(error) = write_content_cache(&path, &cache) {
                tracing::warn!(
                    "Failed to flag content cache at '{}' as stale: {error}",
                    path.display(),
                );
            }
        }
        // Nothing cached, or a cache we cannot parse: removing it is the
        // desired end state either way.
        Ok(None) | Err(_) => match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                "Failed to invalidate content cache at '{}': {error}",
                path.display(),
            ),
        },
    }
}

// ─── Resolved item cache (skips SQLite and the network on a second open) ──────

/// Fingerprint of a synced file list.
///
/// Identifies *what* the resolved items were derived from, rather than *when* —
/// a timestamp would have to be matched against a snapshot that a background
/// refresh can replace at any moment, whereas this compares the lists
/// themselves. Order-insensitive; covers everything that can change an item.
pub(crate) fn instance_files_fingerprint(files: &[InstanceFile]) -> String {
    fingerprint(files.iter().filter(|file| !file.missing).map(|file| {
        (file.relative_path.as_str(), file.sha1.as_str(), file.enabled)
    }))
}

fn fingerprint<'a>(
    entries: impl Iterator<Item = (&'a str, &'a str, bool)>,
) -> String {
    let mut keys = entries.collect::<Vec<_>>();
    keys.sort_unstable();
    let mut hasher = Sha1::new();
    for (path, sha1, enabled) in keys {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(sha1.as_bytes());
        hasher.update(if enabled { b"\x01" } else { b"\x00" });
    }
    hasher.digest().to_string()
}

fn cache_files_fingerprint(entries: &[ContentCacheEntry]) -> String {
    fingerprint(entries.iter().map(|entry| {
        (
            entry.relative_path.as_str(),
            entry.sha1.as_str(),
            entry.enabled,
        )
    }))
}

/// Resolved items for the file list currently on disk, if a previous run stored
/// them for exactly that list.
pub(crate) fn load_cached_content_items(
    instance: &Instance,
    state: &State,
) -> Option<Vec<ContentItem>> {
    if !instance.is_json_backed() {
        return None;
    }
    let cache =
        load_content_cache(&content_cache_path(instance, state)).ok()??;
    if cache.stale {
        return None;
    }
    let expected = cache_files_fingerprint(&cache.files);
    let items = cache.items?;
    (items.schema == CONTENT_ITEMS_SCHEMA
        && items.files_fingerprint == expected)
        .then_some(items.items)
}

/// Store resolved items against the file list they were derived from.
///
/// Awaited rather than fire-and-forget: a background completion pass notifies the
/// frontend as soon as it finishes, and the refetch that follows reads this cache.
/// Returning before the write landed would hand back the very data the pass just
/// replaced. A no-op when the cached file list has moved on, so a slow resolve can
/// never publish items for a list that has already been replaced.
pub(crate) async fn store_cached_content_items(
    instance: &Instance,
    state: &State,
    files_fingerprint: String,
    items: Vec<ContentItem>,
) {
    if !instance.is_json_backed() {
        return;
    }
    let path = content_cache_path(instance, state);
    let write = tokio::task::spawn_blocking(move || {
        let Ok(Some(mut cache)) = load_content_cache(&path) else {
            return;
        };
        if cache.stale
            || cache_files_fingerprint(&cache.files) != files_fingerprint
        {
            return;
        }
        cache.items = Some(ContentItemsCache {
            schema: CONTENT_ITEMS_SCHEMA,
            files_fingerprint,
            items,
        });
        if let Err(error) = write_content_cache(&path, &cache) {
            tracing::warn!(
                "Failed to store resolved content items at '{}': {error}",
                path.display(),
            );
        }
    })
    .await;
    if let Err(error) = write {
        tracing::warn!("Content item cache write task failed: {error}");
    }
}

/// Kick the throttled background rescan for an instance whose file list was not
/// read through [`sync_instance_content_files_with_freshness`].
///
/// The resolved-item fast path returns before the sync runs, so it has to
/// arrange the revalidation itself — otherwise an instance that always hits the
/// item cache would never notice a file appearing on disk.
pub(crate) fn spawn_content_refresh_for_instance(
    instance: &Instance,
    state: &State,
) {
    if !instance.is_json_backed() {
        return;
    }
    let path = content_cache_path(instance, state);
    let Ok(Some(cache)) = load_content_cache(&path) else {
        return;
    };
    if cache.stale {
        return;
    }
    spawn_content_refresh(instance.id.clone(), path, cache.scanned_at);
}

// ─── JSON-backed path: file-based cache ──────────────────────────────────────

/// Loads content for a JSON-backed instance from its local JSON cache, falling
/// back to a full filesystem scan if the cache is absent or bypassed.
async fn sync_json_instance_content_files(
    instance: &Instance,
    freshness: ContentSyncFreshness,
    state: &State,
) -> crate::Result<Vec<InstanceFile>> {
    let cache_path = content_cache_path(instance, state);

    // Try loading cached data first — this is near-instant.
    let t_cache = std::time::Instant::now();
    if freshness == ContentSyncFreshness::UseCache
        && let Some(cache) = load_content_cache(&cache_path)?
        && !cache.stale
    {
        let scanned_at = cache.scanned_at;
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

        tracing::info!(
            "content_timing: [3a] content_cache.json HIT {} ms ({} files) for '{}'",
            t_cache.elapsed().as_millis(),
            instance_files.len(),
            instance.id,
        );
        // Fire-and-forget background refresh so subsequent calls get fresh
        // data without blocking the UI. Throttled: several queries can ask for
        // the same instance's content within a second of each other.
        spawn_content_refresh(instance.id.clone(), cache_path, scanned_at);
        return Ok(instance_files);
    }

    // Full filesystem scan (slow path, result is cached for next time).
    tracing::info!(
        "content_timing: [3a] content_cache.json MISS (freshness={:?}) for '{}', doing full scan",
        freshness,
        instance.id,
    );
    sync_json_instance_content_files_internal(instance, state, cache_path, None)
        .await
}

/// Whether a background refresh for this instance may start now.
///
/// Records the start time on success, so concurrent callers cannot both pass.
fn claim_background_refresh(instance_id: &str) -> bool {
    let Ok(mut last) = LAST_BACKGROUND_REFRESH.lock() else {
        // A poisoned lock only means some earlier task panicked; skipping the
        // refresh is the safe reading.
        return false;
    };
    let now = Instant::now();
    let recent = last.get(instance_id).is_some_and(|previous| {
        now.duration_since(*previous) < BACKGROUND_REFRESH_INTERVAL
    });
    if recent {
        return false;
    }
    last.insert(instance_id.to_string(), now);
    true
}

/// Clear the throttle so the next read refreshes immediately. Called when the
/// content is known to have changed, where waiting out the interval would mean
/// serving a list we already know is wrong.
fn forget_background_refresh(instance_id: &str) {
    if let Ok(mut last) = LAST_BACKGROUND_REFRESH.lock() {
        last.remove(instance_id);
    }
}

/// Background task: re-scan the filesystem and overwrite the cache.
///
/// `expected_scanned_at` is the stamp of the cache this refresh was started
/// from; the rescan is only published if that cache is still the current one.
fn spawn_content_refresh(
    instance_id: String,
    cache_path: PathBuf,
    expected_scanned_at: u64,
) {
    if !claim_background_refresh(&instance_id) {
        tracing::debug!(
            "content_timing: [bg] skipping background refresh of '{}' (throttled)",
            instance_id
        );
        return;
    }
    tokio::spawn(async move {
        let started = std::time::Instant::now();
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
        if let Err(e) = sync_json_instance_content_files_internal_from_id(
            &instance_id,
            &state,
            cache_path,
            Some(expected_scanned_at),
        )
        .await
        {
            tracing::warn!(
                "Background content refresh failed for '{}': {}",
                instance_id,
                e
            );
        }
        tracing::info!(
            "content_timing: [bg] background content refresh {} ms for '{}'",
            started.elapsed().as_millis(),
            instance_id
        );
    });
}

async fn sync_json_instance_content_files_internal_from_id(
    instance_id: &str,
    state: &State,
    cache_path: PathBuf,
    expected_scanned_at: Option<u64>,
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
    sync_json_instance_content_files_internal(
        &instance,
        state,
        cache_path,
        expected_scanned_at,
    )
    .await
}

async fn sync_json_instance_content_files_internal(
    instance: &Instance,
    state: &State,
    cache_path: PathBuf,
    expected_scanned_at: Option<u64>,
) -> crate::Result<Vec<InstanceFile>> {
    let t_lock = std::time::Instant::now();
    let _content_lock = state.lock_instance_content(&instance.id).await;
    let lock_wait = t_lock.elapsed();
    if lock_wait.as_millis() > 50 {
        tracing::info!(
            "content_timing: [3b-lock] waited {} ms for the content lock of '{}'",
            lock_wait.as_millis(),
            instance.id
        );
    }
    let instance_dir =
        libraries::resolve_instance_dir(state, &instance.path);
    let shared_dirs = libraries::shared_content_dirs(
        &instance_dir,
        &instance.library_format,
    );
    tracing::info!(
        "sync_json_content_files: scanning directory '{}'",
        instance_dir.display()
    );
    let t_scan = std::time::Instant::now();
    let scanned =
        filesystem::scan_content_files(&instance_dir, &shared_dirs)?;
    tracing::info!(
        "content_timing: [3b] scan_content_files {} ms ({} files) for '{}'",
        t_scan.elapsed().as_millis(),
        scanned.len(),
        instance.id
    );

    // Hashes recorded by the previous scan. Reusable because the key embeds the
    // file's size and mtime, so a hit proves the bytes are unchanged.
    let previous = load_content_cache(&cache_path).ok().flatten();
    let known_hashes = previous
        .as_ref()
        .map(|cache| {
            cache
                .files
                .iter()
                .map(|entry| {
                    (entry.hash_cache_key.clone(), entry.sha1.clone())
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    let hashes_by_key = hash_scanned_files(
        &instance.id,
        &instance_dir,
        &scanned,
        &known_hashes,
    )
    .await?;

    let now = Utc::now();
    let mut cache_entries = Vec::with_capacity(scanned.len());
    for file in scanned {
        // Owned, and the hash copied out, so nothing still borrows `file` when
        // its fields move into the entry below.
        let hash_key = hash_key_for(&file).to_string();
        let Some(sha1) = hashes_by_key.get(&hash_key).cloned() else {
            continue;
        };
        cache_entries.push(ContentCacheEntry {
            relative_path: file.relative_path,
            file_name: file.file_name,
            enabled: file.enabled,
            size: file.size,
            hash_cache_key: hash_key,
            sha1,
        });
    }

    // Persist cache so next open is instant. A background refresh must not
    // resurrect a list that was invalidated (or already replaced) while it was
    // scanning: it only publishes when the cache it started from is still there,
    // unflagged, and unchanged.
    let current = load_content_cache(&cache_path).ok().flatten();
    let may_publish = match expected_scanned_at {
        None => true,
        Some(expected) => current
            .as_ref()
            .is_some_and(|cache| !cache.stale && cache.scanned_at == expected),
    };
    if may_publish {
        // Resolved items survive a rescan only when the rescan found exactly the
        // same files; otherwise they describe a list that no longer exists.
        let new_fingerprint = cache_files_fingerprint(&cache_entries);
        let items = current.and_then(|cache| cache.items).filter(|items| {
            items.files_fingerprint == new_fingerprint
        });
        let cache = ContentCacheFile {
            version: CONTENT_CACHE_VERSION.to_string(),
            scanned_at: now.timestamp_millis() as u64,
            files: cache_entries.clone(),
            stale: false,
            items,
        };
        if let Err(e) = write_content_cache(&cache_path, &cache) {
            tracing::warn!(
                "sync_json_content_files: failed to write cache at '{}': {}",
                cache_path.display(),
                e
            );
        }
    } else {
        tracing::info!(
            "sync_json_content_files: dropping stale background refresh for '{}'",
            instance.id,
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
    let shared_dirs = libraries::shared_content_dirs(
        &instance_dir,
        &instance.library_format,
    );
    tracing::info!(
        "sync_db_content_files: scanning directory '{}'",
        instance_dir.display()
    );
    let t_scan = std::time::Instant::now();
    let scanned =
        filesystem::scan_content_files(&instance_dir, &shared_dirs)?;
    tracing::info!(
        "content_timing: [3b] scan_content_files (db) {} ms ({} files) for '{}'",
        t_scan.elapsed().as_millis(),
        scanned.len(),
        instance.id
    );
    // No hash reuse on this path: the DB rows carry no mtime, so a row cannot
    // prove a file is unchanged (a same-size edit would slip through). It still
    // gets the parallel, streaming hasher.
    let hashes_by_key = hash_scanned_files(
        &instance.id,
        &instance_dir,
        &scanned,
        &HashMap::new(),
    )
    .await?;
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
        let hash_key = hash_key_for(&file);
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

/// Key a scanned file's SHA1 is stored under.
///
/// `hash_cache_key` is `v2-{size}-{mtime_ns}-{relative_path}`, so dropping the
/// `.disabled` suffix makes a mod hash the same whether it is enabled or not —
/// toggling only renames the file, it does not change its bytes.
///
/// Both the insert and the lookup must use this form. They used not to, which is
/// why every `.disabled` file was silently dropped from the content list.
fn hash_key_for(file: &filesystem::ScannedContentFile) -> &str {
    file.hash_cache_key.trim_end_matches(".disabled")
}

/// SHA1 for every scanned file, keyed by [`hash_key_for`].
///
/// Files whose key is already in `known` are not read at all: the key embeds
/// size and mtime, so a hit proves the contents are unchanged. The rest are
/// hashed across several blocking tasks, streaming through a fixed buffer.
async fn hash_scanned_files(
    instance_id: &str,
    instance_dir: &Path,
    scanned: &[filesystem::ScannedContentFile],
    known: &HashMap<String, String>,
) -> crate::Result<HashMap<String, String>> {
    let started = std::time::Instant::now();
    let mut hashes: HashMap<String, String> =
        HashMap::with_capacity(scanned.len());
    let mut pending: Vec<(String, PathBuf)> = Vec::new();
    let mut pending_bytes = 0u64;

    for file in scanned {
        let key = hash_key_for(file);
        if hashes.contains_key(key) {
            continue;
        }
        if let Some(sha1) = known.get(key) {
            hashes.insert(key.to_string(), sha1.clone());
            continue;
        }
        pending_bytes += file.size;
        pending.push((
            key.to_string(),
            instance_dir.join(&file.relative_path),
        ));
    }

    let reused = hashes.len();
    if pending.is_empty() {
        tracing::info!(
            "content_timing: [3c] sha1 hashing {} ms (0 hashed, {} reused of {} files) for '{}'",
            started.elapsed().as_millis(),
            reused,
            scanned.len(),
            instance_id
        );
        return Ok(hashes);
    }

    let tasks = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2)
        .clamp(1, MAX_HASH_TASKS)
        .min(pending.len());
    // Round-robin rather than contiguous chunks: the scan is ordered by folder,
    // so contiguous chunks would put all the big jars in one task.
    let mut buckets: Vec<Vec<(String, PathBuf)>> = vec![Vec::new(); tasks];
    for (index, entry) in pending.into_iter().enumerate() {
        buckets[index % tasks].push(entry);
    }

    let handles = buckets
        .into_iter()
        .map(|bucket| {
            spawn_blocking(move || {
                let mut buffer = vec![0u8; HASH_READ_BUFFER];
                let mut out = Vec::with_capacity(bucket.len());
                for (key, path) in bucket {
                    match hash_file(&path, &mut buffer) {
                        Ok(sha1) => out.push((key, sha1)),
                        Err(error) => tracing::warn!(
                            "sync: failed to hash file: {} ({})",
                            path.display(),
                            error
                        ),
                    }
                }
                out
            })
        })
        .collect::<Vec<_>>();

    let mut hashed = 0usize;
    for handle in handles {
        for (key, sha1) in handle.await? {
            hashes.insert(key, sha1);
            hashed += 1;
        }
    }

    tracing::info!(
        "content_timing: [3c] sha1 hashing {} ms ({} hashed, {} reused of {} files, {:.1} MiB read, {} tasks) for '{}'",
        started.elapsed().as_millis(),
        hashed,
        reused,
        scanned.len(),
        pending_bytes as f64 / (1024.0 * 1024.0),
        tasks,
        instance_id
    );

    Ok(hashes)
}

/// Streaming SHA1 of one file, reusing the caller's buffer.
fn hash_file(path: &Path, buffer: &mut [u8]) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    loop {
        let read = file.read(buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.digest().to_string())
}

fn write_content_cache(
    path: &Path,
    cache: &ContentCacheFile,
) -> crate::Result<()> {
    let json = serde_json::to_string_pretty(cache).map_err(|e| {
        crate::ErrorKind::OtherError(format!(
            "Failed to serialize content cache: {e}"
        ))
    })?;
    std::fs::write(path, json).map_err(|e| {
        crate::ErrorKind::OtherError(format!(
            "Failed to write content cache at '{}': {e}",
            path.display()
        ))
    })?;
    Ok(())
}

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

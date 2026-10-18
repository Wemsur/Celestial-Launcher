//! A persistent response cache for content sources.
//!
//! A source is a plugin running in the webview with no storage of its own that
//! is cheap enough to use as a cache: `api.storage` is a single JSON file
//! rewritten in full on every write, and the launcher's network proxy is a pure
//! passthrough. So a source that fetches a listing pays a round trip every time,
//! and the same listing is fetched again on the next tab, the next launch, and
//! the next instance. Modrinth does not have this problem — its results sit in
//! the SQLite cache — and the asymmetry is what makes plugin sources feel slow.
//!
//! This module gives every source that same treatment without asking it to do
//! anything: the proxy records a successful `GET` response and serves it from
//! disk while it is fresh, refreshing it in the background once it is stale.
//!
//! Deliberately narrow, because the cache is keyed by URL and a source cannot
//! be trusted to have meant what it wrote:
//!
//! - Only `GET` is cached. A `POST` is a query whose answer can depend on the
//!   body, and caching it would serve one query's answer to another.
//! - A response is only stored when it says so itself: `no-store` and `private`
//!   are honoured, and a `Set-Cookie` in the response disqualifies it (the
//!   answer is personalised, so it belongs to the user who asked, not to the
//!   cache). `Vary` is not interpreted — a source that varies on a header it
//!   sends should send `Cache-Control: no-store` rather than rely on us
//!   guessing which header matters.
//! - The lifetime is `max-age` when the response gives one, clamped to a range
//!   that keeps a misconfigured site from pinning a listing for a year.
//! - Entries are per source, so one source can never read another's answers.
//!
//! Staleness is served, not blocked on: a stale entry is returned immediately
//! and refreshed in the background, the same contract as `StaleWhileRevalidate`
//! in the launcher's own cache. That is what makes a revisit instant even when
//! the site is slow — the point of the cache is to hide latency, not to be
//! perfectly fresh.

use super::network::SourceFetchResponse;
use crate::State;
use crate::util::io;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;

/// How long a response is kept when it does not say.
const DEFAULT_TTL_SECONDS: u64 = 10 * 60;

/// Bounds on a source-declared `max-age`. A site that asks for a year is not
/// asking for a year of this launcher's list; a site that asks for a second is
/// better served by no cache than by one that churns.
const MIN_TTL_SECONDS: u64 = 5;
const MAX_TTL_SECONDS: u64 = 24 * 60 * 60;

/// Cap on one stored response, matching the proxy's own response cap. A source
/// that fetched something bigger is not cached rather than half-stored.
const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;

/// Cap on the whole cache, so a source paging through a catalogue cannot fill
/// the disk. Oldest entries are dropped first.
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;

/// One cached response, as stored on disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CacheEntry {
    /// The request URL, including its query.
    url: String,
    status: u16,
    headers: HashMap<String, String>,
    body: String,
    /// Unix seconds after which the entry is stale (still served, refreshed in
    /// the background).
    expires: i64,
    /// Unix seconds when it was stored, for eviction order.
    stored_at: i64,
    #[serde(default)]
    final_url: Option<String>,
}

/// Per-source locks, so two concurrent requests for the same URL do not both
/// write the entry. The write is atomic on its own; the lock only keeps a
/// background refresh from racing a foreground one for the same key.
static LOCKS: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

async fn lock_for(key: &str) -> tokio::sync::OwnedMutexGuard<()> {
    let lock = {
        let mut locks = LOCKS.lock().await;
        locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    lock.lock_owned().await
}

fn cache_dir(state: &State, source_id: &str) -> crate::Result<PathBuf> {
    Ok(super::store::source_data_dir(state, source_id)?.join("http-cache"))
}

/// A file name for a URL. Hashed rather than encoded: a URL can be far longer
/// than a file name allows, and contains characters that are not safe on every
/// platform.
fn entry_path(dir: &Path, url: &str) -> PathBuf {
    let digest = sha1_smol::Sha1::from(url.as_bytes()).hexdigest();
    dir.join(format!("{digest}.json"))
}

/// The URL a request is cached under, or `None` when it must not be cached.
///
/// The method and body are part of the decision: only a `GET` has a URL that
/// identifies its answer. A request that carries credentials is excluded too —
/// its answer belongs to whoever sent the token, and this cache is shared by
/// every request the source makes, including ones made before the user signed
/// in.
pub fn cache_key(
    method: &str,
    url: &str,
    body: Option<&str>,
    headers: &HashMap<String, String>,
) -> Option<String> {
    if !method.eq_ignore_ascii_case("GET") || body.is_some_and(|body| !body.is_empty()) {
        return None;
    }
    let carries_credentials = headers.keys().any(|name| {
        name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case("cookie")
            || name.eq_ignore_ascii_case("proxy-authorization")
    });
    if carries_credentials {
        return None;
    }
    Some(url.to_string())
}

/// Whether a response may be stored, and for how long.
///
/// Returns `None` when the response forbids storage, otherwise the lifetime in
/// seconds. Public so the proxy can decide before it even reads the body.
pub fn cacheable_for(headers: &HashMap<String, String>) -> Option<u64> {
    let lower = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };

    // A personalised answer belongs to the user who asked, not to the cache.
    if lower("set-cookie").is_some() {
        return None;
    }

    let directives = lower("cache-control").unwrap_or("");
    for directive in directives.split(',') {
        let directive = directive.trim().to_ascii_lowercase();
        if directive == "no-store" || directive == "private" {
            return None;
        }
    }

    let max_age = directives.split(',').find_map(|directive| {
        directive
            .trim()
            .strip_prefix("max-age=")
            .and_then(|value| value.trim().parse::<u64>().ok())
    });

    Some(max_age.unwrap_or(DEFAULT_TTL_SECONDS).clamp(MIN_TTL_SECONDS, MAX_TTL_SECONDS))
}

fn now_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A stored response, and whether it is still fresh.
pub struct CachedResponse {
    pub response: SourceFetchResponse,
    pub fresh: bool,
}

async fn read_entry(dir: &Path, url: &str) -> Option<CacheEntry> {
    let bytes = io::read(entry_path(dir, url)).await.ok()?;
    let entry: CacheEntry = serde_json::from_slice(&bytes).ok()?;
    // The digest could in principle collide across URLs; the stored URL is the
    // truth, so a mismatch is treated as a miss rather than served.
    (entry.url == url).then_some(entry)
}

/// Look up a cached response.
///
/// `Ok(Some(...))` means the caller can answer without the network; `fresh`
/// says whether it should still be refreshed in the background.
pub async fn get(
    source_id: &str,
    url: &str,
    state: &State,
) -> crate::Result<Option<CachedResponse>> {
    let dir = cache_dir(state, source_id)?;
    let Some(entry) = read_entry(&dir, url).await else {
        return Ok(None);
    };
    Ok(Some(CachedResponse {
        fresh: entry.expires > now_seconds(),
        response: SourceFetchResponse {
            status: entry.status,
            ok: (200..300).contains(&entry.status),
            headers: entry.headers,
            body: entry.body,
            final_url: entry.final_url,
        },
    }))
}

/// Store a response, replacing any earlier one for the same URL.
pub async fn put(
    source_id: &str,
    url: &str,
    ttl_seconds: u64,
    response: &SourceFetchResponse,
    state: &State,
) -> crate::Result<()> {
    if response.body.len() > MAX_ENTRY_BYTES {
        return Ok(());
    }
    let dir = cache_dir(state, source_id)?;
    io::create_dir_all(&dir).await?;

    let entry = CacheEntry {
        url: url.to_string(),
        status: response.status,
        headers: response.headers.clone(),
        body: response.body.clone(),
        expires: now_seconds() + ttl_seconds as i64,
        stored_at: now_seconds(),
        final_url: response.final_url.clone(),
    };
    let encoded = serde_json::to_vec(&entry)?;

    let _guard = lock_for(url).await;
    io::write(entry_path(&dir, url), encoded).await?;

    prune(&dir).await;
    Ok(())
}

/// Drop the oldest entries until the cache fits its budget.
///
/// Best-effort: a failure here costs disk space, not correctness, and must not
/// fail the request that triggered it.
async fn prune(dir: &Path) {
    let Ok(mut entries) = io::read_dir(dir).await else {
        return;
    };

    let mut files: Vec<(PathBuf, u64, i64)> = Vec::new();
    let mut total: u64 = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let Ok(metadata) = entry.metadata().await else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        total += metadata.len();
        files.push((path, metadata.len(), modified));
    }

    if total <= MAX_CACHE_BYTES {
        return;
    }

    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, size, _) in files {
        if total <= MAX_CACHE_BYTES {
            break;
        }
        if io::remove_file(&path).await.is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

/// Forget everything cached for a source, for when it is removed.
pub async fn clear(source_id: &str, state: &State) -> crate::Result<()> {
    let dir = cache_dir(state, source_id)?;
    if dir.is_dir() {
        io::remove_dir_all(&dir).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn only_get_without_a_body_is_cacheable() {
        let none = HashMap::new();
        assert_eq!(
            cache_key("GET", "https://example.com/a", None, &none).as_deref(),
            Some("https://example.com/a")
        );
        assert!(cache_key("POST", "https://example.com/a", None, &none).is_none());
        assert!(cache_key("GET", "https://example.com/a", Some("{}"), &none).is_none());
    }

    #[test]
    fn a_request_with_credentials_is_not_cached() {
        for name in ["Authorization", "cookie", "Proxy-Authorization"] {
            let headers = headers(&[(name, "secret")]);
            assert!(
                cache_key("GET", "https://example.com/a", None, &headers).is_none(),
                "{name} must disqualify the request"
            );
        }
        let plain = headers(&[("accept", "application/json")]);
        assert!(cache_key("GET", "https://example.com/a", None, &plain).is_some());
    }

    #[test]
    fn no_store_and_private_are_honoured() {
        assert!(cacheable_for(&headers(&[("cache-control", "no-store")])).is_none());
        assert!(cacheable_for(&headers(&[("cache-control", "private, max-age=60")])).is_none());
        assert!(cacheable_for(&headers(&[("Cache-Control", "no-cache")])).is_some());
    }

    #[test]
    fn a_set_cookie_disqualifies_the_response() {
        assert!(
            cacheable_for(&headers(&[("set-cookie", "session=abc")])).is_none(),
            "a personalised answer must not be shared through the cache"
        );
    }

    #[test]
    fn max_age_is_clamped_and_defaults_when_absent() {
        assert_eq!(
            cacheable_for(&headers(&[("cache-control", "max-age=600")])),
            Some(600)
        );
        assert_eq!(
            cacheable_for(&headers(&[("cache-control", "max-age=1")])),
            Some(MIN_TTL_SECONDS)
        );
        assert_eq!(
            cacheable_for(&headers(&[("cache-control", "max-age=99999999")])),
            Some(MAX_TTL_SECONDS)
        );
        assert_eq!(cacheable_for(&HashMap::new()), Some(DEFAULT_TTL_SECONDS));
    }

    #[test]
    fn entry_paths_are_stable_and_distinct() {
        let dir = Path::new("/tmp/cache");
        assert_eq!(entry_path(dir, "https://a/1"), entry_path(dir, "https://a/1"));
        assert_ne!(entry_path(dir, "https://a/1"), entry_path(dir, "https://a/2"));
    }
}

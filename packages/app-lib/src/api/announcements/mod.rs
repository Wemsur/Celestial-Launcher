//! Built-in, read-only public announcements and per-ID popup preferences.

mod network;

use crate::state::celestial_db::announcements as storage;
use crate::State;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use tokio::sync::Semaphore;

pub const MAX_INDEX_BYTES: usize = 1024 * 1024;
pub const MAX_BODY_BYTES: usize = 128 * 1024;
const MAX_ANNOUNCEMENTS: usize = 2000;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementState {
	Published,
	Ended,
	Withdrawn,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementLevel {
	Info,
	Warning,
	Critical,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Announcement {
	pub id: String,
	#[serde(default)]
	pub issue_number: Option<u64>,
	#[serde(default)]
	pub title: Option<String>,
	#[serde(default)]
	pub level: Option<AnnouncementLevel>,
	pub published_at: DateTime<Utc>,
	pub updated_at: DateTime<Utc>,
	#[serde(default)]
	pub starts_at: Option<DateTime<Utc>>,
	#[serde(default)]
	pub expires_at: Option<DateTime<Utc>>,
	pub state: AnnouncementState,
	#[serde(default)]
	pub body_path: Option<String>,
	#[serde(default)]
	pub body_sha256: Option<String>,
}

impl Announcement {
	pub fn is_active(&self, now: DateTime<Utc>) -> bool {
		self.state == AnnouncementState::Published
			&& self.starts_at.is_some_and(|start| start <= now)
			&& self.expires_at.is_some_and(|end| now < end)
	}

	pub fn is_visible(&self, now: DateTime<Utc>) -> bool {
		self.state == AnnouncementState::Withdrawn
			|| self.starts_at.is_some_and(|start| start <= now)
	}
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Feed {
	pub schema_version: u32,
	pub generation: u64,
	pub generated_at: DateTime<Utc>,
	pub announcements: Vec<Announcement>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnnouncementMetadata {
	#[serde(flatten)]
	pub announcement: Announcement,
	pub suppressed: bool,
	pub body_cached: bool,
}

#[derive(Serialize)]
pub struct Snapshot {
	pub generation: Option<u64>,
	pub last_success_at: Option<DateTime<Utc>>,
	pub refreshing: bool,
	pub fresh: bool,
	pub error: Option<String>,
	pub active: Vec<AnnouncementMetadata>,
	pub visible_count: usize,
	pub candidate_id: Option<String>,
}

#[derive(Serialize)]
pub struct AnnouncementPage {
	pub items: Vec<AnnouncementMetadata>,
	pub next_cursor: Option<usize>,
}

#[derive(Serialize)]
pub struct AnnouncementBody {
	pub announcement: AnnouncementMetadata,
	pub markdown: Option<String>,
	pub cached: bool,
	pub stale: bool,
	pub error: Option<String>,
}

#[derive(Default)]
struct RefreshStatus {
	refreshing: bool,
	fresh: bool,
	error: Option<String>,
}

static STATUS: LazyLock<Mutex<RefreshStatus>> = LazyLock::new(|| Mutex::new(RefreshStatus::default()));
static BODY_REQUESTS: Semaphore = Semaphore::const_new(3);
static BODY_LOCKS: LazyLock<dashmap::DashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>> =
	LazyLock::new(dashmap::DashMap::new);

pub fn begin_refresh() -> bool {
	let mut status = STATUS.lock().unwrap_or_else(|error| error.into_inner());
	if status.refreshing {
		return false;
	}
	status.refreshing = true;
	status.fresh = false;
	status.error = None;
	true
}

pub async fn refresh(state: &State) -> crate::Result<()> {
	let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
		let bytes = network::fetch("index.json", MAX_INDEX_BYTES).await?;
		let feed: Feed = serde_json::from_slice(&bytes)?;
		validate_feed(&feed)?;
		storage::accept_feed(&state.celestial, &feed, &digest(&bytes)).await?;
		let entries = storage::load_metadata(&state.celestial).await?;
		let now = Utc::now();
		if let Some(candidate) = entries.iter().find(|entry| !entry.suppressed && entry.announcement.is_active(now)) {
			let body = get(state, &candidate.announcement.id).await?;
			if body.stale || body.markdown.is_none() {
				return Err(input(body.error.unwrap_or_else(|| "Announcement body is unavailable".into())));
			}
		}
		Ok::<(), crate::Error>(())
	}).await.unwrap_or_else(|_| Err(input("Announcement refresh timed out")));
	let mut status = STATUS.lock().unwrap_or_else(|error| error.into_inner());
	status.refreshing = false;
	status.fresh = result.is_ok();
	status.error = result.as_ref().err().map(ToString::to_string);
	if let Err(error) = &result {
		tracing::warn!("Announcement refresh failed: {error}");
	}
	result
}

pub async fn snapshot(state: &State) -> crate::Result<Snapshot> {
	let (entries, sync) = storage::load_snapshot(&state.celestial).await?;
	let now = Utc::now();
	let visible_count = entries.iter().filter(|entry| entry.announcement.is_visible(now)).count();
	let active: Vec<_> = entries.into_iter().filter(|entry| entry.announcement.is_active(now)).collect();
	let candidate_id = active.iter().find(|entry| !entry.suppressed).map(|entry| entry.announcement.id.clone());
	let status = STATUS.lock().unwrap_or_else(|error| error.into_inner());
	Ok(Snapshot {
		generation: sync.as_ref().map(|(generation, _)| *generation),
		last_success_at: sync.map(|(_, at)| at),
		refreshing: status.refreshing,
		fresh: status.fresh,
		error: status.error.clone(),
		active,
		visible_count,
		candidate_id,
	})
}

pub async fn list(state: &State, cursor: Option<usize>, limit: usize) -> crate::Result<AnnouncementPage> {
	let now = Utc::now();
	let entries: Vec<_> = storage::load_metadata(&state.celestial).await?.into_iter()
		.filter(|entry| entry.announcement.is_visible(now)).collect();
	let offset = cursor.unwrap_or(0).min(entries.len());
	let end = offset.saturating_add(limit.clamp(1, 50)).min(entries.len());
	Ok(AnnouncementPage {
		items: entries[offset..end].to_vec(),
		next_cursor: (end < entries.len()).then_some(end),
	})
}

pub async fn get(state: &State, id: &str) -> crate::Result<AnnouncementBody> {
	validate_id(id)?;
	let lock = BODY_LOCKS.entry(id.to_string()).or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(()))).clone();
	let _lock = lock.lock().await;
	let metadata = storage::metadata(&state.celestial, id).await?.ok_or_else(|| input("Unknown announcement"))?;
	if metadata.announcement.state == AnnouncementState::Withdrawn || !metadata.announcement.is_visible(Utc::now()) {
		return Ok(AnnouncementBody { announcement: metadata, markdown: None, cached: false, stale: false, error: None });
	}
	let hash = metadata.announcement.body_sha256.as_deref().ok_or_else(|| input("Announcement has no body digest"))?;
	if let Some((_, markdown)) = storage::cached_body(&state.celestial, id, Some(hash)).await? {
		if digest(markdown.as_bytes()) == hash {
			storage::cache_body(&state.celestial, &metadata.announcement, &markdown).await?;
			return finish_body(state, metadata, markdown, true, None).await;
		}
	}
	let downloaded = tokio::time::timeout(std::time::Duration::from_secs(10), async {
		let _permit = BODY_REQUESTS.acquire().await.map_err(|_| input("Announcement downloads are unavailable"))?;
		let path = metadata.announcement.body_path.as_deref().ok_or_else(|| input("Announcement has no body path"))?;
		let bytes = network::fetch(path, MAX_BODY_BYTES).await?;
		if digest(&bytes) != hash {
			return Err(input("Announcement body digest does not match the index"));
		}
		let markdown = String::from_utf8(bytes).map_err(|_| input("Announcement body is not UTF-8"))?;
		if markdown.trim().is_empty() {
			return Err(input("Announcement body is empty"));
		}
		Ok::<_, crate::Error>(markdown)
	}).await.unwrap_or_else(|_| Err(input("Announcement body download timed out")));
	match downloaded {
		Ok(markdown) => {
			storage::cache_body(&state.celestial, &metadata.announcement, &markdown).await?;
			finish_body(state, metadata, markdown, false, None).await
		}
		Err(error) => {
			if let Some((old, markdown)) = storage::cached_body(&state.celestial, id, None).await? {
				if old.body_sha256.as_deref() == Some(digest(markdown.as_bytes()).as_str()) {
					let old = AnnouncementMetadata { announcement: old, suppressed: metadata.suppressed, body_cached: true };
					return finish_body(state, old, markdown, true, Some(error.to_string())).await;
				}
			}
			Ok(AnnouncementBody { announcement: metadata, markdown: None, cached: false, stale: true, error: Some(error.to_string()) })
		}
	}
}

async fn finish_body(state: &State, metadata: AnnouncementMetadata, markdown: String, cached: bool, error: Option<String>) -> crate::Result<AnnouncementBody> {
	let latest = storage::metadata(&state.celestial, &metadata.announcement.id).await?.ok_or_else(|| input("Unknown announcement"))?;
	if latest.announcement.state == AnnouncementState::Withdrawn || !latest.announcement.is_visible(Utc::now()) {
		return Ok(AnnouncementBody { announcement: latest, markdown: None, cached: false, stale: false, error: None });
	}
	let stale = error.is_some() || latest.announcement.body_sha256 != metadata.announcement.body_sha256;
	let announcement = if stale { AnnouncementMetadata { suppressed: latest.suppressed, ..metadata } } else { latest };
	Ok(AnnouncementBody { announcement, markdown: Some(markdown), cached, stale, error })
}

pub async fn suppress(state: &State, id: &str) -> crate::Result<()> {
	validate_id(id)?;
	storage::suppress(&state.celestial, id).await
}

pub(crate) fn digest(bytes: &[u8]) -> String {
	format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn input(message: impl Into<String>) -> crate::Error {
	crate::ErrorKind::InputError(message.into()).into()
}

pub(crate) fn validate_id(id: &str) -> crate::Result<()> {
	let number = id.strip_prefix("github-").ok_or_else(|| input("Invalid announcement ID"))?;
	if number.is_empty() || number.len() > 20 || number.starts_with('0') || !number.bytes().all(|byte| byte.is_ascii_digit()) {
		return Err(input("Invalid announcement ID"));
	}
	Ok(())
}

fn validate_feed(feed: &Feed) -> crate::Result<()> {
	if feed.schema_version != 1 || feed.generation == 0 || feed.generation > MAX_SAFE_INTEGER || feed.announcements.len() > MAX_ANNOUNCEMENTS {
		return Err(input("Unsupported announcement index"));
	}
	let mut ids = HashSet::new();
	for item in &feed.announcements {
		validate_id(&item.id)?;
		if !ids.insert(&item.id) {
			return Err(input("Duplicate announcement ID"));
		}
		if item.state == AnnouncementState::Withdrawn {
			if item.title.is_some() || item.body_path.is_some() || item.body_sha256.is_some() || item.issue_number.is_some() || item.starts_at.is_some() || item.expires_at.is_some() || item.level.is_some() {
				return Err(input("Withdrawn announcements must not retain content"));
			}
			continue;
		}
		let valid_title = item.title.as_ref().is_some_and(|title| !title.trim().is_empty() && title.chars().count() <= 500);
		let valid_time = matches!((item.starts_at, item.expires_at), (Some(start), Some(end)) if start < end);
		let valid_number = item.issue_number.is_some_and(|number| number > 0 && number <= MAX_SAFE_INTEGER);
		let hash = item.body_sha256.as_deref().unwrap_or("");
		let valid_hash = hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
		let expected = format!("announcements/{}/{hash}.md", item.id);
		if !valid_title || !valid_time || !valid_number || !valid_hash || item.level.is_none() || item.body_path.as_deref() != Some(expected.as_str()) {
			return Err(input("Invalid announcement metadata"));
		}
	}
	let _ = feed.generated_at;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn index_rejects_duplicates_and_paths() {
		let mut feed: Feed = serde_json::from_value(serde_json::json!({
			"schema_version": 1, "generation": 1, "generated_at": "2026-01-01T00:00:00Z",
			"announcements": [{"id":"github-1","issue_number":1,"title":"Test","level":"info",
				"published_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z",
				"starts_at":"2026-01-01T00:00:00Z","expires_at":"2026-01-02T00:00:00Z",
				"state":"published","body_sha256":"a".repeat(64),"body_path":format!("announcements/github-1/{}.md", "a".repeat(64))}]
		})).unwrap();
		assert!(validate_feed(&feed).is_ok());
		feed.announcements.push(feed.announcements[0].clone());
		assert!(validate_feed(&feed).is_err());
		let _ = feed.announcements.pop();
		feed.announcements[0].body_path = Some("../instance.json".into());
		assert!(validate_feed(&feed).is_err());
	}

	#[test]
	fn effective_period_has_exclusive_end() {
		let start: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
		let item = Announcement { id: "github-1".into(), issue_number: None, title: None, level: None,
			published_at: start, updated_at: start, starts_at: Some(start), expires_at: Some(start + chrono::Duration::hours(1)),
			state: AnnouncementState::Published, body_path: None, body_sha256: None };
		assert!(item.is_active(start));
		assert!(!item.is_active(start + chrono::Duration::hours(1)));
	}
}

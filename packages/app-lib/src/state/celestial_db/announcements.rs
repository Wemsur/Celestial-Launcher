//! Announcement snapshots and body caches are independent of popup preferences.

use super::CelestialDb;
use crate::api::announcements::{input, Announcement, AnnouncementMetadata, AnnouncementState, Feed};
use chrono::{DateTime, Utc};
use sqlx::SqliteConnection;
use std::collections::HashSet;

const CACHE_LIMIT: i64 = 32 * 1024 * 1024;

type MetadataRow = (String, i64, i64);
pub(crate) type SyncState = Option<(u64, DateTime<Utc>)>;

fn metadata_row((json, suppressed, cached): MetadataRow) -> crate::Result<AnnouncementMetadata> {
	Ok(AnnouncementMetadata {
		announcement: serde_json::from_str(&json)?,
		suppressed: suppressed != 0,
		body_cached: cached != 0,
	})
}

async fn read_metadata(connection: &mut SqliteConnection) -> crate::Result<Vec<AnnouncementMetadata>> {
	let rows: Vec<MetadataRow> = sqlx::query_as(
		"SELECT a.metadata_json, p.id IS NOT NULL,
		 EXISTS(SELECT 1 FROM announcement_body_cache b
		 WHERE b.id = a.id AND b.body_sha256 = json_extract(a.metadata_json, '$.body_sha256'))
		 FROM announcements a LEFT JOIN announcement_preferences p ON p.id = a.id
		 ORDER BY a.published_at DESC, a.id DESC",
	).fetch_all(connection).await?;
	rows.into_iter().map(metadata_row).collect()
}

async fn read_sync(connection: &mut SqliteConnection) -> crate::Result<SyncState> {
	let row: Option<(i64, i64)> = sqlx::query_as("SELECT generation, last_success_at FROM announcement_sync WHERE id = 1")
		.fetch_optional(connection).await?;
	row.map(|(generation, timestamp)| {
		let generation = u64::try_from(generation).map_err(|_| input("Invalid cached announcement generation"))?;
		let timestamp = DateTime::from_timestamp(timestamp, 0).ok_or_else(|| input("Invalid announcement sync timestamp"))?;
		Ok((generation, timestamp))
	}).transpose()
}

pub(crate) async fn load_snapshot(db: &CelestialDb) -> crate::Result<(Vec<AnnouncementMetadata>, SyncState)> {
	let mut tx = db.pool().begin().await?;
	let metadata = read_metadata(&mut tx).await?;
	let sync = read_sync(&mut tx).await?;
	tx.commit().await?;
	Ok((metadata, sync))
}

pub(crate) async fn load_metadata(db: &CelestialDb) -> crate::Result<Vec<AnnouncementMetadata>> {
	Ok(load_snapshot(db).await?.0)
}

pub(crate) async fn metadata(db: &CelestialDb, id: &str) -> crate::Result<Option<AnnouncementMetadata>> {
	let row: Option<MetadataRow> = sqlx::query_as(
		"SELECT a.metadata_json, p.id IS NOT NULL,
		 EXISTS(SELECT 1 FROM announcement_body_cache b
		 WHERE b.id = a.id AND b.body_sha256 = json_extract(a.metadata_json, '$.body_sha256'))
		 FROM announcements a LEFT JOIN announcement_preferences p ON p.id = a.id WHERE a.id = ?",
	).bind(id).fetch_optional(db.pool()).await?;
	row.map(metadata_row).transpose()
}

pub(crate) async fn accept_feed(db: &CelestialDb, feed: &Feed, digest: &str) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	let previous: Option<(i64, String)> = sqlx::query_as("SELECT generation, feed_sha256 FROM announcement_sync WHERE id = 1")
		.fetch_optional(&mut *tx).await?;
	if let Some((generation, previous_digest)) = previous {
		if feed.generation < generation as u64 || (feed.generation == generation as u64 && digest != previous_digest) {
			return Err(input("Announcement index is older than the accepted snapshot or has a conflicting generation"));
		}
	}
	let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, metadata_json FROM announcements")
		.fetch_all(&mut *tx).await?;
	let incoming: HashSet<_> = feed.announcements.iter().map(|item| item.id.as_str()).collect();
	for (id, json) in rows {
		let mut old: Announcement = serde_json::from_str(&json)?;
		if old.state == AnnouncementState::Withdrawn && feed.announcements.iter().any(|item| item.id == id && item.state != AnnouncementState::Withdrawn) {
			return Err(input("A withdrawn announcement ID cannot be republished"));
		}
		if !incoming.contains(id.as_str()) && old.state == AnnouncementState::Published {
			old.state = AnnouncementState::Ended;
			write_metadata(&mut tx, &old).await?;
		}
	}
	for item in &feed.announcements {
		write_metadata(&mut tx, item).await?;
		if item.state == AnnouncementState::Withdrawn {
			sqlx::query("DELETE FROM announcement_body_cache WHERE id = ?")
				.bind(&item.id).execute(&mut *tx).await?;
		}
	}
	sqlx::query("INSERT INTO announcement_sync (id, generation, feed_sha256, last_success_at)
	 VALUES (1, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET generation = excluded.generation,
	 feed_sha256 = excluded.feed_sha256, last_success_at = excluded.last_success_at")
		.bind(feed.generation as i64).bind(digest).bind(Utc::now().timestamp()).execute(&mut *tx).await?;
	tx.commit().await?;
	Ok(())
}

async fn write_metadata(connection: &mut SqliteConnection, item: &Announcement) -> crate::Result<()> {
	let state = match item.state {
		AnnouncementState::Published => "published",
		AnnouncementState::Ended => "ended",
		AnnouncementState::Withdrawn => "withdrawn",
	};
	sqlx::query("INSERT INTO announcements (id, metadata_json, published_at, state) VALUES (?, ?, ?, ?)
	 ON CONFLICT(id) DO UPDATE SET metadata_json = excluded.metadata_json,
	 published_at = excluded.published_at, state = excluded.state")
		.bind(&item.id).bind(serde_json::to_string(item)?).bind(item.published_at.timestamp()).bind(state)
		.execute(connection).await?;
	Ok(())
}

pub(crate) async fn cached_body(db: &CelestialDb, id: &str, hash: Option<&str>) -> crate::Result<Option<(Announcement, String)>> {
	let row: Option<(String, String)> = sqlx::query_as(
		"SELECT metadata_json, markdown FROM announcement_body_cache
		 WHERE id = ? AND (? IS NULL OR body_sha256 = ?) ORDER BY accessed_at DESC LIMIT 1",
	).bind(id).bind(hash).bind(hash).fetch_optional(db.pool()).await?;
	row.map(|(metadata, markdown)| Ok((serde_json::from_str(&metadata)?, markdown))).transpose()
}

pub(crate) async fn cache_body(db: &CelestialDb, item: &Announcement, markdown: &str) -> crate::Result<bool> {
	let mut tx = db.pool().begin().await?;
	let state: Option<String> = sqlx::query_scalar("SELECT state FROM announcements WHERE id = ?")
		.bind(&item.id).fetch_optional(&mut *tx).await?;
	if state.as_deref() == Some("withdrawn") {
		return Ok(false);
	}
	if state.is_none() {
		return Err(input("Cannot cache an unknown announcement"));
	}
	let hash = item.body_sha256.as_deref().ok_or_else(|| input("Announcement has no digest"))?;
	sqlx::query("INSERT INTO announcement_body_cache (id, body_sha256, metadata_json, markdown, byte_length, accessed_at)
	 VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(id, body_sha256) DO UPDATE SET
	 metadata_json = excluded.metadata_json, markdown = excluded.markdown,
	 byte_length = excluded.byte_length, accessed_at = excluded.accessed_at")
		.bind(&item.id).bind(hash).bind(serde_json::to_string(item)?).bind(markdown)
		.bind(markdown.len() as i64).bind(Utc::now().timestamp_millis()).execute(&mut *tx).await?;
	let mut total: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(byte_length), 0) FROM announcement_body_cache")
		.fetch_one(&mut *tx).await?;
	if total > CACHE_LIMIT {
		let rows: Vec<(String, String, i64)> = sqlx::query_as("SELECT id, body_sha256, byte_length FROM announcement_body_cache ORDER BY accessed_at, id, body_sha256")
			.fetch_all(&mut *tx).await?;
		for (id, hash, bytes) in rows {
			if total <= CACHE_LIMIT { break; }
			sqlx::query("DELETE FROM announcement_body_cache WHERE id = ? AND body_sha256 = ?")
				.bind(id).bind(hash).execute(&mut *tx).await?;
			total -= bytes;
		}
	}
	tx.commit().await?;
	Ok(true)
}

pub(crate) async fn suppress(db: &CelestialDb, id: &str) -> crate::Result<()> {
	let mut tx = db.pool().begin().await?;
	let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM announcements WHERE id = ?)")
		.bind(id).fetch_one(&mut *tx).await?;
	if !exists { return Err(input("Unknown announcement")); }
	sqlx::query("INSERT INTO announcement_preferences (id, suppressed_at) VALUES (?, ?)
	 ON CONFLICT(id) DO NOTHING").bind(id).bind(Utc::now().timestamp()).execute(&mut *tx).await?;
	tx.commit().await?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::api::announcements::{digest, AnnouncementLevel};

	fn feed(generation: u64) -> Feed {
		let date: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
		let hash = digest(b"test");
		Feed { schema_version: 1, generation, generated_at: date, announcements: vec![Announcement {
			id: "github-1".into(), issue_number: Some(1), title: Some("Test".into()), level: Some(AnnouncementLevel::Info),
			published_at: date, updated_at: date, starts_at: Some(date), expires_at: Some(date + chrono::Duration::days(1)),
			state: AnnouncementState::Published, body_path: Some(format!("announcements/github-1/{hash}.md")), body_sha256: Some(hash),
		}] }
	}

	#[tokio::test]
	async fn edits_and_withdrawals_preserve_preferences() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		let original = feed(1);
		accept_feed(&db, &original, "first").await.unwrap();
		cache_body(&db, &original.announcements[0], "test").await.unwrap();
		suppress(&db, "github-1").await.unwrap();
		let mut edited = feed(2);
		edited.announcements[0].title = Some("Edited".into());
		accept_feed(&db, &edited, "second").await.unwrap();
		assert!(metadata(&db, "github-1").await.unwrap().unwrap().suppressed);
		let item = &mut edited.announcements[0];
		item.state = AnnouncementState::Withdrawn;
		item.title = None; item.level = None; item.issue_number = None;
		item.starts_at = None; item.expires_at = None; item.body_path = None; item.body_sha256 = None;
		edited.generation = 3;
		accept_feed(&db, &edited, "third").await.unwrap();
		assert!(cached_body(&db, "github-1", None).await.unwrap().is_none());
		assert!(metadata(&db, "github-1").await.unwrap().unwrap().suppressed);
		assert!(accept_feed(&db, &original, "first").await.is_err());
	}

	#[tokio::test]
	async fn failed_transaction_does_not_replace_metadata() {
		let dir = tempfile::tempdir().unwrap();
		let db = CelestialDb::connect(dir.path()).await.unwrap();
		accept_feed(&db, &feed(1), "first").await.unwrap();
		sqlx::query("CREATE TRIGGER reject_sync BEFORE UPDATE ON announcement_sync BEGIN SELECT RAISE(ABORT, 'injected'); END")
			.execute(db.pool()).await.unwrap();
		let mut next = feed(2);
		next.announcements[0].title = Some("Changed".into());
		assert!(accept_feed(&db, &next, "second").await.is_err());
		assert_eq!(metadata(&db, "github-1").await.unwrap().unwrap().announcement.title.as_deref(), Some("Test"));
	}
}

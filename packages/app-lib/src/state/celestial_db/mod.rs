//! The Celestial fork's own database (`celestial.db`), stored next to
//! `app.db` in the settings directory.
//!
//! Only fork-owned persistent state lives here, deliberately separated from
//! `app.db` so upstream schema churn (migrations, `.sqlx` cache) never touches
//! it. Queries use runtime SQL (`sqlx::query`) rather than the `query!` macro:
//! there is no offline cache to regenerate and no build-time database
//! dependency.
//!
//! What lives where is documented in `migrations-celestial/0001_init.sql`.

use crate::state::State;
use sqlx::sqlite::{
	SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions,
	SqliteSynchronous,
};
use sqlx::{Pool, Sqlite, SqlitePool};
use std::path::Path;

pub mod announcements;
pub mod content_sources;
pub mod groups;
pub mod libraries;
pub mod translation_cache;
pub mod ui_settings;

/// How many connection attempts to make while another process holds the
/// database file briefly (e.g. an updater running right after the app quits).
const CONNECT_MAX_ATTEMPTS: usize = 5;
const CONNECT_RETRY_DELAY_MS: u64 = 200;

pub const CELESTIAL_DB_FILE_NAME: &str = "celestial.db";

pub struct CelestialDb {
	pub(crate) pool: SqlitePool,
}

impl CelestialDb {
	pub fn pool(&self) -> &Pool<Sqlite> {
		&self.pool
	}

	pub(crate) async fn connect(settings_dir: &Path) -> crate::Result<Self> {
		let path = settings_dir.join(CELESTIAL_DB_FILE_NAME);
		let mut attempt = 0;
		let pool = loop {
			match open_pool(&path).await {
				Ok(pool) => break pool,
				Err(error) => {
					attempt += 1;
					if attempt >= CONNECT_MAX_ATTEMPTS {
						return Err(error.into());
					}
					tokio::time::sleep(std::time::Duration::from_millis(
						CONNECT_RETRY_DELAY_MS,
					))
					.await;
				}
			}
		};

		run_migrations(&pool).await?;

		Ok(Self { pool })
	}

	pub(crate) async fn get_meta(
		&self,
		key: &str,
	) -> crate::Result<Option<String>> {
		let value: Option<(String,)> =
			sqlx::query_as("SELECT value FROM meta WHERE key = ?")
				.bind(key)
				.fetch_optional(&self.pool)
				.await?;
		Ok(value.map(|(value,)| value))
	}

	pub(crate) async fn set_meta(
		&self,
		key: &str,
		value: &str,
	) -> crate::Result<()> {
		sqlx::query(
			"INSERT INTO meta (key, value) VALUES (?, ?)
			 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
		)
		.bind(key)
		.bind(value)
		.execute(&self.pool)
		.await?;
		Ok(())
	}
}

async fn open_pool(path: &Path) -> Result<SqlitePool, sqlx::Error> {
	// Per-connection pragmas go through SqliteConnectOptions so every pooled
	// connection gets them; setting them once after connect would not.
	let options = SqliteConnectOptions::new()
		.filename(path)
		.create_if_missing(true)
		.journal_mode(SqliteJournalMode::Wal)
		.synchronous(SqliteSynchronous::Normal)
		.foreign_keys(true);
	SqlitePoolOptions::new().connect_with(options).await
}

async fn run_migrations(pool: &SqlitePool) -> crate::Result<()> {
	sqlx::migrate!("./migrations-celestial")
		.run(pool)
		.await
		.map_err(|error| {
			crate::ErrorKind::FSError(format!(
				"celestial.db migration failed: {error}"
			))
		})?;
	Ok(())
}

/// One-time import of the legacy JSON files this database replaces. Each
/// domain imports only when its `meta` marker is unset, and the marker is set
/// whether or not a legacy file existed, so a source deleted later is never
/// resurrected by a stale file. The legacy files themselves stay on disk
/// untouched; cleanup is a separate future step.
pub(crate) async fn ensure_imported(state: &State) -> crate::Result<()> {
	libraries::ensure_imported(state).await?;
	groups::ensure_imported(&state.celestial, &state.directories.settings_dir).await?;
	content_sources::ensure_imported(state).await?;
	ui_settings::ensure_imported(state).await?;
	translation_cache::ensure_imported(state).await?;
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

	#[tokio::test]
	async fn migration_creates_all_tables() {
		let (_dir, db) = test_db().await;
		let tables: Vec<(String,)> =
			sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx%'")
				.fetch_all(db.pool())
				.await
				.unwrap();
		let mut names: Vec<_> = tables.into_iter().map(|(n,)| n).collect();
		names.sort();
		assert_eq!(
			names,
			vec![
				"announcement_body_cache",
				"announcement_preferences",
				"announcement_sync",
				"announcements",
				"content_sources",
				"group_memberships",
				"instance_groups",
				"libraries",
				"meta",
				"translation_cache",
				"ui_settings",
			]
		);
	}

	#[tokio::test]
	async fn ui_settings_roundtrip() {
		let (_dir, db) = test_db().await;
		assert_eq!(
			ui_settings::get_json(db.pool(), "split_view_settings")
				.await
				.unwrap(),
			None
		);
		ui_settings::set_json(db.pool(), "split_view_settings", "{\"enabled\":true}")
			.await
			.unwrap();
		// Upsert overwrites rather than duplicating the row.
		ui_settings::set_json(db.pool(), "split_view_settings", "{\"enabled\":false}")
			.await
			.unwrap();
		assert_eq!(
			ui_settings::get_json(db.pool(), "split_view_settings")
				.await
				.unwrap()
				.as_deref(),
			Some("{\"enabled\":false}")
		);
		ui_settings::delete(db.pool(), "split_view_settings")
			.await
			.unwrap();
		assert_eq!(
			ui_settings::get_json(db.pool(), "split_view_settings")
				.await
				.unwrap(),
			None
		);
	}

	#[tokio::test]
	async fn translation_cache_roundtrip() {
		let (_dir, db) = test_db().await;
		let document = serde_json::json!({
			"version": 1,
			"entries": {
				"transmart:zh:1:a": {"t": "你好", "at": 1000i64},
				"transmart:zh:2:b": {"t": "世界", "at": 2000i64},
			}
		});
		translation_cache::replace_json(db.pool(), &document.to_string())
			.await
			.unwrap();

		let loaded: serde_json::Value =
			serde_json::from_str(&translation_cache::load_json(db.pool()).await.unwrap())
				.unwrap();
		assert_eq!(
			loaded["entries"]["transmart:zh:1:a"]["t"],
			serde_json::json!("你好")
		);

		// Saving an empty map clears the table instead of leaving stale rows.
		translation_cache::replace_json(db.pool(), "{\"version\":1,\"entries\":{}}")
			.await
			.unwrap();
		assert_eq!(
			serde_json::from_str::<serde_json::Value>(
				&translation_cache::load_json(db.pool()).await.unwrap(),
			)
			.unwrap(),
			serde_json::json!({"version": 1, "entries": {}})
		);
		translation_cache::clear(db.pool()).await.unwrap();
	}

	#[tokio::test]
	async fn groups_roundtrip_preserves_order_and_memberships() {
		let (_dir, db) = test_db().await;
		let file = crate::state::instance_groups::InstanceGroupsFile {
			schema_version: 1,
			groups: vec![
				crate::state::instance_groups::GroupDefinition {
					id: "group:favorites".into(),
					name: "Favorites".into(),
				},
				crate::state::instance_groups::GroupDefinition {
					id: "g1".into(),
					name: "原版".into(),
				},
			],
			memberships: std::collections::HashMap::from([(
				"local:abc".to_string(),
				vec!["g1".to_string(), "group:favorites".to_string()],
			)]),
		};
		groups::save(&db, &file).await.unwrap();
		let loaded = groups::load(&db).await.unwrap();
		assert_eq!(loaded.groups.len(), 2);
		assert_eq!(loaded.groups[0].id, "group:favorites");
		assert_eq!(loaded.groups[1].name, "原版");
		assert_eq!(
			loaded.memberships.get("local:abc").unwrap(),
			&vec!["g1".to_string(), "group:favorites".to_string()]
		);
	}
}

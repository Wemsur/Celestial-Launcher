-- Celestial fork storage (celestial.db)
--
-- Holds only fork-owned persistent state, deliberately separate from app.db so
-- that upstream schema churn in migrations/ and .sqlx never touches this file.
-- Queries against this database use runtime SQL (sqlx::query), not the query!
-- macro: no offline cache, no build-time database dependency.
--
-- Legacy sources migrated into this database (kept on disk read-only):
--   libraries.json -> libraries (+ meta keys)
--   instance_groups.json -> instance_groups / group_memberships
--   content-sources.json -> content_sources
--   custom_backgrounds/celestial_settings.json -> ui_settings keys
--   interface/*.json, translation/settings.json -> ui_settings keys
--   translation/cache.json -> translation_cache

CREATE TABLE meta (
	key TEXT NOT NULL,
	value TEXT NOT NULL,

	PRIMARY KEY (key)
);

CREATE TABLE libraries (
	path TEXT NOT NULL,
	name TEXT NOT NULL,
	format TEXT NOT NULL,
	display_order INTEGER NOT NULL,

	PRIMARY KEY (path)
);

CREATE TABLE instance_groups (
	id TEXT NOT NULL,
	name TEXT NOT NULL,
	display_order INTEGER NOT NULL,

	PRIMARY KEY (id)
);

CREATE TABLE group_memberships (
	instance_id TEXT NOT NULL,
	group_id TEXT NOT NULL,
	position INTEGER NOT NULL DEFAULT 0,

	PRIMARY KEY (instance_id, group_id),
	FOREIGN KEY (group_id) REFERENCES instance_groups(id) ON DELETE CASCADE
);

CREATE TABLE content_sources (
	id TEXT NOT NULL,
	enabled INTEGER NOT NULL DEFAULT 0,
	installed_at INTEGER,
	source TEXT,

	PRIMARY KEY (id)
);

CREATE TABLE ui_settings (
	key TEXT NOT NULL,
	value TEXT NOT NULL,
	updated_at INTEGER NOT NULL,

	PRIMARY KEY (key)
);

CREATE TABLE translation_cache (
	cache_key TEXT NOT NULL,
	entry_json TEXT NOT NULL,
	created_at INTEGER NOT NULL,

	PRIMARY KEY (cache_key)
);

CREATE INDEX translation_cache_created_at ON translation_cache(created_at);

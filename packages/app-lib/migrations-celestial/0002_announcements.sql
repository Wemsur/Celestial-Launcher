CREATE TABLE announcements (
	id TEXT PRIMARY KEY NOT NULL,
	metadata_json TEXT NOT NULL,
	published_at INTEGER NOT NULL,
	state TEXT NOT NULL CHECK (state IN ('published', 'ended', 'withdrawn'))
);

CREATE TABLE announcement_body_cache (
	id TEXT NOT NULL,
	body_sha256 TEXT NOT NULL,
	metadata_json TEXT NOT NULL,
	markdown TEXT NOT NULL,
	byte_length INTEGER NOT NULL,
	accessed_at INTEGER NOT NULL,
	PRIMARY KEY (id, body_sha256),
	FOREIGN KEY (id) REFERENCES announcements(id) ON DELETE CASCADE
);

CREATE INDEX announcement_body_cache_accessed ON announcement_body_cache(accessed_at);

CREATE TABLE announcement_preferences (
	id TEXT PRIMARY KEY NOT NULL,
	suppressed_at INTEGER NOT NULL
);

CREATE TABLE announcement_sync (
	id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
	generation INTEGER NOT NULL,
	feed_sha256 TEXT NOT NULL,
	last_success_at INTEGER NOT NULL
);

ALTER TABLE artists ADD COLUMN art_path  TEXT;
ALTER TABLE artists ADD COLUMN art_size  INTEGER;
ALTER TABLE artists ADD COLUMN art_mtime INTEGER;

CREATE TABLE IF NOT EXISTS server_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

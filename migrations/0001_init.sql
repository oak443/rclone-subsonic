CREATE TABLE IF NOT EXISTS artists (
    id        TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    sort_name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS albums (
    id          TEXT PRIMARY KEY,
    artist_id   TEXT NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    sort_name   TEXT NOT NULL,
    year        INTEGER,
    genre       TEXT,
    dir_path    TEXT NOT NULL UNIQUE,
    cover_path  TEXT,
    cover_size  INTEGER,
    cover_mtime INTEGER,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    song_count  INTEGER NOT NULL DEFAULT 0,
    added_at    INTEGER NOT NULL,
    generation  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_albums_artist ON albums(artist_id);
CREATE INDEX IF NOT EXISTS idx_albums_sort ON albums(sort_name);
CREATE INDEX IF NOT EXISTS idx_albums_added ON albums(added_at);

CREATE TABLE IF NOT EXISTS tracks (
    id           TEXT PRIMARY KEY,
    album_id     TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    artist_id    TEXT NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    sort_name    TEXT NOT NULL,
    track_no     INTEGER NOT NULL DEFAULT 0,
    disc_no      INTEGER NOT NULL DEFAULT 1,
    file_path    TEXT NOT NULL UNIQUE,
    file_size    INTEGER NOT NULL,
    file_mtime   INTEGER NOT NULL,
    duration_ms  INTEGER NOT NULL DEFAULT 0,
    bitrate      INTEGER NOT NULL DEFAULT 0,
    suffix       TEXT NOT NULL,
    content_type TEXT NOT NULL,
    generation   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album_id);
CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist_id);
CREATE INDEX IF NOT EXISTS idx_tracks_sort ON tracks(sort_name);

CREATE TABLE IF NOT EXISTS scan_state (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    last_scan  INTEGER,
    last_error TEXT
);

INSERT OR IGNORE INTO scan_state (id) VALUES (1);

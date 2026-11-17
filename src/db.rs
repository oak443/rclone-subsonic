use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rand::RngCore;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{QueryBuilder, Sqlite, SqlitePool, Transaction};

use crate::ids::{self, EntityKind};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArtistRow {
    pub id: String,
    pub name: String,
    pub album_count: i64,
    pub art_path: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AlbumRow {
    pub id: String,
    pub artist_id: String,
    pub name: String,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub duration_ms: i64,
    pub song_count: i64,
    pub added_at: i64,
    pub artist_name: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SongRow {
    pub id: String,
    pub album_id: String,
    pub artist_id: String,
    pub title: String,
    pub track_no: i64,
    pub disc_no: i64,
    pub file_path: String,
    pub file_size: i64,
    pub duration_ms: i64,
    pub bitrate: i64,
    pub suffix: String,
    pub content_type: String,
    pub album_name: String,
    pub artist_name: String,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub album_added_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ExistingTrack {
    pub file_path: String,
    pub file_size: i64,
    pub file_mtime: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CoverRow {
    pub owner_id: String,
    pub cover_path: Option<String>,
    pub cover_size: Option<i64>,
    pub cover_mtime: Option<i64>,
}

pub async fn connect(path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("创建数据库目录失败: {}", parent.display()))?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(10))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .with_context(|| format!("打开数据库失败: {}", path.display()))?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("执行数据库迁移失败")?;
    Ok(pool)
}

pub struct AlbumUpsert<'a> {
    pub id: &'a str,
    pub artist_id: &'a str,
    pub name: &'a str,
    pub sort_name: &'a str,
    pub year: Option<i64>,
    pub genre: Option<&'a str>,
    pub dir_path: &'a str,
    pub cover_path: Option<&'a str>,
    pub cover_size: Option<i64>,
    pub cover_mtime: Option<i64>,
    pub duration_ms: i64,
    pub song_count: i64,
    pub added_at: i64,
    pub generation: i64,
}

pub struct TrackUpsert<'a> {
    pub id: &'a str,
    pub album_id: &'a str,
    pub artist_id: &'a str,
    pub title: &'a str,
    pub sort_name: &'a str,
    pub track_no: i64,
    pub disc_no: i64,
    pub file_path: &'a str,
    pub file_size: i64,
    pub file_mtime: i64,
    pub duration_ms: i64,
    pub bitrate: i64,
    pub suffix: &'a str,
    pub content_type: &'a str,
    pub generation: i64,
}

pub async fn upsert_artist(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    name: &str,
    sort_name: &str,
    art: Option<(&str, i64, i64)>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO artists (id, name, sort_name, art_path, art_size, art_mtime)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           sort_name = excluded.sort_name,
           art_path = excluded.art_path,
           art_size = excluded.art_size,
           art_mtime = excluded.art_mtime",
    )
    .bind(id)
    .bind(name)
    .bind(sort_name)
    .bind(art.map(|value| value.0))
    .bind(art.map(|value| value.1))
    .bind(art.map(|value| value.2))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn upsert_album(tx: &mut Transaction<'_, Sqlite>, album: AlbumUpsert<'_>) -> Result<()> {
    sqlx::query(
        "INSERT INTO albums
           (id, artist_id, name, sort_name, year, genre, dir_path, cover_path, cover_size,
            cover_mtime, duration_ms, song_count, added_at, generation)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           artist_id = excluded.artist_id,
           name = excluded.name,
           sort_name = excluded.sort_name,
           year = excluded.year,
           genre = excluded.genre,
           dir_path = excluded.dir_path,
           cover_path = excluded.cover_path,
           cover_size = excluded.cover_size,
           cover_mtime = excluded.cover_mtime,
           duration_ms = excluded.duration_ms,
           song_count = excluded.song_count,
           generation = excluded.generation",
    )
    .bind(album.id)
    .bind(album.artist_id)
    .bind(album.name)
    .bind(album.sort_name)
    .bind(album.year)
    .bind(album.genre)
    .bind(album.dir_path)
    .bind(album.cover_path)
    .bind(album.cover_size)
    .bind(album.cover_mtime)
    .bind(album.duration_ms)
    .bind(album.song_count)
    .bind(album.added_at)
    .bind(album.generation)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn upsert_track(tx: &mut Transaction<'_, Sqlite>, track: TrackUpsert<'_>) -> Result<()> {
    sqlx::query(
        "INSERT INTO tracks
           (id, album_id, artist_id, title, sort_name, track_no, disc_no, file_path, file_size,
            file_mtime, duration_ms, bitrate, suffix, content_type, generation)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           album_id = excluded.album_id,
           artist_id = excluded.artist_id,
           title = excluded.title,
           sort_name = excluded.sort_name,
           track_no = excluded.track_no,
           disc_no = excluded.disc_no,
           file_path = excluded.file_path,
           file_size = excluded.file_size,
           file_mtime = excluded.file_mtime,
           duration_ms = excluded.duration_ms,
           bitrate = excluded.bitrate,
           suffix = excluded.suffix,
           content_type = excluded.content_type,
           generation = excluded.generation",
    )
    .bind(track.id)
    .bind(track.album_id)
    .bind(track.artist_id)
    .bind(track.title)
    .bind(track.sort_name)
    .bind(track.track_no)
    .bind(track.disc_no)
    .bind(track.file_path)
    .bind(track.file_size)
    .bind(track.file_mtime)
    .bind(track.duration_ms)
    .bind(track.bitrate)
    .bind(track.suffix)
    .bind(track.content_type)
    .bind(track.generation)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn prune_stale(tx: &mut Transaction<'_, Sqlite>, generation: i64) -> Result<()> {
    sqlx::query("DELETE FROM tracks WHERE generation <> ?")
        .bind(generation)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM albums WHERE generation <> ?")
        .bind(generation)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM artists WHERE id NOT IN (SELECT artist_id FROM albums)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn existing_tracks(pool: &SqlitePool, album_id: &str) -> Result<Vec<ExistingTrack>> {
    let rows = sqlx::query_as::<_, ExistingTrack>(
        "SELECT file_path, file_size, file_mtime, duration_ms FROM tracks WHERE album_id = ?",
    )
    .bind(album_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn album_count(pool: &SqlitePool) -> Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM albums")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn track_count(pool: &SqlitePool) -> Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn get_last_scan(pool: &SqlitePool) -> Result<Option<i64>> {
    let value: Option<i64> = sqlx::query_scalar("SELECT last_scan FROM scan_state WHERE id = 1")
        .fetch_one(pool)
        .await?;
    Ok(value)
}

pub async fn set_last_scan(
    pool: &SqlitePool,
    timestamp: Option<i64>,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE scan_state SET last_scan = COALESCE(?, last_scan), last_error = ? WHERE id = 1",
    )
    .bind(timestamp)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

const ALBUM_SELECT: &str = "SELECT al.id, al.artist_id, al.name, al.year, al.genre,
        al.duration_ms, al.song_count, al.added_at, ar.name AS artist_name
     FROM albums al JOIN artists ar ON ar.id = al.artist_id";

pub async fn list_artists(pool: &SqlitePool) -> Result<Vec<ArtistRow>> {
    let rows = sqlx::query_as::<_, ArtistRow>(
        "SELECT ar.id, ar.name, ar.art_path, COUNT(al.id) AS album_count
         FROM artists ar LEFT JOIN albums al ON al.artist_id = ar.id
         GROUP BY ar.id, ar.name, ar.art_path
         ORDER BY ar.sort_name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn artist_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ArtistRow>> {
    let row = sqlx::query_as::<_, ArtistRow>(
        "SELECT ar.id, ar.name, ar.art_path, COUNT(al.id) AS album_count
         FROM artists ar LEFT JOIN albums al ON al.artist_id = ar.id
         WHERE ar.id = ?
         GROUP BY ar.id, ar.name, ar.art_path",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn albums_by_artist(pool: &SqlitePool, artist_id: &str) -> Result<Vec<AlbumRow>> {
    let sql = format!("{ALBUM_SELECT} WHERE al.artist_id = ? ORDER BY al.year, al.sort_name");
    let rows = sqlx::query_as::<_, AlbumRow>(&sql)
        .bind(artist_id)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn album_by_id(pool: &SqlitePool, id: &str) -> Result<Option<AlbumRow>> {
    let sql = format!("{ALBUM_SELECT} WHERE al.id = ?");
    let row = sqlx::query_as::<_, AlbumRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn albums_for_list(
    pool: &SqlitePool,
    order: &str,
    limit: i64,
    offset: i64,
    from_year: Option<i64>,
    to_year: Option<i64>,
    genre: Option<&str>,
) -> Result<Vec<AlbumRow>> {
    let mut qb = QueryBuilder::<Sqlite>::new(ALBUM_SELECT);
    qb.push(" WHERE 1 = 1");
    if let Some(year) = from_year {
        qb.push(" AND al.year >= ").push_bind(year);
    }
    if let Some(year) = to_year {
        qb.push(" AND al.year <= ").push_bind(year);
    }
    if let Some(genre) = genre {
        qb.push(" AND al.genre = ").push_bind(genre.to_string());
    }
    qb.push(" ORDER BY ").push(order);
    qb.push(" LIMIT ").push_bind(limit);
    qb.push(" OFFSET ").push_bind(offset);
    let rows = qb.build_query_as::<AlbumRow>().fetch_all(pool).await?;
    Ok(rows)
}

pub async fn random_songs(
    pool: &SqlitePool,
    limit: i64,
    genre: Option<&str>,
    from_year: Option<i64>,
    to_year: Option<i64>,
) -> Result<Vec<SongRow>> {
    let mut qb = QueryBuilder::<Sqlite>::new(SONG_SELECT);
    qb.push(" WHERE 1 = 1");
    if let Some(genre) = genre {
        qb.push(" AND al.genre = ").push_bind(genre.to_string());
    }
    if let Some(year) = from_year {
        qb.push(" AND al.year >= ").push_bind(year);
    }
    if let Some(year) = to_year {
        qb.push(" AND al.year <= ").push_bind(year);
    }
    qb.push(" ORDER BY RANDOM() LIMIT ").push_bind(limit);
    let rows = qb.build_query_as::<SongRow>().fetch_all(pool).await?;
    Ok(rows)
}

const SONG_SELECT: &str = "SELECT t.id, t.album_id, t.artist_id, t.title, t.track_no,
        t.disc_no, t.file_path, t.file_size, t.duration_ms, t.bitrate,
        t.suffix, t.content_type, al.name AS album_name, ar.name AS artist_name, al.year,
        al.genre, al.added_at AS album_added_at
     FROM tracks t
     JOIN albums al ON al.id = t.album_id
     JOIN artists ar ON ar.id = t.artist_id";

pub async fn songs_by_album(pool: &SqlitePool, album_id: &str) -> Result<Vec<SongRow>> {
    let sql =
        format!("{SONG_SELECT} WHERE t.album_id = ? ORDER BY t.disc_no, t.track_no, t.sort_name");
    let rows = sqlx::query_as::<_, SongRow>(&sql)
        .bind(album_id)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn song_by_id(pool: &SqlitePool, id: &str) -> Result<Option<SongRow>> {
    let sql = format!("{SONG_SELECT} WHERE t.id = ?");
    let row = sqlx::query_as::<_, SongRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// 按实体类型解析封面：artist 只读 artists 表，绝不跨实体回退。
pub async fn art_by_id(pool: &SqlitePool, id: &str) -> Result<Option<CoverRow>> {
    let row = match ids::entity_kind(id) {
        Some(EntityKind::Artist) => {
            sqlx::query_as::<_, CoverRow>(
                "SELECT id AS owner_id, art_path AS cover_path,
                        art_size AS cover_size, art_mtime AS cover_mtime
                 FROM artists WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
        Some(EntityKind::Album) => {
            sqlx::query_as::<_, CoverRow>(
                "SELECT id AS owner_id, cover_path, cover_size, cover_mtime
                 FROM albums WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
        Some(EntityKind::Track) => {
            sqlx::query_as::<_, CoverRow>(
                "SELECT al.id AS owner_id, al.cover_path, al.cover_size, al.cover_mtime
                 FROM tracks t JOIN albums al ON al.id = t.album_id
                 WHERE t.id = ?",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
        None => None,
    };
    Ok(row)
}

/// 读取或首次生成图片 URL 签名密钥（持久化在 server_state 表）。
pub async fn image_secret(pool: &SqlitePool) -> Result<Vec<u8>> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT value FROM server_state WHERE key = 'image_secret'")
            .fetch_optional(pool)
            .await?;
    if let Some(bytes) = existing
        .and_then(|value| hex::decode(value).ok())
        .filter(|bytes| !bytes.is_empty())
    {
        return Ok(bytes);
    }

    let mut generated = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut generated);
    sqlx::query("INSERT OR IGNORE INTO server_state (key, value) VALUES ('image_secret', ?)")
        .bind(hex::encode(&generated))
        .execute(pool)
        .await?;
    let stored: String =
        sqlx::query_scalar("SELECT value FROM server_state WHERE key = 'image_secret'")
            .fetch_one(pool)
            .await?;
    let secret = hex::decode(stored).context("解析 image_secret 失败")?;
    if secret.is_empty() {
        anyhow::bail!("image_secret 为空");
    }
    Ok(secret)
}

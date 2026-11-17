use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value, json};

use super::response::{ApiError, Format};
use super::{AppState, Params};
use crate::db::{self, AlbumRow, ArtistRow, SongRow};
use crate::ids::{self, EntityKind};
use crate::media;

const IGNORED_ARTICLES: &str = "The A An";

pub async fn api(
    state: &AppState,
    method: &str,
    params: &Params,
    base_url: &str,
) -> Result<Value, ApiError> {
    match method {
        "ping" => Ok(json!({})),
        "getLicense" => Ok(json!({ "license": { "valid": true } })),
        "getMusicFolders" => Ok(json!({
            "musicFolders": {
                "musicFolder": [{
                    "id": "1",
                    "name": state.config.server.music_folder_name,
                }]
            }
        })),
        "getUser" => Ok(json!({
            "user": {
                "username": state.config.auth.username,
                "email": "",
                "scrobblingEnabled": false,
                "adminRole": true,
                "settingsRole": true,
                "downloadRole": true,
                "uploadRole": false,
                "playlistRole": true,
                "coverArtRole": true,
                "commentRole": false,
                "podcastRole": false,
                "streamRole": true,
                "jukeboxRole": false,
                "shareRole": false,
                "videoConversionRole": false,
                "folder": [1],
            }
        })),
        "getOpenSubsonicExtensions" => Ok(json!({ "openSubsonicExtensions": [] })),
        "getIndexes" => get_indexes(state, base_url).await,
        "getArtists" => get_artists(state, base_url).await,
        "getArtist" => get_artist(state, params, base_url).await,
        "getAlbum" => get_album(state, params).await,
        "getSong" => get_song(state, params).await,
        "getMusicDirectory" => get_music_directory(state, params, base_url).await,
        "getAlbumList2" => album_list(state, params, "albumList2").await,
        "getAlbumList" => album_list(state, params, "albumList").await,
        "getRandomSongs" => random_songs(state, params).await,
        "getScanStatus" => scan_status(state).await,
        "startScan" => {
            state.scanner.spawn_scan();
            scan_status(state).await
        }
        "getStarred2" => Ok(starred_value("starred2")),
        "getStarred" => Ok(starred_value("starred")),
        "getPlaylists" => Ok(json!({ "playlists": { "playlist": [] } })),
        "getPlaylist" => Err(ApiError::not_found("Playlist not found")),
        "createPlaylist" | "updatePlaylist" | "deletePlaylist" => {
            Err(ApiError::generic("Playlists are not supported yet"))
        }
        "getPlayQueue" => Ok(json!({
            "playQueue": {
                "changed": "",
                "changedBy": "",
                "current": "",
                "position": 0,
                "username": state.config.auth.username,
                "entry": [],
            }
        })),
        "getNowPlaying" => Ok(json!({ "nowPlaying": { "entry": [] } })),
        "getGenres" => Ok(json!({ "genres": { "genre": [] } })),
        "getSongsByGenre" => Ok(json!({ "songsByGenre": { "song": [] } })),
        "getArtistInfo2" => get_artist_info(state, params, base_url, "artistInfo2").await,
        "getArtistInfo" => get_artist_info(state, params, base_url, "artistInfo").await,
        // Aonsoku 调用 getAlbumInfo2 但读取 albumInfo，两个 key 都返回以兼容
        "getAlbumInfo2" => Ok(json!({
            "albumInfo2": empty_album_info(),
            "albumInfo": empty_album_info(),
        })),
        "getAlbumInfo" => Ok(json!({ "albumInfo": empty_album_info() })),
        "getTopSongs" => Ok(json!({ "topSongs": { "song": [] } })),
        "getSimilarSongs2" => Ok(json!({ "similarSongs2": { "song": [] } })),
        "getSimilarSongs" => Ok(json!({ "similarSongs": { "song": [] } })),
        "getLyrics" => Ok(json!({ "lyrics": {} })),
        "getLyricsBySongId" => Ok(json!({ "lyricsList": { "structuredLyrics": [] } })),
        "getPodcasts" => Ok(json!({ "podcasts": { "channel": [] } })),
        "getNewestPodcasts" => Ok(json!({ "newestPodcasts": [] })),
        "getInternetRadioStations" => {
            Ok(json!({ "internetRadioStations": { "internetRadioStation": [] } }))
        }
        "getShares" => Ok(json!({ "shares": { "share": [] } })),
        "getBookmarks" => Ok(json!({ "bookmarks": { "bookmark": [] } })),
        "getChatMessages" => Ok(json!({ "chatMessages": { "chatMessage": [] } })),
        "getVideos" => Ok(json!({ "videos": { "video": [] } })),
        "getCaptions" => Ok(json!({ "captions": {} })),
        "scrobble" | "star" | "unstar" | "setRating" | "savePlayQueue" => Ok(json!({})),
        other => Err(ApiError::generic(format!("Unsupported method: {other}"))),
    }
}

async fn get_indexes(state: &AppState, base_url: &str) -> Result<Value, ApiError> {
    let artists = db::list_artists(&state.db).await?;
    let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for artist in &artists {
        groups
            .entry(index_letter(&artist.name))
            .or_default()
            .push(artist_json(artist, base_url, &state.image_secret));
    }
    let index: Vec<Value> = groups
        .into_iter()
        .map(|(name, artist)| json!({ "name": name, "artist": artist }))
        .collect();
    let last_modified = db::get_last_scan(&state.db).await?.unwrap_or(0);
    Ok(json!({
        "indexes": {
            "ignoredArticles": IGNORED_ARTICLES,
            "lastModified": last_modified,
            "index": index,
        }
    }))
}

async fn get_artists(state: &AppState, base_url: &str) -> Result<Value, ApiError> {
    let artists = db::list_artists(&state.db).await?;
    let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for artist in &artists {
        groups
            .entry(index_letter(&artist.name))
            .or_default()
            .push(artist_json(artist, base_url, &state.image_secret));
    }
    let index: Vec<Value> = groups
        .into_iter()
        .map(|(name, artist)| json!({ "name": name, "artist": artist }))
        .collect();
    Ok(json!({
        "artists": {
            "ignoredArticles": IGNORED_ARTICLES,
            "index": index,
        }
    }))
}

async fn get_artist(state: &AppState, params: &Params, base_url: &str) -> Result<Value, ApiError> {
    let id = required(params, "id")?;
    let artist = db::artist_by_id(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("Artist not found"))?;
    let albums = db::albums_by_artist(&state.db, id).await?;
    let mut value = artist_json(&artist, base_url, &state.image_secret);
    value["album"] = Value::Array(albums.iter().map(|album| album_json(album, None)).collect());
    Ok(json!({ "artist": value }))
}

async fn get_album(state: &AppState, params: &Params) -> Result<Value, ApiError> {
    let id = required(params, "id")?;
    let album = db::album_by_id(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("Album not found"))?;
    let songs = db::songs_by_album(&state.db, id).await?;
    Ok(json!({
        "album": album_json(&album, Some(songs.iter().map(song_json).collect())),
    }))
}

async fn get_song(state: &AppState, params: &Params) -> Result<Value, ApiError> {
    let id = required(params, "id")?;
    let song = db::song_by_id(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("Song not found"))?;
    Ok(json!({ "song": song_json(&song) }))
}

async fn get_music_directory(
    state: &AppState,
    params: &Params,
    base_url: &str,
) -> Result<Value, ApiError> {
    let id = required(params, "id")?;

    if id == "1" {
        let artists = db::list_artists(&state.db).await?;
        let children: Vec<Value> = artists
            .iter()
            .map(|artist| {
                let mut child = artist_json(artist, base_url, &state.image_secret);
                child["parent"] = json!("1");
                child["isDir"] = json!(true);
                child["title"] = json!(artist.name);
                child["name"] = json!(artist.name);
                child["album"] = json!(artist.name);
                child
            })
            .collect();
        return Ok(json!({
            "directory": {
                "id": "1",
                "name": state.config.server.music_folder_name,
                "title": state.config.server.music_folder_name,
                "isDir": true,
                "child": children,
            }
        }));
    }

    match ids::entity_kind(id) {
        Some(EntityKind::Artist) => {
            let artist = db::artist_by_id(&state.db, id)
                .await?
                .ok_or_else(|| ApiError::not_found("Artist not found"))?;
            let albums = db::albums_by_artist(&state.db, id).await?;
            let children: Vec<Value> = albums
                .iter()
                .map(|album| {
                    json!({
                        "id": album.id,
                        "parent": album.artist_id,
                        "isDir": true,
                        "title": album.name,
                        "album": album.name,
                        "artist": album.artist_name,
                        "artistId": album.artist_id,
                        "year": album.year,
                        "genre": album.genre,
                        "coverArt": album.id,
                        "songCount": album.song_count,
                        "duration": album.duration_ms / 1000,
                        "created": iso(album.added_at),
                    })
                })
                .collect();
            Ok(json!({
                "directory": {
                    "id": artist.id,
                    "parent": "1",
                    "name": artist.name,
                    "title": artist.name,
                    "isDir": true,
                    "child": children,
                }
            }))
        }
        Some(EntityKind::Album) => {
            let album = db::album_by_id(&state.db, id)
                .await?
                .ok_or_else(|| ApiError::not_found("Album not found"))?;
            let songs = db::songs_by_album(&state.db, id).await?;
            Ok(json!({
                "directory": {
                    "id": album.id,
                    "parent": album.artist_id,
                    "name": album.name,
                    "title": album.name,
                    "isDir": true,
                    "coverArt": album.id,
                    "child": songs.iter().map(song_json).collect::<Vec<_>>(),
                }
            }))
        }
        _ => Err(ApiError::not_found("Directory not found")),
    }
}

async fn album_list(state: &AppState, params: &Params, wrapper: &str) -> Result<Value, ApiError> {
    let kind = params.get("type").unwrap_or("newest");
    let size = params.get_i64("size").unwrap_or(10).clamp(1, 500);
    let offset = params.get_i64("offset").unwrap_or(0).max(0);
    let from_year = params.get_i64("fromYear");
    let to_year = params.get_i64("toYear");
    let genre = params.get("genre");

    let order = match kind {
        "newest" => "al.added_at DESC, al.sort_name",
        "alphabeticalByName" => "al.sort_name",
        "alphabeticalByArtist" => "ar.sort_name, al.sort_name",
        "random" => "RANDOM()",
        "byYear" => "al.year, al.sort_name",
        "byGenre" => "al.sort_name",
        "recentlyPlayed" | "frequentlyPlayed" | "recent" | "frequent" | "starred" | "highest" => {
            return Ok(wrapped_list(wrapper, "album", Vec::new()));
        }
        _ => "al.sort_name",
    };

    let albums =
        db::albums_for_list(&state.db, order, size, offset, from_year, to_year, genre).await?;
    Ok(wrapped_list(
        wrapper,
        "album",
        albums.iter().map(|album| album_json(album, None)).collect(),
    ))
}

async fn random_songs(state: &AppState, params: &Params) -> Result<Value, ApiError> {
    let size = params.get_i64("size").unwrap_or(50).clamp(1, 500);
    let songs = db::random_songs(
        &state.db,
        size,
        params.get("genre"),
        params.get_i64("fromYear"),
        params.get_i64("toYear"),
    )
    .await?;
    Ok(json!({
        "randomSongs": {
            "song": songs.iter().map(song_json).collect::<Vec<_>>(),
        }
    }))
}

async fn scan_status(state: &AppState) -> Result<Value, ApiError> {
    let status = state.scanner.status();
    let last_scan = db::get_last_scan(&state.db).await?;
    let folder_count = db::album_count(&state.db).await.unwrap_or(0);
    let mut value = json!({
        "scanning": status.scanning,
        "count": status.count,
        "folderCount": folder_count,
    });
    if let Some(timestamp) = last_scan {
        value["lastScan"] = json!(iso(timestamp));
    }
    if let Some(error) = status.error {
        value["error"] = json!(error);
    }
    Ok(json!({ "scanStatus": value }))
}

async fn get_artist_info(
    state: &AppState,
    params: &Params,
    base_url: &str,
    wrapper: &str,
) -> Result<Value, ApiError> {
    let id = required(params, "id")?;
    let artist = db::artist_by_id(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("Artist not found"))?;
    let mut info = empty_artist_info();
    if has_artist_art(&artist) {
        info["smallImageUrl"] = json!(super::image_url(
            base_url,
            &state.image_secret,
            &artist.id,
            300
        ));
        info["mediumImageUrl"] = json!(super::image_url(
            base_url,
            &state.image_secret,
            &artist.id,
            600
        ));
        info["largeImageUrl"] = json!(super::image_url(
            base_url,
            &state.image_secret,
            &artist.id,
            1200
        ));
    }
    let mut root = Map::new();
    root.insert(wrapper.to_string(), info);
    Ok(Value::Object(root))
}

fn artist_json(artist: &ArtistRow, base_url: &str, secret: &[u8]) -> Value {
    let mut value = json!({
        "id": artist.id,
        "name": artist.name,
        "albumCount": artist.album_count,
    });
    if has_artist_art(artist) {
        value["coverArt"] = json!(artist.id);
        value["artistImageUrl"] = json!(super::image_url(base_url, secret, &artist.id, 600));
    }
    value
}

fn has_artist_art(artist: &ArtistRow) -> bool {
    artist
        .art_path
        .as_deref()
        .is_some_and(|path| !path.is_empty())
}

fn album_json(album: &AlbumRow, songs: Option<Vec<Value>>) -> Value {
    let artist = json!({ "id": album.artist_id, "name": album.artist_name });
    let mut value = json!({
        "id": album.id,
        "name": album.name,
        "album": album.name,
        "title": album.name,
        "artist": album.artist_name,
        "artistId": album.artist_id,
        "artists": [artist],
        "parent": album.artist_id,
        "isDir": true,
        "isVideo": false,
        "coverArt": album.id,
        "songCount": album.song_count,
        "duration": album.duration_ms / 1000,
        "playCount": 0,
        "created": iso(album.added_at),
        "year": album.year,
        "genre": album.genre,
    });
    if let Some(songs) = songs {
        value["song"] = Value::Array(songs);
    }
    value
}

fn song_json(song: &SongRow) -> Value {
    let artist = json!({ "id": song.artist_id, "name": song.artist_name });
    json!({
        "id": song.id,
        "parent": song.album_id,
        "isDir": false,
        "title": song.title,
        "album": song.album_name,
        "artist": song.artist_name,
        "artists": [artist],
        "albumArtists": [artist],
        "displayArtist": song.artist_name,
        "displayAlbumArtist": song.artist_name,
        "track": song.track_no,
        "year": song.year,
        "genre": song.genre,
        "coverArt": song.album_id,
        "size": song.file_size,
        "contentType": song.content_type,
        "suffix": song.suffix,
        "duration": song.duration_ms / 1000,
        "bitRate": song.bitrate,
        "path": song.file_path,
        "discNumber": song.disc_no,
        "albumId": song.album_id,
        "artistId": song.artist_id,
        "type": "music",
        "isVideo": false,
        "created": iso(song.album_added_at),
    })
}

fn empty_artist_info() -> Value {
    json!({
        "biography": "",
        "musicBrainzId": "",
        "lastFmUrl": "",
        "smallImageUrl": "",
        "mediumImageUrl": "",
        "largeImageUrl": "",
        "similarArtist": [],
    })
}

fn empty_album_info() -> Value {
    json!({
        "notes": "",
        "musicBrainzId": "",
        "lastFmUrl": "",
        "smallImageUrl": "",
        "mediumImageUrl": "",
        "largeImageUrl": "",
    })
}

fn starred_value(key: &str) -> Value {
    wrapped_list3(key, "artist", "album", "song")
}

fn wrapped_list(wrapper: &str, child: &str, items: Vec<Value>) -> Value {
    let mut inner = Map::new();
    inner.insert(child.to_string(), Value::Array(items));
    let mut root = Map::new();
    root.insert(wrapper.to_string(), Value::Object(inner));
    Value::Object(root)
}

fn wrapped_list3(key: &str, a: &str, b: &str, c: &str) -> Value {
    let mut inner = Map::new();
    inner.insert(a.to_string(), Value::Array(Vec::new()));
    inner.insert(b.to_string(), Value::Array(Vec::new()));
    inner.insert(c.to_string(), Value::Array(Vec::new()));
    let mut root = Map::new();
    root.insert(key.to_string(), Value::Object(inner));
    Value::Object(root)
}

fn index_letter(name: &str) -> String {
    let sorted = media::sort_name(name);
    match sorted.chars().next() {
        Some(ch) if ch.is_ascii_alphabetic() => ch.to_ascii_uppercase().to_string(),
        _ => "#".to_string(),
    }
}

fn required<'a>(params: &'a Params, key: &str) -> Result<&'a str, ApiError> {
    params
        .get(key)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::missing(key))
}

fn iso(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

pub async fn stream(
    state: &AppState,
    params: &Params,
    headers: &HeaderMap,
    method: &Method,
    format: Format,
    download: bool,
) -> Response {
    let Some(id) = params.get("id").filter(|value| !value.is_empty()) else {
        return super::response::fail(format, ApiError::missing("id"));
    };
    let song = match db::song_by_id(&state.db, id).await {
        Ok(Some(song)) => song,
        Ok(None) => {
            return super::response::fail(format, ApiError::not_found("Song not found"));
        }
        Err(error) => return super::response::fail(format, error.into()),
    };

    let names = [
        header::RANGE,
        header::IF_RANGE,
        header::IF_NONE_MATCH,
        header::IF_MODIFIED_SINCE,
    ];
    let forward: Vec<(HeaderName, HeaderValue)> = names
        .iter()
        .filter_map(|name| {
            headers
                .get(name)
                .cloned()
                .map(|value| (name.clone(), value))
        })
        .collect();
    let upstream_method = if method == Method::HEAD {
        Method::HEAD
    } else {
        Method::GET
    };

    let upstream = match state
        .rclone
        .stream(&song.file_path, upstream_method.clone(), &forward)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!("流式读取失败 {}: {error:#}", song.file_path);
            return super::response::plain(StatusCode::BAD_GATEWAY, "upstream error");
        }
    };

    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let mut builder = Response::builder().status(status);
    for name in ["content-length", "content-range", "last-modified", "etag"] {
        if let Some(value) = upstream_headers.get(name) {
            builder = builder.header(name, value.clone());
        }
    }
    builder = builder.header(header::CONTENT_TYPE, media::content_type(&song.suffix));
    builder = builder.header(header::ACCEPT_RANGES, "bytes");
    builder = builder.header(header::CACHE_CONTROL, "private, max-age=0");
    if download {
        builder = builder.header(
            header::CONTENT_DISPOSITION,
            content_disposition(&song.file_path),
        );
    }

    let body = if upstream_method == Method::HEAD {
        Body::empty()
    } else {
        Body::from_stream(upstream.bytes_stream())
    };
    builder.body(body).unwrap_or_else(|error| {
        tracing::error!("构造流响应失败: {error}");
        super::response::plain(StatusCode::INTERNAL_SERVER_ERROR, "response error")
    })
}

pub async fn cover(
    state: &AppState,
    params: &Params,
    headers: &HeaderMap,
    format: Format,
) -> Response {
    let Some(id) = params.get("id").filter(|value| !value.is_empty()) else {
        return super::response::fail(format, ApiError::missing("id"));
    };
    serve_art(state, id, headers).await
}

/// 按实体 id 输出封面/艺术家图；artist 只读 artists 表，不做任何回退。
pub async fn serve_art(state: &AppState, id: &str, headers: &HeaderMap) -> Response {
    let cover = match db::art_by_id(&state.db, id).await {
        Ok(Some(cover)) => cover,
        Ok(None) => return super::response::plain(StatusCode::NOT_FOUND, "cover not found"),
        Err(error) => {
            tracing::warn!("读取封面记录失败: {error:#}");
            return super::response::plain(StatusCode::INTERNAL_SERVER_ERROR, "cover error");
        }
    };
    let Some(path) = cover.cover_path.clone() else {
        return super::response::plain(StatusCode::NOT_FOUND, "cover not found");
    };
    if path.is_empty() {
        return super::response::plain(StatusCode::NOT_FOUND, "cover not found");
    }

    let size = cover.cover_size.unwrap_or(0);
    let mtime = cover.cover_mtime.unwrap_or(0);
    let extension = media::extension(&path);
    let extension = if extension.is_empty() {
        "img".to_string()
    } else {
        extension
    };
    let cache_dir = state.cover_cache.join("covers");
    let cache_path = cache_dir.join(format!(
        "{}-{}-{}.{}",
        cover.owner_id, size, mtime, extension
    ));

    if !cache_path.exists() {
        match state
            .rclone
            .get_bytes_limited(&path, 20 * 1024 * 1024)
            .await
        {
            Ok(bytes) => {
                if let Err(error) = tokio::fs::create_dir_all(&cache_dir).await {
                    tracing::warn!("创建封面缓存目录失败: {error}");
                } else {
                    let temp = cache_path.with_extension("tmp");
                    if let Err(error) = tokio::fs::write(&temp, &bytes).await {
                        tracing::warn!("写入封面缓存失败: {error}");
                    } else if let Err(error) = tokio::fs::rename(&temp, &cache_path).await {
                        tracing::warn!("重命名封面缓存失败: {error}");
                    }
                }
            }
            Err(error) => {
                tracing::warn!("下载封面失败 {}: {error:#}", path);
                return super::response::plain(StatusCode::BAD_GATEWAY, "cover upstream error");
            }
        }
    }

    let data = match tokio::fs::read(&cache_path).await {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!("读取封面缓存失败: {error}");
            return super::response::plain(StatusCode::INTERNAL_SERVER_ERROR, "cover cache error");
        }
    };

    let etag = format!("\"{}-{}-{}\"", cover.owner_id, size, mtime);
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        == Some(etag.as_str())
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .body(Body::empty())
            .unwrap_or_else(|_| Response::new(Body::empty()));
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, media::image_content_type(&path))
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .header(header::ETAG, etag)
        .body(Body::from(data))
        .unwrap_or_else(|error| {
            tracing::error!("构造封面响应失败: {error}");
            super::response::plain(StatusCode::INTERNAL_SERVER_ERROR, "response error")
        })
}

fn content_disposition(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or("download");
    let ascii: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_graphic() && ch != '"' && ch != '\\' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let encoded = utf8_percent_encode(name, NON_ALPHANUMERIC).to_string();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

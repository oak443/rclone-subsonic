use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::json;
use sqlx::SqlitePool;

use crate::config::Config;
use crate::cue::{self, CueSheet, CueTrack};
use crate::db::{self, AlbumUpsert, TrackUpsert};
use crate::flac;
use crate::ids;
use crate::media;
use crate::rclone::{RcloneClient, RemoteItem};

#[derive(Debug, Default, Clone)]
pub struct ScanSummary {
    pub albums: usize,
    pub tracks: usize,
}

#[derive(Debug, Clone)]
pub struct ScanStatusView {
    pub scanning: bool,
    pub count: u64,
    pub error: Option<String>,
}

pub struct Scanner {
    db: SqlitePool,
    rclone: Arc<RcloneClient>,
    remote: String,
    concurrency: usize,
    cover_names: Vec<String>,
    artist_art_names: Vec<String>,
    scanning: AtomicBool,
    count: AtomicU64,
    error: Mutex<Option<String>>,
}

impl Scanner {
    pub async fn new(
        db: SqlitePool,
        rclone: Arc<RcloneClient>,
        config: &Config,
    ) -> Result<Arc<Self>> {
        let count = db::track_count(&db).await.unwrap_or(0).max(0) as u64;
        Ok(Arc::new(Self {
            db,
            rclone,
            remote: config.storage.remote.clone(),
            concurrency: config.scan.list_concurrency.max(1),
            cover_names: config.cover.names.clone(),
            artist_art_names: config.artist_art.names.clone(),
            scanning: AtomicBool::new(false),
            count: AtomicU64::new(count),
            error: Mutex::new(None),
        }))
    }

    pub fn status(&self) -> ScanStatusView {
        ScanStatusView {
            scanning: self.scanning.load(Ordering::SeqCst),
            count: self.count.load(Ordering::SeqCst),
            error: self.error.lock().unwrap().clone(),
        }
    }

    pub fn spawn_scan(self: &Arc<Self>) -> bool {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.begin_state();
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(error) = this.finish_scan().await {
                tracing::error!("扫描失败: {error:#}");
            }
        });
        true
    }

    pub async fn run(self: &Arc<Self>) -> Result<ScanSummary> {
        if self.scanning.swap(true, Ordering::SeqCst) {
            bail!("扫描已在进行中");
        }
        self.begin_state();
        self.finish_scan().await
    }

    fn begin_state(&self) {
        self.count.store(0, Ordering::SeqCst);
        *self.error.lock().unwrap() = None;
    }

    async fn finish_scan(self: &Arc<Self>) -> Result<ScanSummary> {
        let result = self.scan_inner().await;
        self.finalize(result).await
    }

    async fn finalize(&self, result: Result<ScanSummary>) -> Result<ScanSummary> {
        let now = now_ms();
        if result.is_err() {
            let total = db::track_count(&self.db).await.unwrap_or(0).max(0) as u64;
            self.count.store(total, Ordering::SeqCst);
        }
        match &result {
            Ok(_) => {
                if let Err(error) = db::set_last_scan(&self.db, Some(now), None).await {
                    tracing::warn!("写入扫描状态失败: {error:#}");
                }
            }
            Err(error) => {
                let message = format!("{error:#}");
                *self.error.lock().unwrap() = Some(message.clone());
                let _ = db::set_last_scan(&self.db, None, Some(&message)).await;
            }
        }
        self.scanning.store(false, Ordering::SeqCst);
        result
    }

    pub async fn list_root_dirs(&self) -> Result<Vec<RemoteItem>> {
        let root = self
            .rclone
            .list(&self.remote, "", json!({ "dirsOnly": true }))
            .await
            .context("列取存储根目录失败")?;
        Ok(root
            .into_iter()
            .filter(|item| item.is_dir && !item.name.starts_with('.'))
            .collect())
    }

    pub async fn sample_audio(&self, dirs: &[RemoteItem]) -> Result<Option<RemoteItem>> {
        for dir in dirs.iter().take(20) {
            let items = self
                .rclone
                .list(
                    &self.remote,
                    &dir.path,
                    json!({ "recurse": true, "filesOnly": true }),
                )
                .await?;
            if let Some(item) = items.into_iter().find(|item| media::is_audio(&item.name)) {
                return Ok(Some(item));
            }
        }
        Ok(None)
    }

    async fn scan_inner(self: &Arc<Self>) -> Result<ScanSummary> {
        let generation: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(generation), 0) + 1 FROM albums")
                .fetch_one(&self.db)
                .await
                .context("读取扫描代际失败")?;

        let root = self.list_root_dirs().await?;
        let mut artist_dirs: Vec<String> = root.into_iter().map(|item| item.path).collect();
        artist_dirs.sort();

        if artist_dirs.is_empty() {
            let existing = db::album_count(&self.db).await?;
            if existing > 0 {
                bail!("存储根目录没有子目录，拒绝清空现有曲库（请检查 storage.remote）");
            }
            return Ok(ScanSummary::default());
        }

        let mut listing = futures_util::stream::iter(artist_dirs.into_iter().map(|dir| {
            let this = self.clone();
            async move {
                let items = this
                    .rclone
                    .list(
                        &this.remote,
                        &dir,
                        json!({ "recurse": true, "filesOnly": true }),
                    )
                    .await?;
                Ok::<(String, Vec<RemoteItem>), anyhow::Error>((dir, items))
            }
        }))
        .buffer_unordered(self.concurrency);

        let mut summary = ScanSummary::default();
        while let Some(result) = listing.next().await {
            let (artist_dir, items) = result?;
            let artist_art =
                choose_artist_art(&items, &artist_dir, &self.artist_art_names).map(|item| {
                    ArtistArt {
                        path: item.path.clone(),
                        size: item.size,
                        mtime: item.mtime_ms(),
                    }
                });
            let mut groups: HashMap<String, AlbumFiles> = HashMap::new();
            for item in items {
                if item.is_dir || item.name.starts_with('.') {
                    continue;
                }
                let parent = parent_path(&item.path);
                if parent.is_empty() {
                    continue;
                }
                let extension = media::extension(&item.name);
                if media::is_audio(&item.name) {
                    groups
                        .entry(parent.to_string())
                        .or_default()
                        .audio
                        .push(item);
                } else if extension == "cue" {
                    groups
                        .entry(parent.to_string())
                        .or_default()
                        .cues
                        .push(item);
                } else if media::is_image(&item.name) {
                    groups
                        .entry(parent.to_string())
                        .or_default()
                        .images
                        .push(item);
                }
            }

            let mut album_dirs: Vec<String> = groups.keys().cloned().collect();
            album_dirs.sort();
            let mut artist_written = false;
            for album_dir in album_dirs {
                let Some(files) = groups.remove(&album_dir) else {
                    continue;
                };
                if files.audio.is_empty() {
                    continue;
                }
                let tracks = self
                    .process_album(
                        &album_dir,
                        files,
                        generation,
                        artist_art.as_ref(),
                        &mut artist_written,
                    )
                    .await?;
                summary.albums += 1;
                summary.tracks += tracks;
                self.count.store(summary.tracks as u64, Ordering::SeqCst);
            }
        }

        let mut tx = self.db.begin().await.context("开始清理事务失败")?;
        db::prune_stale(&mut tx, generation).await?;
        tx.commit().await?;
        tracing::info!(albums = summary.albums, tracks = summary.tracks, "扫描完成");
        Ok(summary)
    }

    async fn process_album(
        &self,
        album_dir: &str,
        mut files: AlbumFiles,
        generation: i64,
        artist_art: Option<&ArtistArt>,
        artist_written: &mut bool,
    ) -> Result<usize> {
        files.audio.sort_by_key(natural_key);

        let folder_artist = album_dir.split('/').next().unwrap_or(album_dir).to_string();
        let folder_album = album_dir
            .rsplit('/')
            .next()
            .unwrap_or(album_dir)
            .to_string();
        let cue_sheet = self.read_cue(&mut files.cues).await;

        let matched = match &cue_sheet {
            Some(sheet) => match_tracks(sheet, &files.audio),
            None => fallback_tracks(&files.audio),
        };

        let album_title = cue_sheet
            .as_ref()
            .and_then(|sheet| sheet.title.clone())
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| folder_album.clone());
        let year = cue_sheet
            .as_ref()
            .and_then(|sheet| sheet.year)
            .or_else(|| media::extract_year(&folder_album));
        let genre = cue_sheet.as_ref().and_then(|sheet| sheet.genre.clone());
        let disc_no = cue_sheet
            .as_ref()
            .map(|sheet| sheet.disc.max(1) as i64)
            .unwrap_or(1);

        let album_id = ids::album_id(album_dir);
        let artist_id = ids::artist_id(&folder_artist);
        let existing: HashMap<String, db::ExistingTrack> = db::existing_tracks(&self.db, &album_id)
            .await?
            .into_iter()
            .map(|track| (track.file_path.clone(), track))
            .collect();

        let mut built = Vec::with_capacity(matched.len());
        let mut total_duration = 0u64;
        for track in matched {
            let Some(item) = files.audio.get(track.audio_idx) else {
                continue;
            };
            let suffix = media::extension(&item.name);
            let size = item.size.max(0);
            let mtime = item.mtime_ms();
            let duration_ms = match existing.get(&item.path) {
                Some(old) if old.file_size == size && old.file_mtime == mtime => {
                    old.duration_ms.max(0) as u64
                }
                _ if suffix == "flac" => match self.rclone.fetch_head(&item.path, 64).await {
                    Ok(head) => flac::duration_ms(&head).unwrap_or(0),
                    Err(error) => {
                        tracing::warn!("读取 FLAC 头失败 {}: {error:#}", item.path);
                        0
                    }
                },
                _ => 0,
            };
            total_duration += duration_ms;
            built.push(BuiltTrack {
                file_path: item.path.clone(),
                title: track.title,
                track_no: track.number as i64,
                duration_ms,
                size,
                mtime,
                suffix,
            });
        }

        if built.is_empty() {
            return Ok(0);
        }

        let cover = choose_cover(&files.images, &self.cover_names);
        let (cover_path, cover_size, cover_mtime) = match cover {
            Some(item) => (
                Some(item.path.clone()),
                Some(item.size),
                Some(item.mtime_ms()),
            ),
            None => (None, None, None),
        };

        let mut tx = self.db.begin().await.context("开始专辑事务失败")?;
        if !*artist_written {
            db::upsert_artist(
                &mut tx,
                &artist_id,
                &folder_artist,
                &media::sort_name(&folder_artist),
                artist_art.map(|art| (art.path.as_str(), art.size, art.mtime)),
            )
            .await?;
            *artist_written = true;
        }
        db::upsert_album(
            &mut tx,
            AlbumUpsert {
                id: &album_id,
                artist_id: &artist_id,
                name: &album_title,
                sort_name: &media::sort_name(&album_title),
                year,
                genre: genre.as_deref(),
                dir_path: album_dir,
                cover_path: cover_path.as_deref(),
                cover_size,
                cover_mtime,
                duration_ms: total_duration as i64,
                song_count: built.len() as i64,
                added_at: now_ms(),
                generation,
            },
        )
        .await?;

        for track in &built {
            db::upsert_track(
                &mut tx,
                TrackUpsert {
                    id: &ids::track_id(&track.file_path),
                    album_id: &album_id,
                    artist_id: &artist_id,
                    title: &track.title,
                    sort_name: &media::sort_name(&track.title),
                    track_no: track.track_no,
                    disc_no,
                    file_path: &track.file_path,
                    file_size: track.size,
                    file_mtime: track.mtime,
                    duration_ms: track.duration_ms as i64,
                    bitrate: bitrate(track.size, track.duration_ms),
                    suffix: &track.suffix,
                    content_type: media::content_type(&track.suffix),
                    generation,
                },
            )
            .await?;
        }
        tx.commit().await?;

        let count = built.len();
        tracing::debug!(
            album = %album_title,
            artist = %folder_artist,
            tracks = count,
            "专辑已入库"
        );
        Ok(count)
    }

    async fn read_cue(&self, cues: &mut [RemoteItem]) -> Option<CueSheet> {
        if cues.is_empty() {
            return None;
        }
        cues.sort_by_key(|a| a.name.to_lowercase());
        let cue_item = cues.first()?;
        match self
            .rclone
            .get_bytes_limited(&cue_item.path, cue::MAX_BYTES)
            .await
        {
            Ok(bytes) => match cue::parse(&bytes) {
                Ok(sheet) => Some(sheet),
                Err(error) => {
                    tracing::warn!("解析 CUE 失败 {}: {error}", cue_item.path);
                    None
                }
            },
            Err(error) => {
                tracing::warn!("读取 CUE 失败 {}: {error:#}", cue_item.path);
                None
            }
        }
    }
}

#[derive(Default)]
struct AlbumFiles {
    audio: Vec<RemoteItem>,
    cues: Vec<RemoteItem>,
    images: Vec<RemoteItem>,
}

struct BuiltTrack {
    file_path: String,
    title: String,
    track_no: i64,
    duration_ms: u64,
    size: i64,
    mtime: i64,
    suffix: String,
}

struct MatchedTrack {
    audio_idx: usize,
    number: u32,
    title: String,
}

fn match_tracks(sheet: &CueSheet, audio: &[RemoteItem]) -> Vec<MatchedTrack> {
    let mut used = vec![false; audio.len()];
    let mut out = Vec::new();

    for cue_track in &sheet.tracks {
        let candidates: Vec<usize> = (0..audio.len()).filter(|index| !used[*index]).collect();
        if candidates.is_empty() {
            break;
        }
        let Some(index) = find_audio(cue_track, audio, &candidates) else {
            continue;
        };
        used[index] = true;
        let (_, filename_title) = media::parse_filename(&audio[index].name);
        let title = cue_track
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(filename_title);
        out.push(MatchedTrack {
            audio_idx: index,
            number: if cue_track.number == 0 {
                out.len() as u32 + 1
            } else {
                cue_track.number
            },
            title,
        });
    }

    push_unmatched(audio, &used, &mut out);
    out.sort_by_key(|track| (track.number, track.audio_idx));
    out
}

fn push_unmatched(audio: &[RemoteItem], used: &[bool], out: &mut Vec<MatchedTrack>) {
    for (index, item) in audio.iter().enumerate() {
        if used[index] {
            continue;
        }
        let (number, title) = media::parse_filename(&item.name);
        out.push(MatchedTrack {
            audio_idx: index,
            number: number.unwrap_or(out.len() as u32 + 1),
            title,
        });
    }
}

fn fallback_tracks(audio: &[RemoteItem]) -> Vec<MatchedTrack> {
    let mut out = Vec::with_capacity(audio.len());
    push_unmatched(audio, &vec![false; audio.len()], &mut out);
    out
}

fn find_audio(track: &CueTrack, audio: &[RemoteItem], candidates: &[usize]) -> Option<usize> {
    if let Some(file) = &track.file {
        let filename = file.rsplit(['/', '\\']).next().unwrap_or(file);
        if let Some(index) = candidates
            .iter()
            .copied()
            .find(|index| audio[*index].name.eq_ignore_ascii_case(filename))
        {
            return Some(index);
        }
        let normalized = media::norm_stem(filename);
        if let Some(index) = candidates
            .iter()
            .copied()
            .find(|index| media::norm_stem(&audio[*index].name) == normalized)
        {
            return Some(index);
        }
    }
    if let Some(index) = candidates
        .iter()
        .copied()
        .find(|index| media::leading_number(&audio[*index].name) == Some(track.number))
    {
        return Some(index);
    }
    candidates.first().copied()
}

/// 专辑封面只按配置的名字匹配；不兜底任意图片（例如 back.jpg）。
fn choose_cover<'a>(images: &'a [RemoteItem], names: &[String]) -> Option<&'a RemoteItem> {
    for wanted in names {
        let wanted = wanted.to_lowercase();
        if let Some(item) = images
            .iter()
            .find(|item| item.name.to_lowercase() == wanted)
        {
            return Some(item);
        }
    }
    None
}

struct ArtistArt {
    path: String,
    size: i64,
    mtime: i64,
}

/// 艺术家图只认 artist 目录根部的指定文件名；不找专辑目录，也不兜底任意图片。
fn choose_artist_art<'a>(
    items: &'a [RemoteItem],
    artist_dir: &str,
    names: &[String],
) -> Option<&'a RemoteItem> {
    for wanted in names {
        if let Some(item) = items.iter().find(|item| {
            !item.is_dir
                && media::is_image(&item.name)
                && parent_path(&item.path) == artist_dir
                && item.name.eq_ignore_ascii_case(wanted)
        }) {
            return Some(item);
        }
    }
    None
}

fn natural_key(item: &RemoteItem) -> (u32, String) {
    (
        media::leading_number(&item.name).unwrap_or(u32::MAX),
        item.name.to_lowercase(),
    )
}

fn parent_path(path: &str) -> &str {
    match path.rfind('/') {
        Some(index) => &path[..index],
        None => "",
    }
}

fn bitrate(size: i64, duration_ms: u64) -> i64 {
    if duration_ms == 0 || size <= 0 {
        0
    } else {
        size * 8 / duration_ms as i64
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str) -> RemoteItem {
        RemoteItem {
            path: path.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            size: 10,
            mod_time: String::new(),
            is_dir: false,
        }
    }

    fn names() -> Vec<String> {
        vec!["artist.jpg".to_string(), "artist.png".to_string()]
    }

    fn cover_names() -> Vec<String> {
        vec!["cover.jpg".to_string(), "front.jpg".to_string()]
    }

    #[test]
    fn artist_art_is_strict() {
        let items = vec![item("A/Album/artist.jpg"), item("A/cover.jpg")];
        assert!(choose_artist_art(&items, "A", &names()).is_none());
    }

    #[test]
    fn artist_art_prefers_configured_name() {
        let items = vec![
            item("A/artist.png"),
            item("A/artist.jpg"),
            item("A/Album/artist.jpg"),
        ];
        let chosen = choose_artist_art(&items, "A", &names()).expect("artist art");
        assert_eq!(chosen.path, "A/artist.jpg");
    }

    #[test]
    fn artist_art_is_case_insensitive() {
        let items = vec![item("A/Artist.JPG")];
        let chosen = choose_artist_art(&items, "A", &names()).expect("artist art");
        assert_eq!(chosen.path, "A/Artist.JPG");
    }

    #[test]
    fn cover_is_strict() {
        let images = vec![item("A/Album/back.jpg")];
        assert!(choose_cover(&images, &cover_names()).is_none());
    }

    #[test]
    fn cover_prefers_configured_name() {
        let images = vec![item("A/Album/front.jpg"), item("A/Album/cover.jpg")];
        let chosen = choose_cover(&images, &cover_names()).expect("cover");
        assert_eq!(chosen.path, "A/Album/cover.jpg");
    }
}

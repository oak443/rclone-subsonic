use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub cover: CoverConfig,
    #[serde(default)]
    pub artist_art: ArtistArtConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(skip)]
    db_path: PathBuf,
    #[serde(skip)]
    cover_cache_dir: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PathsConfig {
    pub db: Option<PathBuf>,
    pub cover_cache: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_folder_name")]
    pub music_folder_name: String,
    /// 对外访问根地址，用于生成 artistImageUrl；留空则按请求 Host 推导
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
    #[serde(default = "default_true")]
    pub allow_plaintext: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    /// rclone fs string: "gdrive:Music" 或本地路径 "/path/to/Music"
    pub remote: String,
    #[serde(default = "default_true")]
    pub managed: bool,
    #[serde(default = "default_rclone_binary")]
    pub rclone_binary: String,
    /// 外部 rclone serve http 的地址（managed = false 时必填）
    pub http_url: Option<String>,
    pub http_user: Option<String>,
    pub http_pass: Option<String>,
    /// 外部 rclone RC 的地址（managed = false 时必填）
    pub rc_url: Option<String>,
    pub rc_user: Option<String>,
    pub rc_pass: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanConfig {
    #[serde(default = "default_true")]
    pub on_start: bool,
    #[serde(default)]
    pub interval_hours: u64,
    #[serde(default = "default_concurrency")]
    pub list_concurrency: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoverConfig {
    #[serde(default = "default_cover_names")]
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtistArtConfig {
    /// 只匹配艺术家目录根部的这些文件名；不匹配目录内任意图片
    #[serde(default = "default_artist_art_names")]
    pub names: Vec<String>,
}

fn default_bind() -> String {
    "0.0.0.0:4533".to_string()
}

fn default_folder_name() -> String {
    "Music".to_string()
}

fn default_true() -> bool {
    true
}

fn default_rclone_binary() -> String {
    "rclone".to_string()
}

fn default_concurrency() -> usize {
    4
}

fn default_cover_names() -> Vec<String> {
    [
        "cover.jpg",
        "front.jpg",
        "folder.jpg",
        "album.jpg",
        "cover.png",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_artist_art_names() -> Vec<String> {
    ["artist.jpg", "artist.jpeg", "artist.png", "artist.webp"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            music_folder_name: default_folder_name(),
            public_url: None,
        }
    }
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            on_start: true,
            interval_hours: 0,
            list_concurrency: default_concurrency(),
        }
    }
}

impl Default for CoverConfig {
    fn default() -> Self {
        Self {
            names: default_cover_names(),
        }
    }
}

impl Default for ArtistArtConfig {
    fn default() -> Self {
        Self {
            names: default_artist_art_names(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text)
            .with_context(|| format!("解析配置文件失败: {}", path.display()))?;

        if let Ok(password) = std::env::var("RCLONE_SUBSONIC_PASSWORD") {
            cfg.auth.password = password;
        }

        if cfg.auth.username.trim().is_empty() {
            bail!("auth.username 不能为空");
        }
        if cfg.auth.password.is_empty() {
            bail!("auth.password 不能为空（可用 RCLONE_SUBSONIC_PASSWORD 覆盖）");
        }
        if cfg.storage.remote.trim().is_empty() {
            bail!("storage.remote 不能为空");
        }
        if !cfg.storage.managed && (cfg.storage.http_url.is_none() || cfg.storage.rc_url.is_none())
        {
            bail!("storage.managed = false 时必须配置 storage.http_url 和 storage.rc_url");
        }
        if cfg.scan.list_concurrency == 0 {
            cfg.scan.list_concurrency = 1;
        }
        if cfg.cover.names.is_empty() {
            cfg.cover.names = default_cover_names();
        }
        if cfg.artist_art.names.is_empty() {
            cfg.artist_art.names = default_artist_art_names();
        }
        if let Some(url) = cfg.server.public_url.take() {
            let trimmed = url.trim().trim_end_matches('/').to_string();
            cfg.server.public_url = (!trimmed.is_empty()).then_some(trimmed);
        }

        cfg.db_path = expand_path(cfg.paths.db.clone().unwrap_or_else(default_db_path));
        cfg.cover_cache_dir = expand_path(
            cfg.paths
                .cover_cache
                .clone()
                .unwrap_or_else(default_cache_dir),
        );
        Ok(cfg)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn cover_cache_dir(&self) -> &Path {
        &self.cover_cache_dir
    }
}

pub fn default_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("RCLONE_SUBSONIC_CONFIG") {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("rclone-subsonic").join("config.toml")
}

fn default_db_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("rclone-subsonic").join("library.db")
}

fn default_cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("rclone-subsonic")
}

fn expand_path(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    path
}

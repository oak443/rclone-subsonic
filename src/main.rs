mod config;
mod cue;
mod db;
mod flac;
mod ids;
mod media;
mod rclone;
mod scanner;
mod subsonic;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::rclone::{ManagedRclone, RcloneClient};
use crate::scanner::Scanner;
use crate::subsonic::AppState;

#[derive(Debug, Parser)]
#[command(
    name = "rclone-subsonic",
    version,
    about = "在 rclone 音乐库上运行的 Subsonic 服务器"
)]
struct Cli {
    /// 配置文件路径
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 启动 Subsonic HTTP 服务器（默认）
    Serve,
    /// 扫描曲库后退出
    Scan,
    /// 检查 rclone 连通性和文件读取
    Check,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(config::default_config_path);
    let cfg = Arc::new(Config::load(&config_path)?);
    tracing::info!(path = %config_path.display(), "配置已加载");
    tracing::info!(
        db = %cfg.db_path().display(),
        cover_cache = %cfg.cover_cache_dir().display(),
        "数据库与缓存路径"
    );

    let (rclone, _managed) = connect_rclone(&cfg).await?;
    tokio::fs::create_dir_all(cfg.cover_cache_dir())
        .await
        .context("创建封面缓存目录失败")?;
    let db = db::connect(cfg.db_path()).await?;
    let image_secret = db::image_secret(&db).await?;
    let scanner = Scanner::new(db.clone(), rclone.clone(), &cfg).await?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Scan => {
            let summary = scanner.run().await?;
            println!(
                "扫描完成: {} 张专辑, {} 首曲目",
                summary.albums, summary.tracks
            );
        }
        Command::Check => {
            check(&rclone, &cfg).await?;
        }
        Command::Serve => {
            serve(cfg, db, scanner, rclone, image_secret).await?;
        }
    }
    Ok(())
}

async fn connect_rclone(cfg: &Config) -> Result<(Arc<RcloneClient>, Option<ManagedRclone>)> {
    if cfg.storage.managed {
        let (client, managed) =
            rclone::start_managed(&cfg.storage.remote, &cfg.storage.rclone_binary).await?;
        Ok((Arc::new(client), Some(managed)))
    } else {
        let http_url = cfg
            .storage
            .http_url
            .as_deref()
            .context("缺少 storage.http_url")?;
        let rc_url = cfg
            .storage
            .rc_url
            .as_deref()
            .context("缺少 storage.rc_url")?;
        let client = RcloneClient::new(
            http_url,
            rc_url,
            cfg.storage.http_user.clone().unwrap_or_default(),
            cfg.storage.http_pass.clone().unwrap_or_default(),
            cfg.storage.rc_user.clone().unwrap_or_default(),
            cfg.storage.rc_pass.clone().unwrap_or_default(),
        )?;
        Ok((Arc::new(client), None))
    }
}

async fn serve(
    cfg: Arc<Config>,
    db: sqlx::SqlitePool,
    scanner: Arc<Scanner>,
    rclone: Arc<RcloneClient>,
    image_secret: Vec<u8>,
) -> Result<()> {
    let state = Arc::new(AppState {
        config: cfg.clone(),
        db: db.clone(),
        scanner: scanner.clone(),
        rclone,
        cover_cache: cfg.cover_cache_dir().to_path_buf(),
        image_secret,
    });
    let app = subsonic::router(state);

    let listener = tokio::net::TcpListener::bind(&cfg.server.bind)
        .await
        .with_context(|| format!("监听 {} 失败", cfg.server.bind))?;
    tracing::info!(bind = %cfg.server.bind, "Subsonic 服务器已启动");

    if cfg.scan.on_start {
        scanner.spawn_scan();
    }
    if cfg.scan.interval_hours > 0 {
        let scanner = scanner.clone();
        let interval = Duration::from_secs(cfg.scan.interval_hours * 3600);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if !scanner.spawn_scan() {
                    tracing::info!("周期扫描跳过：已有扫描在进行");
                }
            }
        });
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP 服务器异常退出")?;
    tracing::info!("服务器已停止");
    Ok(())
}

async fn check(rclone: &RcloneClient, cfg: &Config) -> Result<()> {
    let version = rclone.rc_version().await?;
    println!(
        "rclone: {}",
        version
            .get("version")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
    );

    let root = rclone
        .list(
            &cfg.storage.remote,
            "",
            serde_json::json!({ "dirsOnly": true }),
        )
        .await?;
    println!("根目录子目录: {} 个", root.len());
    for item in root.iter().take(5) {
        println!("  - {}", item.name);
    }
    if root.len() > 5 {
        println!("  ...");
    }

    let mut sample = None;
    for dir in root.iter().filter(|item| item.is_dir).take(20) {
        let items = rclone
            .list(
                &cfg.storage.remote,
                &dir.path,
                serde_json::json!({ "recurse": true, "filesOnly": true }),
            )
            .await?;
        if let Some(item) = items.iter().find(|item| media::is_audio(&item.name)) {
            sample = Some(item.clone());
            break;
        }
    }
    let audio = sample.context("前 20 个子目录中没有音频文件")?;
    let head = rclone.fetch_head(&audio.path, 64).await?;
    println!(
        "抽样读取: {} ({} bytes, head {} bytes, magic {:?})",
        audio.path,
        audio.size,
        head.len(),
        head.get(0..4).unwrap_or_default()
    );
    println!("检查通过");
    Ok(())
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,rclone_subsonic=debug"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

# 配置说明

## 前置要求

- Rust 1.85+（edition 2024）
- rclone 1.60+（本机在 1.60.1 上验证过；建议用最新版）
- 一个可被 rclone 访问的曲库：远端（如 `gdrive:Music`）或本地路径（如 `/path/to/Music`）
- 首次构建需要能访问 crates.io

如果用的是远端，先确认 `rclone lsf <remote> --max-depth 2` 能正常列出。

## 目录结构

```
Music/
└── <artist>/
    ├── artist.jpg               # 可选：艺术家图
    └── <album>/
        ├── 01 曲名.flac
        ├── 02 曲名.flac
        ├── <album>.cue          # 可选
        └── cover.jpg            # 可选
```

每轨必须是独立音频文件。**不支持「整张专辑一个 FLAC + CUE 分轨」**（按时间切片需要解码，留给后续版本）。

## 构建与配置文件

```bash
cargo build --release
cp config.example.toml ~/.config/rclone-subsonic/config.toml
```

配置项：

- `[storage].remote`：rclone fs 字符串，或本地目录。
- `[auth].username` / `password`：Subsonic 客户端登录用；建议 chmod 600 配置文件，或使用 `RCLONE_SUBSONIC_PASSWORD` 覆盖密码。
- `[server].bind`：默认 `0.0.0.0:4533`，手机需要局域网访问。
- `[server].public_url`：可选，生成 `artistImageUrl` 的对外根地址；反向代理时建议填写，未填则按请求 Host 推导。
- `[artist_art].names`：艺术家图文件名列表，只在 artist 目录根部匹配，**不做任何回退**。
- `[paths]`：数据库和封面缓存位置。

默认 `managed = true`，程序会自己启动 `rclone serve http <remote> --rc`（监听 127.0.0.1 随机端口、随机凭据），退出时回收子进程。`managed = false` 时连接你自己启动的 rclone，见 `config.example.toml` 中的注释。

## 运行方式

```bash
# 只扫描一次
./target/release/rclone-subsonic scan --config ~/.config/rclone-subsonic/config.toml

# 连通性自检：RC、列目录、抽样 Range 读取
./target/release/rclone-subsonic check --config ~/.config/rclone-subsonic/config.toml

# 启动服务（默认子命令也是 serve）
./target/release/rclone-subsonic serve --config ~/.config/rclone-subsonic/config.toml
```

服务启动后：

- `GET /healthz`：扫描状态（无认证，只暴露数量和错误）
- `GET /rest/ping.view?...`：Subsonic API

## 安全

- 服务默认绑定 `0.0.0.0`，请务必改掉示例密码。
- rclone 的 HTTP 与 RC 只绑定 127.0.0.1，凭据每次随机生成。
- 日志不打印 query string（密码/token 不在日志里）。
- 如果要暴露到公网，请在前面加 HTTPS 反向代理，并限制访问来源。

# rclone-subsonic

一个架在 rclone 音乐库上的最小 Subsonic 服务器。

- 目录索引走 **rclone RC**（`operations/list`），音频/封面走 **rclone HTTP + Range**，不需要 FUSE 挂载。
- 元数据来自目录结构、`.cue`、`cover.jpg` 和 `artist.jpg`，不读音频标签，不转码。
- 单用户、单音乐库、SQLite 索引，重点保证浏览、播放、封面和艺术家图。
- JSON 输出按 Feishin 实际使用的字段做了补齐（`artists`/`albumArtists`、`title`、`isDir` 等）；XML 为基础可用版本。
- 已用真实曲库（每轨一个 FLAC + CUE + cover.jpg）验证：扫描、时长、封面、Range 播放、断点/拖动。

## 适用范围

支持的目录结构：

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

## 前置要求

- Rust 1.85+（edition 2024）
- rclone 1.60+（本机在 1.60.1 上验证过；建议用最新版）
- 一个可被 rclone 访问的曲库：远端（如 `gdrive:Music`）或本地路径（如 `/home/saki/Music`）
- 首次构建需要能访问 crates.io

如果用的是远端，先确认 `rclone lsf <remote> --max-depth 2` 能正常列出。

## 构建与配置

```bash
cargo build --release
cp config.example.toml ~/.config/rclone-subsonic/config.toml
```

编辑配置：

- `[storage].remote`：rclone fs 字符串，或本地目录。
- `[auth].username` / `password`：Subsonic 客户端登录用；建议 chmod 600 配置文件，或使用 `RCLONE_SUBSONIC_PASSWORD` 覆盖密码。
- `[server].bind`：默认 `0.0.0.0:4533`，手机需要局域网访问。
- `[server].public_url`：可选，生成 `artistImageUrl` 的对外根地址；反向代理时建议填写，未填则按请求 Host 推导。
- `[artist_art].names`：艺术家图文件名列表，只在 artist 目录根部匹配，**不做任何回退**。
- `[paths]`：数据库和封面缓存位置。

默认 `managed = true`，程序会自己启动 `rclone serve http <remote> --rc`（监听 127.0.0.1 随机端口、随机凭据），退出时回收子进程。

## 运行

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

### systemd

```bash
sudo install -m755 target/release/rclone-subsonic /usr/local/bin/
sudo mkdir -p /etc/rclone-subsonic
sudo cp config.example.toml /etc/rclone-subsonic/config.toml
sudo chmod 600 /etc/rclone-subsonic/config.toml
sudo cp deploy/rclone-subsonic.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now rclone-subsonic
journalctl -u rclone-subsonic -f
```

`managed = true` 时，运行用户需要能找到 `rclone`，并且能读 rclone 配置文件（默认 `~/.config/rclone/rclone.conf`）。

## 客户端

以 Feishin 为例：

1. 添加服务器，类型选 **Subsonic**。
2. 地址填 `http://<主机>:4533`。
3. 用户名/密码填配置里的 `[auth]`。

已实现的主流程：

- `ping`、`getLicense`、`getMusicFolders`、`getUser`
- `getIndexes`、`getArtists`、`getArtist`、`getAlbum`、`getSong`、`getMusicDirectory`
- `getAlbumList2` / `getAlbumList`（`newest`、`alphabeticalByName`、`alphabeticalByArtist`、`random`、`byYear`、`byGenre`）
- `getRandomSongs`
- `stream`（完整 Range / HEAD / 416）、`download`、`getCoverArt`
- `getArtistInfo` / `getArtistInfo2`：仅在艺术家有图时返回图片 URL
- `getScanStatus`、`startScan`
- 认证：`u` + `t`/`s`（推荐），兼容 `p` / `p=enc:hex`

艺术家图规则（对齐 Navidrome 的实体语义，但去掉所有回退）：

- 只读取 `Music/<artist>/artist.*` 中命中 `[artist_art].names` 的文件；
- 不用专辑目录里的图，不用专辑封面代替，不访问外部元数据服务；
- 有图时 `coverArt` = `ar-…`、`artistImageUrl` = 签名公开地址；无图时这两个字段直接省略；
- 图片地址形如 `/share/img/ar-…/<token>?size=600`，可直接取图，URL 中不含登录凭据；`size` 目前只是提示，返回原图；
- `getCoverArt?id=ar-…` 仅在该艺术家有图时返回 200，否则 404。

返回空结果的兼容桩（避免客户端报错）：

- `getPlaylists`（空）、`getPlayQueue`、`getNowPlaying`、`getGenres`、`getSongsByGenre`、`getStarred2`/`getStarred`
- `scrobble`、`star`/`unstar`、`setRating`：返回成功但**不落库**
- `getAlbumInfo2`、`getTopSongs`、`getSimilarSongs2`、播客/电台/分享/书签：空结构

**不支持**：搜索（`search2`/`search3`）、播放列表创建/修改、收藏/评分持久化、歌词、转码、多用户、多音乐库。搜索框在客户端里会不可用或报错。

## 扫描与元数据行为

- 先列根目录，再并发列每个 artist 目录；忽略隐藏文件和非音频文件。
- 专辑 = 音频文件的父目录；artist/album 名取目录名。
- `.cue` 用于标题/年份/流派；编码支持 UTF-8 / Shift-JIS / GBK 自动检测。
- CUE 的 `FILE` 名和实际文件名不一致时，按 `TRACK` 号或顺序匹配（cyanrip 产物实测会不一致）。
- 时长：Range 读取 FLAC 前 42 字节的 STREAMINFO；非 FLAC 或解析失败时记 0。
- 封面：专辑目录内按配置的名字顺序找；首次请求时下载并缓存。
- 艺术家图：只匹配 artist 目录根部的 `artist.*`；缺失时保持缺失，不回退到专辑封面。
- 重扫按 `path + size + mtime` 增量更新；未变化文件不重复读 FLAC 头；消失的文件/专辑会被清理。
- 如果 rclone 根目录意外为空而数据库里已有专辑，扫描会报错并保留旧数据，避免误清库。

## 安全

- 服务默认绑定 `0.0.0.0`，请务必改掉示例密码。
- rclone 的 HTTP 与 RC 只绑定 127.0.0.1，凭据每次随机生成。
- 日志不打印 query string（密码/token 不在日志里）。
- 如果要暴露到公网，请在前面加 HTTPS 反向代理，并限制访问来源。

## 测试

```bash
cargo test
cargo clippy --all-targets
./scripts/smoke.sh
```

`smoke.sh` 会生成一个临时曲库（需要 ffmpeg），启动真实 rclone 和服务器，验证扫描、浏览、Range 播放、封面，以及艺术家图的严格解析与“删图后不回退”。

## 故障排查

- **启动报「rclone 提前退出」**：手工执行 `rclone serve http <remote> --addr 127.0.0.1:18080` 看 remote 是否正确。
- **扫描 0 首**：确认目录是 `<remote>/<artist>/<album>/<file>` 两层结构，文件扩展名在支持列表内。
- **客户端能浏览但不能播放**：检查 `stream` 请求是否带认证参数；查看日志是否出现 `流式读取失败`。
- **封面 404**：确认专辑目录里有 `cover.jpg` 等文件；`back.jpg` 不会被当作正面封面。
- **时长 0**：只对 FLAC 解析 STREAMINFO；MP3/M4A 等当前不计算时长。
- **Feishin 搜索报错**：搜索未实现，属预期行为。

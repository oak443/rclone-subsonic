# rclone-subsonic

把 rclone 里的音乐库变成一个 Subsonic 服务器，供 Feishin 等客户端浏览和播放。

## 安装

需要 Rust 1.85+ 和 rclone。

```bash
git clone https://github.com/oak443/rclone-subsonic.git
cd rclone-subsonic
cargo build --release
```

复制配置并编辑：

```bash
cp config.example.toml ~/.config/rclone-subsonic/config.toml
```

只需要改这几项：

```toml
[storage]
remote = "/path/to/Music"     # 或 rclone 远端，如 "gdrive:Music"

[auth]
username = "youruser"
password = "change-me"        # 一定要改

[server]
bind = "0.0.0.0:4533"         # 手机也要访问就保持 0.0.0.0
```

## 使用

```bash
# 1. 扫描曲库
./target/release/rclone-subsonic scan

# 2. 启动服务
./target/release/rclone-subsonic serve
```

然后在客户端里添加服务器：

1. 类型选 **Subsonic**
2. 地址填 `http://<主机>:4533`
3. 用户名、密码填配置里的 `[auth]`

曲库目录结构：

```
Music/
└── <artist>/
    ├── artist.jpg           # 可选
    └── <album>/
        ├── 01 曲名.flac
        └── cover.jpg        # 可选
```

## 遇到问题

- **扫描到 0 首**：确认曲库是 `<artist>/<album>/<文件>` 两层结构，且文件是音频文件。
- **能浏览但不能播放**：确认客户端 URL 和用户名密码正确，查看服务日志。
- **搜索不可用**：没有实现搜索接口，属预期行为。

更多内容见 [docs/](docs/)：

- [配置说明](docs/configuration.md)
- [部署为 systemd 服务](docs/deployment.md)
- [客户端支持与 API](docs/api.md)
- [扫描与元数据行为](docs/scanning.md)
- [故障排查](docs/troubleshooting.md)

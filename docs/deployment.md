# 部署为 systemd 服务

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

部署前先把 `deploy/rclone-subsonic.service` 里的 `User` / `Group` 改成实际运行用户；该用户需要能读 rclone 配置和音乐库。

`managed = true` 时，运行用户需要能找到 `rclone`，并且能读 rclone 配置文件（默认 `~/.config/rclone/rclone.conf`）。

# 故障排查

- **启动报「rclone 提前退出」**：手工执行 `rclone serve http <remote> --addr 127.0.0.1:18080` 看 remote 是否正确。
- **扫描 0 首**：确认目录是 `<remote>/<artist>/<album>/<file>` 两层结构，文件扩展名在支持列表内。
- **客户端能浏览但不能播放**：检查 `stream` 请求是否带认证参数；查看日志是否出现 `流式读取失败`。
- **封面 404**：确认专辑目录里有 `cover.jpg` 等文件；`back.jpg` 不会被当作正面封面。
- **时长 0**：只对 FLAC 解析 STREAMINFO；MP3/M4A 等当前不计算时长。
- **Feishin 搜索报错**：搜索未实现，属预期行为。

## 测试

```bash
cargo test
cargo clippy --all-targets
./scripts/smoke.sh
```

`smoke.sh` 会生成一个临时曲库（需要 ffmpeg），启动真实 rclone 和服务器，验证扫描、浏览、Range 播放、封面，以及艺术家图的严格解析与“删图后不回退”。

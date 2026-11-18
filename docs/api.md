# 客户端支持与 API

## 客户端

以 Feishin 为例：

1. 添加服务器，类型选 **Subsonic**。
2. 地址填 `http://<主机>:4533`。
3. 用户名/密码填配置里的 `[auth]`。

## 已实现的主流程

- `ping`、`getLicense`、`getMusicFolders`、`getUser`
- `getIndexes`、`getArtists`、`getArtist`、`getAlbum`、`getSong`、`getMusicDirectory`
- `getAlbumList2` / `getAlbumList`（`newest`、`alphabeticalByName`、`alphabeticalByArtist`、`random`、`byYear`、`byGenre`）
- `getRandomSongs`
- `stream`（完整 Range / HEAD / 416）、`download`、`getCoverArt`
- `getArtistInfo` / `getArtistInfo2`：仅在艺术家有图时返回图片 URL
- `getScanStatus`、`startScan`
- 认证：`u` + `t`/`s`（推荐），兼容 `p` / `p=enc:hex`

## 艺术家图规则

对齐 Navidrome 的实体语义，但去掉所有回退：

- 只读取 `Music/<artist>/artist.*` 中命中 `[artist_art].names` 的文件；
- 不用专辑目录里的图，不用专辑封面代替，不访问外部元数据服务；
- 有图时 `coverArt` = `ar-…`、`artistImageUrl` = 签名公开地址；无图时这两个字段直接省略；
- 图片地址形如 `/share/img/ar-…/<token>?size=600`，可直接取图，URL 中不含登录凭据；`size` 目前只是提示，返回原图；
- `getCoverArt?id=ar-…` 仅在该艺术家有图时返回 200，否则 404。

## 兼容桩与不支持项

返回空结果的兼容桩（避免客户端报错）：

- `getPlaylists`（空）、`getPlayQueue`、`getNowPlaying`、`getGenres`、`getSongsByGenre`、`getStarred2`/`getStarred`
- `scrobble`、`star`/`unstar`、`setRating`：返回成功但**不落库**
- `getAlbumInfo2`、`getTopSongs`、`getSimilarSongs2`、播客/电台/分享/书签：空结构

**不支持**：搜索（`search2`/`search3`）、播放列表创建/修改、收藏/评分持久化、歌词、转码、多用户、多音乐库。搜索框在客户端里会不可用或报错。

## 其他说明

- 目录索引走 **rclone RC**（`operations/list`），音频/封面走 **rclone HTTP + Range**，不需要 FUSE 挂载。
- 元数据来自目录结构、`.cue`、`cover.jpg` 和 `artist.jpg`，不读音频标签，不转码。
- 单用户、单音乐库、SQLite 索引，重点保证浏览、播放、封面和艺术家图。
- JSON 输出按 Feishin 实际使用的字段做了补齐（`artists`/`albumArtists`、`title`、`isDir` 等）；XML 为基础可用版本。

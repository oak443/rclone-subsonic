#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/rclone-subsonic"
command -v rclone >/dev/null || { echo "需要 rclone"; exit 1; }
command -v ffmpeg >/dev/null || { echo "需要 ffmpeg"; exit 1; }
command -v curl >/dev/null || { echo "需要 curl"; exit 1; }
command -v python3 >/dev/null || { echo "需要 python3"; exit 1; }

if [[ ! -x "$BIN" ]]; then
  (cd "$ROOT" && cargo build)
fi

TMP="$(mktemp -d)"
SERVER_PID=""
cleanup() {
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

LIB="$TMP/library"
ALBUM="$LIB/Smoke Artist/Smoke Album"
mkdir -p "$ALBUM"

ffmpeg -v error -f lavfi -i "sine=frequency=440:duration=1" -c:a flac "$ALBUM/01 first.flac"
ffmpeg -v error -f lavfi -i "sine=frequency=660:duration=2" -c:a flac "$ALBUM/02 second.flac"
ffmpeg -v error -f lavfi -i "color=c=blue:s=64x64" -frames:v 1 "$ALBUM/cover.jpg"
cp "$ALBUM/cover.jpg" "$LIB/Smoke Artist/artist.jpg"

cat > "$ALBUM/smoke.cue" <<'EOF'
REM DATE "2024-01-02"
REM GENRE "Smoke"
PERFORMER "Smoke Artist"
TITLE "Smoke Album"
FILE "1 - first.flac" WAVE
  TRACK 01 AUDIO
    TITLE "First Track"
    INDEX 01 00:00:00
FILE "2 - second.flac" WAVE
  TRACK 02 AUDIO
    TITLE "Second Track"
    INDEX 01 00:00:00
EOF

PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"

cat > "$TMP/config.toml" <<EOF
[paths]
db = "$TMP/library.db"
cover_cache = "$TMP/cache"

[server]
bind = "127.0.0.1:$PORT"
music_folder_name = "Music"

[auth]
username = "smoke"
password = "smokepass"
allow_plaintext = true

[storage]
remote = "$LIB"
managed = true

[scan]
on_start = false
EOF

echo "== scan"
"$BIN" scan --config "$TMP/config.toml"

echo "== serve"
"$BIN" serve --config "$TMP/config.toml" >"$TMP/server.log" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 100); do
  if curl -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done
curl -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null

SALT="c0ffee"
TOKEN="$(printf 'smokepass%s' "$SALT" | md5sum | cut -d' ' -f1)"
BASE="http://127.0.0.1:$PORT/rest"
AUTH="u=smoke&t=$TOKEN&s=$SALT&v=1.16.1&c=smoke"

echo "== ping / artists / album"
curl -fsS "$BASE/ping.view?$AUTH&f=json" > "$TMP/ping.json"
curl -fsS "$BASE/getArtists.view?$AUTH&f=json" > "$TMP/artists.json"
curl -fsS "$BASE/getAlbumList2.view?$AUTH&f=json&type=newest&size=10" > "$TMP/albums.json"

ALBUM_ID="$(python3 - "$TMP/albums.json" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))["subsonic-response"]
albums = data["albumList2"]["album"]
assert len(albums) == 1, albums
assert albums[0]["name"] == "Smoke Album", albums[0]
print(albums[0]["id"])
PY
)"

curl -fsS "$BASE/getAlbum.view?$AUTH&f=json&id=$ALBUM_ID" > "$TMP/album.json"
python3 - "$TMP/ping.json" "$TMP/artists.json" "$TMP/album.json" <<'PY'
import json, sys
ping = json.load(open(sys.argv[1]))["subsonic-response"]
assert ping["status"] == "ok", ping
artists = json.load(open(sys.argv[2]))["subsonic-response"]["artists"]
assert artists["index"][0]["artist"][0]["name"] == "Smoke Artist", artists
album = json.load(open(sys.argv[3]))["subsonic-response"]["album"]
assert album["name"] == "Smoke Album", album
songs = album["song"]
assert len(songs) == 2, songs
assert [s["title"] for s in songs] == ["First Track", "Second Track"], songs
assert [s["duration"] for s in songs] == [1, 2], songs
assert songs[0]["contentType"] == "audio/flac", songs[0]
print("    JSON 形状 OK")
PY

SONG_ID="$(python3 - "$TMP/album.json" <<'PY'
import json, sys
album = json.load(open(sys.argv[1]))["subsonic-response"]["album"]
print(album["song"][0]["id"])
PY
)"

echo "== stream range"
STATUS="$(curl -sS -o "$TMP/part.bin" -w '%{http_code}' -H 'Range: bytes=0-99' "$BASE/stream.view?$AUTH&id=$SONG_ID")"
[[ "$STATUS" == "206" ]] || { echo "Range 状态码错误: $STATUS"; exit 1; }
[[ "$(wc -c < "$TMP/part.bin")" == "100" ]] || { echo "Range 长度错误"; exit 1; }

echo "== full stream byte identity"
curl -fsS "$BASE/stream.view?$AUTH&id=$SONG_ID" -o "$TMP/full.flac"
cmp "$TMP/full.flac" "$ALBUM/01 first.flac"

echo "== cover"
curl -fsS "$BASE/getCoverArt.view?$AUTH&id=$ALBUM_ID" -o "$TMP/cover.jpg"
cmp "$TMP/cover.jpg" "$ALBUM/cover.jpg"

echo "== artist art"
ARTIST_ID="$(python3 - "$TMP/artists.json" <<'PY'
import json, sys
artists = json.load(open(sys.argv[1]))["subsonic-response"]["artists"]
print(artists["index"][0]["artist"][0]["id"])
PY
)"
curl -fsS "$BASE/getArtist.view?$AUTH&f=json&id=$ARTIST_ID" > "$TMP/artist.json"
ARTIST_URL="$(python3 - "$TMP/artist.json" "$ARTIST_ID" <<'PY'
import json, sys
artist = json.load(open(sys.argv[1]))["subsonic-response"]["artist"]
assert artist["coverArt"] == sys.argv[2], artist
url = artist["artistImageUrl"]
assert url.startswith("http://127.0.0.1:"), url
assert "/share/img/" in url and "u=" not in url, url
print(url)
PY
)"
curl -fsS "$ARTIST_URL" -o "$TMP/artist.jpg"
cmp "$TMP/artist.jpg" "$LIB/Smoke Artist/artist.jpg"
curl -fsS "$BASE/getArtistInfo.view?$AUTH&f=json&id=$ARTIST_ID" > "$TMP/artist-info.json"
python3 - "$TMP/artist-info.json" <<'PY'
import json, sys
info = json.load(open(sys.argv[1]))["subsonic-response"]["artistInfo"]
for key in ("smallImageUrl", "mediumImageUrl", "largeImageUrl"):
    assert info[key].startswith("http://127.0.0.1:"), info
assert info["smallImageUrl"] != info["largeImageUrl"], info
PY

echo "== artist art removed: no fallback"
rm "$LIB/Smoke Artist/artist.jpg"
"$BIN" scan --config "$TMP/config.toml" >/dev/null
curl -fsS "$BASE/getArtist.view?$AUTH&f=json&id=$ARTIST_ID" > "$TMP/artist-gone.json"
python3 - "$TMP/artist-gone.json" <<'PY'
import json, sys
artist = json.load(open(sys.argv[1]))["subsonic-response"]["artist"]
assert "coverArt" not in artist, artist
assert "artistImageUrl" not in artist, artist
PY
STATUS="$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/getCoverArt.view?$AUTH&id=$ARTIST_ID")"
[[ "$STATUS" == "404" ]] || { echo "artist 无图应返回 404，实际 $STATUS"; exit 1; }
curl -fsS "$BASE/getCoverArt.view?$AUTH&id=$ALBUM_ID" -o /dev/null

echo "== scan status / auth failure"
curl -fsS "$BASE/getScanStatus.view?$AUTH&f=json" > "$TMP/scan.json"
python3 - "$TMP/scan.json" <<'PY'
import json, sys
status = json.load(open(sys.argv[1]))["subsonic-response"]["scanStatus"]
assert status["scanning"] is False, status
assert status["count"] == 2, status
PY
BAD="$(curl -fsS "$BASE/ping.view?u=smoke&t=deadbeef&s=$SALT&c=smoke&f=json")"
python3 - "$BAD" <<'PY'
import json, sys
data = json.loads(sys.argv[1])["subsonic-response"]
assert data["status"] == "failed" and data["error"]["code"] == 40, data
PY

echo
echo "smoke OK"

# RUVID — Rust Video Downloader

`ruvid` is a standalone Rust CLI and library for analyzing and downloading HTTP media files, parallel HTTP range transfers with crash-safe resume checkpoints, HLS VOD (`.m3u8`), DASH VOD (`.mpd`), playlists, subtitles, and emitting RUVI JSON handoff manifests (`.ruvi.json`) for `ruvic`.

## Commands

- `ruvid get <URL>` — Resolve and download media (supports `--jobs`, `--resume`, `--limit-rate`, `--retry`, `--write-manifest`, `--subtitles`)
- `ruvid info <URL>` — Inspect available media streams/variants
- `ruvid playlist <URL>` — Download playlist items (`--items "1-5,8"`, `--concurrency`, `--skip-errors`)
- `ruvid config path|show|validate|init` — Manage `ruvid.toml`
- `ruvid doctor` — Check environment, config, cache, and writable download directory

## Build & Test

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

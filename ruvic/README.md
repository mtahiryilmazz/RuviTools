# RUVIC — Rust Video Converter

`ruvic` is a standalone Rust CLI and library for analyzing, remuxing, transcoding, extracting audio, trimming, thumbnailing, and batch-processing local media files using a supervised `ffmpeg` / `ffprobe` subprocess backend.

## Commands

- `ruvic probe <INPUT>` — Inspect container, streams, and chapters
- `ruvic convert <INPUT> -o <OUTPUT>` — Transcode with explicit codecs or presets
- `ruvic remux <INPUT> -o <OUTPUT>` — Copy streams to another container without re-encoding
- `ruvic audio <INPUT> -o <OUTPUT>` — Extract or convert audio track
- `ruvic trim <INPUT> --start <TIME> (--end <TIME> | --duration <SECS>) -o <OUTPUT>` — Cut media segment
- `ruvic thumbnail <INPUT> --at <TIME> -o <OUTPUT>` — Extract a still frame
- `ruvic batch <INPUT_DIR> -o <OUTPUT_DIR>` — Convert matching media files in a directory
- `ruvic preset list|show|validate` — Manage conversion presets
- `ruvic config path|show|validate|init` — Manage `ruvic.toml`
- `ruvic doctor` — Check `ffmpeg` / `ffprobe` availability and capabilities

## Build & Test

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

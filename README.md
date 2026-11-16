# RUVI — Rust Video Tools

Harici araçlara (`yt-dlp`, `deno`, `ffmpeg`, `ffprobe`) bağımlı olmadan çalışan iki bağımsız saf Rust CLI uygulaması:

- **[`ruvid/`](ruvid/)** — **RUVID (Rust Video Downloader):** Saf Rust YouTube (Innertube + HTML) çözümleyici, Direct HTTP, paralel `206` Range indirme, crash-safe `checkpoint` resume, HLS VOD, DASH VOD, Playlist, Altyazı ve RUVI JSON Handoff Manifest (`schema_version = "1"`).
- **[`ruvic/`](ruvic/)** — **RUVIC (Rust Video Converter):** Saf Rust yerel medya motoru (`NativeMediaBackend`) ile ISO-BMFF/MP4, EBML/MKV/WebM, MP3, FLAC, WAV, AAC, Ogg ve görsel container analizi ve dönüştürme (`probe`, `convert`, `remux`, `audio`, `trim`, `thumbnail`, `batch`, `preset`, `doctor`). İsteğe bağlı `--ffmpeg`/`--ffprobe` desteği de mevcuttur.

## Hızlı Başlangıç

```powershell
# RUVID derleme ve test
cd ruvid
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release

# RUVIC derleme ve test
cd ..\ruvic
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Ayrıntılar için [HANDOFF.md](HANDOFF.md), [ruvid/docs/STATUS.md](ruvid/docs/STATUS.md) ve [ruvic/docs/STATUS.md](ruvic/docs/STATUS.md) dosyalarına bakın.

# RUVID Durum Raporu (`docs/STATUS.md`)

- **Durum:** %100 Saf Rust sürümü tamamlandı ve release derlemesi hazır (`target/release/ruvid.exe`).
- **Bağımsız Dizin:** `d:\mtahiryilmazz\Github\RuviTools\ruvid`
- **Uygulanan Özellikler:**
  - CLI (`get`, `info`, `playlist`, `config`, `doctor`, `--json`, `--quiet`, `-v`, `--no-color`, `--no-progress`, `--allow-private-net`)
  - Harici `yt-dlp` veya `deno` gerektirmeyen **%100 Saf Rust YouTube Çözümleyici** (`ytInitialPlayerResponse` HTML + Innertube `/youtubei/v1/player` Android/iOS istemci profilleri)
  - Direct HTTP/HTTPS indirme, genişletilmiş SSRF özel/loopback/IPv4-mapped IPv6/NAT64 koruması, URL şema allowlist (`http`/`https`), üstel geri çekilmeli retry (`408`/`429`/`5xx`)
  - Paralel HTTP Range (`206 Partial Content` & `Content-Range` doğrulamalı, tek akış fallback'li), `--limit-rate` token-bucket hız sınırlama, crash-safe `.part.ruvid.json` checkpoint resume + `ETag`/`Last-Modified` değişim reddi
  - HLS VOD (`.m3u8` Master + Media playlist, `#EXT-X-MAP`, `#EXT-X-BYTERANGE`, `#EXT-X-KEY` DRM reddi, `--subtitles`)
  - DASH VOD (`.mpd` XML `SegmentTemplate`, `SegmentTimeline`, `SegmentList`, `ContentProtection` DRM reddi)
  - Platform HTML OpenGraph/JSON-LD medya keşif adaptörü, M3U/JSON `playlist` indirme (`--items`, `--concurrency`, `--skip-errors`)
  - Güvenli dosya adı temizliği (path traversal & Windows ayrılmış cihaz isimleri), atomik `.part` -> hedef taşıma, `--write-manifest` v1 JSON handoff ve kapsamlı URL hassas token/kimlik redaksiyonu
- **Testler:** `cargo test` — **17/17** entegrasyon testi geçti (`cli_tests`: 2, `direct_http_tests`: 2, `hls_dash_tests`: 3, `resume_range_tests`: 2, `security_architecture_tests`: 8).
- **Kalite:** `cargo fmt --all -- --check` ve `cargo clippy --all-targets -- -D warnings` hatasız/uyarısız geçti.
- **Canlı Test:** Gerçek YouTube videosu (`jNQXAC9IVRw` / *Me at the zoo*) üzerinde harici araç olmadan `ruvid.exe info` ve `ruvid.exe get` (629,172 byte) + JSON manifest üretimi doğrulandı.

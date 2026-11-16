# RUVIC Durum Raporu (`docs/STATUS.md`)

- **Durum:** Tamamlandı ve release derlemesi hazır (`target/release/ruvic.exe`).
- **Bağımsız Dizin:** `d:\mtahiryilmazz\Github\RuviTools\ruvic`
- **Uygulanan Özellikler:**
  - CLI (`probe`, `convert`, `remux`, `audio`, `trim`, `thumbnail`, `batch`, `preset`, `config`, `doctor`, `--manifest`, `--json`, `--ffmpeg`, `--ffprobe`)
  - Shell string olmadan güvenli `ffmpeg` / `ffprobe` subprocess orkestrasyonu, `-progress pipe:1` ayrıştırıcı, timeout/iptal koruması ve log redaksiyonu
  - Normalize `MediaInfo` (`probe`), container/codec uyumluluk kontrolü (`mp4`, `mkv`, `webm`, `mov`, `ts`, `mp3`, `flac`, `aac`, `wav`, `ogg`, görseller), aynı girdi/çıktı yolu reddi, overwrite koruması
  - Yerleşik preset profilleri (`web-mp4`, `archive-mkv`, `fast-720p`, `audio-mp3`, `audio-flac`) + TOML preset doğrulama
  - `--hwaccel` (`auto`, `nvenc`, `qsv`, `amf`, `videotoolbox`, `none`) yetenek tespiti ve CPU fallback raporlama
  - Geçici dosyaya yazma, işlem sonrası `ffprobe` çıktısı bütünlük doğrulaması ve atomik taşıma
- **Testler:** `cargo test` — 7/7 entegrasyon testi geçti (`cli_tests`: 3, `operations_tests`: 2, `probe_tests`: 2).
- **Kalite:** `cargo fmt --all -- --check` ve `cargo clippy --all-targets -- -D warnings` hatasız/uyarısız geçti.

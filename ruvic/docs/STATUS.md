# RUVIC Durum Raporu (`docs/STATUS.md`)

- **Durum:** %100 Saf Rust native medya motoru tamamlandı, canlı medya ve 50 MB streaming benchmark testlerinden geçti (`target/release/ruvic.exe`, **3,911,680 B / ~3.73 MB**).
- **Bağımsız Dizin:** `d:\mtahiryilmazz\Github\RuviTools\ruvic`
- **Uygulanan Özellikler:**
  - CLI (`probe`, `convert`, `remux`, `audio`, `trim`, `thumbnail`, `batch`, `preset`, `config`, `doctor`, `--manifest`, `--video`, `--audio`, `--json`, isteğe bağlı `--ffmpeg`/`--ffprobe`)
  - **Minimal Bellek Yükü (Streaming I/O):** `std::fs::read` ile tüm dosyayı belleğe almak yerine seek tabanlı `BufReader`/`BufWriter` ve `copy_chunked` (64 KB / 1 MB) ile sınırlı bellekli akış işleme (50.3 MB MP4 remux işleminde yalnızca **3.53 MB** özel RAM)
  - **Harici `ffmpeg`/`ffprobe` gerektirmeyen %100 Saf Rust `NativeMediaBackend` motoru:**
    - ISO-BMFF (`MP4`/`MOV`/`M4A`) `moov`/`mdat` ayrıştırma, `stco` (32-bit) ve `co64` (64-bit) chunk-offset yeniden hesaplamalı faststart remux ve trim
    - Tam örnek tablosu ayrıştırma (`stsc`, `stsz`, `stco`, `co64`, `stts`, `stss`)
    - Gerçek ses demuxing: örnek tablolarından ADTS AAC (`.aac`) çerçeve çıkarımı ve bağımsız M4A (`.m4a`) üretimi
    - Çift girdili DASH remuxing (`multiplex_mp4_dual_input` ve `multiplex_ebml_dual_input`): ayrık video ve ses akışlarını tek MP4 veya WebM konteynerinde birleştirme (tekil veya dual-stream JSON handoff manifest desteğiyle)
    - Küresel 360/VR (`sv3d`, `proj`, EBML `0x7670`) ve HDR renk (`colr`, `mdcv`, `clli`, EBML `0x55B0`) meta verilerini remux ve trim sırasında koruma
  - **Sistem Bütünlüğü (`ExitCode::Integrity = 8`):** `validate_moov_integrity` ile yazım öncesi `stco`/`co64` tablo uzunluğu, version/flags=0, EOF aşımı ve alt kutu sınır denetimi; atomik `.ruvic-tmp` yazımı ve hata anında temizlik
- **Testler:** `cargo test` — **106/106** birim ve entegrasyon testi geçti (`tier1_feature_coverage_tests`: 25, `tier2_boundary_corner_tests`: 25, `native_engine_tests`: 8, `security_architecture_tests`: 6, `cli_tests`: 3, `operations_tests`: 2, `probe_tests`: 2, birim testleri: 35).
- **Kalite:** `cargo fmt --all -- --check` ve `cargo clippy --all-targets -- -D warnings` sıfır hata/uyarı ile geçti.
- **Canlı Test & Benchmark:** Gerçek YouTube MP4 akışları ve 50.3 MB MP4 üzerinde `probe` (17.08 ms), `remux` (42.36 ms, ~1,187 MB/sn), `trim` (23.78 ms) ve `audio` ADTS AAC / M4A çıkarımı (25.38 ms) doğrulandı.

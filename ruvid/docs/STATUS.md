# RUVID Durum Raporu (`docs/STATUS.md`)

- **Durum:** %100 Saf Rust sürümü tamamlandı, canlı YouTube ve benchmark testlerinden geçti (`target/release/ruvid.exe`, **7,208,448 B / ~6.87 MB**).
- **Bağımsız Dizin:** `d:\mtahiryilmazz\Github\RuviTools\ruvid`
- **Uygulanan Özellikler:**
  - CLI (`get`, `info`, `playlist`, `config`, `doctor`, `--json`, `--quiet`, `-v`, `--no-color`, `--no-progress`, `--allow-private-net`)
  - Harici `yt-dlp` veya `deno` gerektirmeyen **%100 Saf Rust YouTube Çözümleyici** (`ytInitialPlayerResponse` HTML + Innertube `/youtubei/v1/player` Android/iOS istemci profilleri)
  - Dinamik `Range: bytes=0-0` yoklaması ile `total_bytes` bilinmeyen akışları (ör. YouTube `itag-18`) paralel `--jobs` parça indirmeye yükseltme ve ilerleme çubuğunu (`set_total`) dinamik güncelleme
  - DASH 4K/8K/60fps ayrık video + ses akışı seçimi (`select_stream_pair_with_lang`), paralel çift akış indirme, otomatik `ruvic remux` birleştirme ve `DualStreamHandoff` v1 JSON manifest üretimi
  - Video bölümleri (`chapters`) ve önemli anları (`DESCRIPTION_CHAPTERS`, `MACRO_MARKERS`, açıklama zaman damgaları) çözümleme; `--split-chapters` ile her bölümü başlığıyla ayrı dosyaya bölme
  - Klip ve zaman aralığı indirme (`--clip`, `--start-time`, `--end-time`, `--duration`)
  - Çoklu altyazı (`--subtitles`, `--subtitles-lang`, `--auto-subs`) ve XML timedtext / WebVTT → `.vtt`, `.srt`, `.sbv` dönüştürme (`--sub-format`)
  - Çoklu dil / dublaj ses parçası (`audioTrack`) tespiti ve `--audio-lang` seçimi
  - YouTube oynatma listeleri (`ytInitialData`), RSS/Atom podcast beslemeleri ve M3U/JSON kataloglarını toplu indirme (`playlist --items --concurrency --skip-errors`)
  - Genişletilmiş SSRF koruması, atomik `.part` yazımı, crash-safe `.part.ruvid.json` resume kontrol noktası ve URL hassas parametre redaksiyonu
- **Testler:** `cargo test` — **103/103** birim ve entegrasyon testi geçti (`m1_parallel_dash_tests`: 7, `tier1_feature_coverage_tests`: 35, `tier2_boundary_corner_tests`: 35, `security_architecture_tests`: 8, `hls_dash_tests`: 3, `resume_range_tests`: 3, `direct_http_tests`: 2, `cli_tests`: 2, birim testleri: 8).
- **Kalite:** `cargo fmt --all -- --check` ve `cargo clippy --all-targets -- -D warnings` sıfır hata/uyarı ile geçti.
- **Canlı Test & Benchmark:** Gerçek YouTube videosu (`jNQXAC9IVRw`) üzerinde `--jobs 4` paralel Range indirme (8 parça, 1.88 sn), DASH ayrık video+ses (`itag-133+itag-140`, 16 parça + otomatik remux, 1.54 sn) ve SRT altyazı dönüşümü **~10.7 MB** özel RAM kullanımıyla doğrulandı.

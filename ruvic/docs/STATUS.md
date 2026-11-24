# RUVIC Durum Raporu (`docs/STATUS.md`)

- **Sürüm:** `0.2.0` (bkz. `ruvic/Cargo.toml`)
- **Durum:** %100 Saf Rust native medya motoru; harici `ffmpeg`/`ffprobe`/`yt-dlp` bağımlılığı yok.
  Release ikili: `target/release/ruvic.exe`, **5,716,480 B / ~5.45 MB** (strip + fat LTO).
- **Depo:** <https://github.com/mtahiryilmazz/RuviTools> — `ruvic/` · **Lisans:** MIT
- **Rust:** `edition = "2024"`, `rust-version = "1.85"` (bu makinede derleme: rustc 1.100.0)

## 0.2.0 ile gelenler

| Değişiklik | Nerede | Etkisi |
| :--- | :--- | :--- |
| Taşma-güvenli tam sayı zaman ölçekleme | `src/backend/native/timescale.rs` (yeni) | `mvhd`/`mdhd`/`stts`/`elst`/EBML dönüşümleri `f64` yerine `u128`/`i128`; 10+ saatlik dosyalarda ve `30000/1001` tabanında kayma yok |
| `dual_mux.rs` f64 temizliği | `dual_mux.rs` | `tkhd`/`mdhd` süreleri, DASH parça sıralaması, kesit sınırları ve WebM tick hesabı tam sayı; `stts`/`trun` toplamı `u32` taşması yapamaz |
| Diferansiyel "oracle" test paketi | `tests/differential_ffmpeg_tests.rs` (yeni) | 5 vaka, gerçek `ffmpeg`/`ffprobe` referansı; **varsayılan kapalı** (`RUVIC_DIFF=1`), üretim ikilisine girmez |
| Criterion mikro-benchmark'ları | `benches/native_engine.rs` (yeni) | `probe` / `remux` / `trim` / `timescale` motor içi ölçüm |
| CI: `differential-ffmpeg` işi | `.github/workflows/ci.yml` | Ubuntu'da ffmpeg kurup oracle paketini çalıştırır, benchmark hedeflerini derler |
| Paket meta verisi | `Cargo.toml` | `repository`, `homepage`, `documentation`, `keywords`, `categories` |

## Uygulanan Özellikler

- **CLI:** `probe`, `convert`, `remux`, `audio`, `trim`, `thumbnail`, `batch`, `preset`, `config`, `doctor`,
  `--manifest`, `--video`, `--audio`, `--json`; isteğe bağlı `--ffmpeg`/`--ffprobe` (varsayılan kapalı).
- **ISO-BMFF (`MP4`/`MOV`/`M4A`/`fMP4`):** `moov`/`mdat` ayrıştırma, `stco` (32-bit) ve `co64` (64-bit)
  offset yeniden hesaplamalı faststart remux, tam örnek tablosu (`stsc`, `stsz`, `stco`, `co64`, `stts`, `stss`).
- **DASH defragmenter:** `multiplex_mp4_dual_input` / `try_mux_fmp4` — ayrı video ve ses `moof+mdat`
  akışlarını tek standart Faststart MP4'te birleştirir (tekil/çift akış JSON handoff manifest desteği).
- **WebM/Matroska (EBML):** küme birleştirme, `TimestampScale` tabanlı zaman hesabı, kesit kırpma.
- **Ses:** ADTS AAC ve M4A demux, Symphonia + `rusty_mp3` (320k/VBR MP3) ve 16-bit WAV kodlama,
  ID3v2.3 (`APIC`) ve iTunes (`covr`) kapak/meta veri gömme.
- **Meta veri koruma:** HDR10 (`colr`, `mdcv`, `clli`), 360°/VR (`svil`, `proj`, EBML `0x7670`).
- **Streaming I/O:** 64 KB / 1 MB `BufReader`/`BufWriter` ve `copy_chunked` (50 MB remux'ta ~3.5 MB özel RAM).
- **Bütünlük:** `validate_moov_integrity` (yazma öncesi `stco`/`co64` uzunluğu, version/flags=0, EOF aşımı,
  alt kutu sınırı → `ExitCode::Integrity = 8`), atomik `.ruvic-tmp` yazımı ve hata anında temizlik.

## Testler

| Paket | Test | Durum |
| :--- | :--- | :--- |
| Birim testleri (`src/`) | 11 (8'i `timescale` modülü) | ✅ |
| `tier1_feature_coverage_tests` | 25 | ✅ |
| `tier2_boundary_corner_tests` | 25 | ✅ |
| `native_engine_tests` | 9 | ✅ |
| `security_architecture_tests` | 6 | ✅ |
| `cli_tests` | 4 | ✅ |
| `probe_tests` | 2 | ✅ |
| `operations_tests` | 2 | ✅ |
| **Hermetik toplam** | **84** | ✅ (`cargo test`, dış program gerekmez) |
| `differential_ffmpeg_tests` (oracle) | 5 | ✅ `RUVIC_DIFF=1` ile; varsayılan atlanır |

- **Kalite:** `cargo fmt --all -- --check` ve `cargo clippy --all-targets -- -D warnings` sıfır hata/uyarı.
- **Benchmark:** `cargo bench --bench native_engine` — `probe` 41.7 µs, 8 MiB remux 714.6 µs,
  zaman ölçekleme tick başına ~2.6 ns (ayrıntı: `BENCHMARK_REPORT.md` §9).

## Bağımlılık Politikası

`ruvic` çalışma zamanında hiçbir dış ikili çağırmaz; `ffmpeg`/`ffprobe` yalnızca CI'daki oracle
testlerinde referans olarak kullanılır (`docs/DIFFERENTIAL_TESTING.md`). Kullanıcı isterse
`--ffmpeg`/`--ffprobe` ile harici arka ucu elle seçebilir; bu bir varsayılan değil, bilinçli bir tercihtir.

# RuviTools — Devam Dosyası (RESUME)

> Son güncelleme: 2026-11-16 13:15 (+03). Yeni bir ajan/oturum bu dosyayı okuyarak kaldığı yerden devam etmelidir.
> Son commit: `2befa50` (origin/main). Aşağıdaki tüm değişiklikler **commit edilmemiş** çalışma ağacındadır.

## 1. Proje Amacı
`ruvid` (indirici) ve `ruvic` (dönüştürücü) — harici `yt-dlp`, `deno`, `ffmpeg`, `ffprobe` bağımlılığı olmadan **saf Rust** CLI araçları.
Ana ilkeler: **minimal bellek yükü (streaming / bounded buffer), hız, sistem bütünlüğü (atomik yazım, bounds check)**.

Referans mimari: `RUVI_MASTER_ARCHITECTURE.md`, `RUVID_ARCHITECTURE.md`, `RUVIC_ARCHITECTURE.md`, `HANDOFF.md`.

## 2. Onaylanmış Kapsam (Gereksinimler R1–R7 + Benchmark)
- **R1 ruvid**: `Range: bytes=0-0` yoklamasıyla boyutu bilinmeyen akışları paralel `--jobs` indirmeye yükseltme; DASH ayrık video+ses (H.264/VP9/AV1, AAC/Opus, 4K/8K/60fps); playlist / dizi / podcast toplu indirme.
- **R2 ruvid**: Chapters / key moments başlıklarıyla ayrı dosyalara bölme; klip/zaman aralığı indirme; çoklu altyazı (.srt/.vtt/.sbv) ve çoklu dil/dublaj ses parçaları.
- **R3 ruvic**: Saf Rust MP4 `moov`/`mdat` + `stco`/`co64` rebasing ile faststart/remux/trim; MP4/WebM demux/mux; ADTS AAC / M4A çıkarımı; 360/VR (`sv3d`) ve HDR (`colr`/`mdcv`/`clli`) meta veri koruma.
- **R4**: Streaming I/O (`std::fs::read` ile tüm dosyayı RAM'e alma yok), atomik temp→rename, hata anında temizlik, sıkı bounds kontrolü.
- **R5**: Her iki projede `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release` sıfır hata/uyarı.
- **R6**: Canlı kısa herkese açık YouTube testi (`--jobs 4`), DASH birleştirme, altyazı/chapter, ruvic uçtan uca.
- **R7**: `HANDOFF.md`, `ruvid/docs/STATUS.md`, `ruvic/docs/STATUS.md` güncelle; commit + `git push origin main`.
- **M6 Benchmark** (en sonda): piyasa araçlarıyla kıyas — indirme süresi/throughput, dönüştürme süresi, RAM (`PeakWorkingSet64`), binary boyutu, cold-start. Sonuçlar `BENCHMARK.md` + `HANDOFF.md`. Ölçülmeyen sayı uydurulmaz; ölçülen ile referans değer ayrı belirtilir. Harici araçlar makinede kurulu değil (kaldırıldı).

## 3. Mevcut Durum (13:20 itibarıyla doğrulandı — CHECKPOINT COMMIT edildi)

**Her iki projede**: `cargo fmt --check` ✅, `cargo clippy --all-targets -- -D warnings` ✅, `cargo test` ✅ (tüm testler), `cargo build --release` ✅.
Release boyutları: `ruvid.exe` = 7,046,144 B (~6.7 MB), `ruvic.exe` = 3,911,680 B (~3.7 MB).

Bu oturumda düzeltilenler:
- `ruvic/src/domain/media_info.rs`, `cli/commands.rs`: manifest dual-stream şeması (`primary_input`/`secondary_input`, `output` opsiyonel, `resolve_inputs`).
- `ruvic/src/backend/native/mp4_atoms.rs`: `validate_moov_integrity` — kutu boyutu ebeveyn sınırı, `stsd` sample-entry alt kutuları, `stco`/`co64` version/flags=0, girdi sayısı = gövde uzunluğu, offset < dosya boyutu → exit 8.
- `ruvic/src/backend/native/processor.rs`: yazmadan önce yalnızca `moov` okunarak doğrulama (`verify_source_integrity`, 256 MB sınır); trim/remux'ta `colr`/`mdcv`/`clli`/`sv3d` tam kutular olarak korunuyor.
- Clippy düzeltmeleri (`ruvid` selection.rs `?`, handoff.rs, test fikstürleri).

### (Eski) önceki durum notları

### ruvid — `cargo test`: TÜM TESTLER GEÇİYOR ✅
Yapılanlar (Teamwork ekibi, commit edilmedi):
- `downloader/http_range.rs`: dinamik `Range: bytes=0-0` yoklaması ile paralel yükseltme.
- `domain/selection.rs`: `select_stream_pair` (DASH video+ses eşleme).
- `assembly/handoff.rs`: `DualStreamHandoff` manifest üretimi.
- `resolver/platform/youtube.rs`: ayrık DASH formatlarını çözümleme.
- Yeni testler: `tests/m1_parallel_dash_tests.rs`, `tests/tier1_feature_coverage_tests.rs`, `tests/tier2_boundary_corner_tests.rs`, `tests/common/synthetic_media.rs`.

### ruvic — `cargo test`: 4 test BAŞARISIZ ❌ (tier2)
Yapılanlar: `backend/native/mp4_atoms.rs` (stsc/stsz/stco/co64/stss, sv3d/colr/mdcv/clli), `ebml.rs` (WebM), `container_probe.rs`, `ffmpeg/process.rs` OnceCell yetenek önbelleği, `operations/mod.rs` gereksiz capability taramasını atlama.
Bu oturumda düzeltilen: `domain/media_info.rs` + `cli/commands.rs` — manifest artık `primary_input`/`secondary_input` (dual-stream) şemasını kabul ediyor (`output` opsiyonel; `resolve_inputs`). → `test_dual_stream_manifest_remux` geçti.

Kalan başarısız testler (`ruvic/tests/tier2_boundary_corner_tests.rs`):
| Test | Satır | Beklenen | Sorun |
|---|---|---|---|
| `test_truncated_stco_fails_integrity` | 123 | exit 8 | remux bozuk `stco` dosyasını kabul edip 0 dönüyor |
| `test_stco_offset_points_past_eof` | 157 | exit 8 | chunk offset EOF ötesini gösterince reddedilmiyor |
| `test_truncated_sv3d_box_fails_integrity` | 582 | exit 8 | kesik `sv3d` kutusu reddedilmiyor |
| `test_trim_preserves_hdr_vr_metadata` | 652 | `colr` korunmalı | trim çıktısında `colr` kayboluyor |

**Yapılacak düzeltme**: `backend/native/processor.rs` remux/trim yolunda yazmadan önce bütünlük doğrulaması (stco/co64 tablo uzunluğu ve her offset < dosya boyutu; alt kutu boyutları ebeveyn sınırında) → `RuviError` ile exit code 8 (integrity). Trim'de `stsd` içindeki görsel örnek girdisinin alt kutularını (`colr`, `mdcv`, `clli`, `sv3d`/`st3d`) koru.

## 4. Kalan İş Listesi (sırayla)
1. [x] ruvic tier2 4 testi düzelt.
2. [x] R4: processor zaten `File` + `BufWriter` + `copy_chunked` ile streaming; yalnızca `moov` belleğe alınıyor. (Probe tarafında `container_probe.rs` tüm dosyayı okuyor mu — kontrol et.)
3. [ ] R2: ruvid chapters/klip/altyazı/çoklu ses — testler (tier1/tier2) mevcut ve geçiyor; CLI'den uçtan uca kullanımı canlı doğrula.
4. [ ] R1: playlist / dizi / podcast toplu indirme doğrulaması.
5. [ ] R5: her iki projede fmt + clippy `-D warnings` + test + `build --release`.
6. [ ] R6: canlı YouTube `--jobs 4` + DASH remux uçtan uca (örnek: `jNQXAC9IVRw`).
7. [ ] M6: `BENCHMARK.md` — `Measure-Command` ile süre, `Get-Process` `PeakWorkingSet64` ile RAM, `(Get-Item target/release/*.exe).Length` ile boyut.
8. [ ] R7: dokümanlar + `git add -A; git commit; git push origin main`.

## 5. Doğrulama Komutları
```powershell
cd d:\mtahiryilmazz\Github\RuviTools\ruvid; cargo fmt --all -- --check; cargo clippy --all-targets -- -D warnings; cargo test; cargo build --release
cd d:\mtahiryilmazz\Github\RuviTools\ruvic; cargo fmt --all -- --check; cargo clippy --all-targets -- -D warnings; cargo test; cargo build --release
```

## 6. Notlar / Kurallar
- Testlerde `MockHttpServer` ayrı `std::thread` Tokio runtime üzerinde koşmalı (current-thread deadlock önlemi; `ruvid/tests/common/mod.rs`).
- YouTube: Innertube `ANDROID` (20.10.38) + watch HTML `VISITOR_DATA`; itag-18 `contentLength` vermez → Range yoklaması bu yüzden var.
- Token/çerez/URL imzaları loglanmaz (`redact_sensitive_url`). DRM/erişim kontrolü atlatılmaz.
- Teamwork ekip koşusu (`2594c3aa-...`) kota (429) nedeniyle durdu; ekip ara dosyaları: `C:\Users\BT\.gemini\antigravity\scratch\teamwork\2594c3aa-94e9-4658-bf30-70227f50038a\` (PROJECT.md, ORIGINAL_REQUEST.md, TEST_INFRA.md olabilir).

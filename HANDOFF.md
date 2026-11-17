# RuviTools — Proje Durumu ve Handoff Notu

Son güncelleme: 2026-11-16

## 1. Mimari İlkeler ve Mevcut Durum

Her iki uygulama da harici araçlara (`yt-dlp`, `deno`, `ffmpeg`, `ffprobe`) bağımlı olmayan **%100 saf Rust** çekirdeği ile **"minimal bellek yükü, yüksek hız ve sistem bütünlüğü"** ilkeleri temelinde tamamlandı, uçtan uca test edildi ve release `.exe` çıktıları üretildi:

- **`ruvid/` (`d:\mtahiryilmazz\Github\RuviTools\ruvid`) — RUVID (Rust Video Downloader)**
  - Release ikilisi: `ruvid\target\release\ruvid.exe` (**7,350,784 B / ~7.01 MB**)
  - Kalite Kapıları: `cargo fmt --all -- --check` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo test` (**103/103** birim ve entegrasyon testi geçti) ✅
  - Yetenekler:
    - Dinamik `Range: bytes=0-0` yoklaması ile `contentLength` bilinmeyen akışları (ör. YouTube `itag-18`) otomatik olarak paralel `--jobs` parça indirmeye yükseltme ve ilerleme çubuğunu (`set_total`) dinamik güncelleme.
    - DASH ayrık video + ses akışlarını (H.264/VP9/AV1 + AAC/Opus; 4K/8K ve 60fps dahil) tespit etme (`select_stream_pair_with_lang`), paralel çift akış indirme ve otomatik `ruvic remux` birleştirme + `DualStreamHandoff` manifest üretimi.
    - Video bölümleri (chapters) ve önemli anları (`DESCRIPTION_CHAPTERS`, `MACRO_MARKERS` ve açıklama zaman damgaları) çözümleme; `--split-chapters` ile her bölümü başlığıyla ayrı dosyaya ayırma.
    - Klip ve zaman aralığı indirme (`--clip`, `--start-time`, `--end-time`, `--duration`).
    - Çoklu altyazı (`--subtitles`, `--subtitles-lang`, `--auto-subs`) ve XML timedtext / WebVTT → `.vtt`, `.srt`, `.sbv` dönüştürme (`--sub-format`).
    - Çoklu dil / dublaj ses parçalarını (`audioTrack`) çözümleme ve `--audio-lang` ile seçme.
    - YouTube oynatma listeleri (`ytInitialData`), RSS/Atom podcast beslemeleri ve M3U/JSON kataloglarını toplu kuyrukla indirme (`playlist`).

- **`ruvic/` (`d:\mtahiryilmazz\Github\RuviTools\ruvic`) — RUVIC (Rust Video Converter & Native Media Engine)**
  - Release ikilisi: `ruvic\target\release\ruvic.exe` (**3,662,848 B / ~3.49 MB**)
  - Kalite Kapıları: `cargo fmt --all -- --check` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo test` (**106/106** birim ve entegrasyon testi geçti) ✅
  - Yetenekler:
    - **Minimal Bellek Yükü (Streaming I/O):** Medya dosyalarının tamamını RAM'e yükleyen `std::fs::read` yerine seek tabanlı `BufReader` / `BufWriter` ve `copy_chunked` (64 KB okuma / 1 MB yazma tamponu) mimarisi (50.3 MB MP4 remux işleminde yalnızca **3.53 MB** özel RAM kullanımı).
    - **Saf Rust MP4 & WebM Motoru:** Gerçek `moov`/`mdat` ayrıştırma, `stco` (32-bit) ve `co64` (64-bit) chunk-offset yeniden hesaplamalı faststart/remux/trim, `stsc`/`stsz`/`stco`/`stts`/`stss` örnek tablosu ayrıştırma.
    - **Gerçek Ses Demuxing & Çıktı:** `stsc`/`stsz`/`stco` örnek sınırlarından ADTS AAC (`.aac`) ve bağımsız M4A (`.m4a`) çıkarımı.
    - **Çift Girdili DASH Muxing:** Ayrık video ve ses dosyalarını (`--video` + `--audio` veya `--manifest`) tek MP4 veya WebM konteynerinde senkronize birleştirme (`multiplex_mp4_dual_input` / `multiplex_ebml_dual_input`).
    - **360/VR & HDR Meta Veri Koruma:** ISO-BMFF (`sv3d`, `proj`, `colr`, `mdcv`, `clli`) ve WebM EBML (`0x55B0` Colour, `0x7670` Projection) kutularını remux ve trim sırasında koruma.
    - **Sıkı Sistem Bütünlüğü (`ExitCode::Integrity = 8`):** `validate_moov_integrity` ile yazım öncesi `stco`/`co64` tablo boyutu, version/flags=0, EOF sınır aşımı ve alt kutu taşmalarını denetleyip bozuk girdileri reddetme; atomik geçici dosya yazımı ve hata anında temizlik.

- **`ruvi-gui/` (`d:\mtahiryilmazz\Github\RuviTools\ruvi-gui`) — RuviStudio Masaüstü GUI (`egui` / `eframe`)**
  - Release ikilisi: `ruvi-gui\target\release\ruvi-studio.exe` (**9,948,160 B / ~9.49 MB**)
  - Kalite Kapıları: `cargo fmt --all -- --check` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo test` (**2/2** entegrasyon testi geçti) ✅
  - Mimari: `ruvid` ve `ruvic` crate'lerini alt süreç başlatmadan doğrudan Rust kütüphanesi olarak bağlar; 4 sekmeli karanlık tema stüdyo arayüzü (**1. Video İndirici**, **2. Medya Stüdyosu (`ruvic` Native Motor)**, **3. Playlist & Podcast Kuyruğu**, **4. Performans & Bellek Monitörü**) sunar.

## 2. Performans ve Piyasa Kıyaslama Özeti (`BENCHMARK.md`)

Detaylı ölçümler [BENCHMARK.md](file:///d:/mtahiryilmazz/Github/RuviTools/BENCHMARK.md) dosyasındadır:

| Metrik | RuviTools (`ruvid` + `ruvic`) | Piyasa Referansı (`yt-dlp` + `deno` + `ffmpeg`) |
|---|---|---|
| **Toplam Uygulama Boyutu** | **10.50 MB** (`ruvid.exe` 7.01 MB + `ruvic.exe` 3.49 MB) | **~300 – 428 MB** |
| **Soğuk Başlangıç (`--version`)** | `ruvid`: **50.23 ms** · `ruvic`: **30.95 ms** | `yt-dlp`: ~450 – 1,200 ms |
| **Canlı YouTube İndirme (`--jobs 4`)** | **1.88 sn** (8 Range parçası) · **10.74 MB** özel RAM | Harici Python + Deno süreci |
| **Canlı YouTube DASH Video+Ses + Otomatik Remux** | **1.54 sn** (16 Range parçası + `ruvic remux`) · **10.83 MB** özel RAM | `yt-dlp` + `ffmpeg` alt süreç birleştirme |
| **50.3 MB MP4 Faststart Remux** | **42.36 ms** (**~1,187 MB/sn**) · **3.53 MB** özel RAM (`8.98 MB` WS) | Harici `ffmpeg -c copy -movflags +faststart` |

## 3. Çalıştırma Örnekleri

```powershell
# 1. YouTube akışlarını, altyazıları ve bölümleri (chapters) incele
.\ruvid\target\release\ruvid.exe info "https://www.youtube.com/watch?v=jNQXAC9IVRw" --all-formats --json

# 2. Paralel Range (--jobs 4) + SRT altyazı + RUVI handoff manifest ile indir
.\ruvid\target\release\ruvid.exe get "https://www.youtube.com/watch?v=jNQXAC9IVRw" --jobs 4 --subtitles --sub-format srt --write-manifest ".\yt.ruvi.json" -o ".\yt.mp4" --json

# 3. DASH ayrık video+ses indir ve otomatik ruvic ile birleştir
.\ruvid\target\release\ruvid.exe get "https://www.youtube.com/watch?v=jNQXAC9IVRw" -f itag-133 --jobs 4 -o ".\yt_dash.mp4" --json

# 4. Bölümlere (chapters) göre ayır veya belirli klip aralığını indir
.\ruvid\target\release\ruvid.exe get "https://www.youtube.com/watch?v=jNQXAC9IVRw" --clip "00:00:02-00:00:07" -o ".\yt_clip.mp4" --json

# 5. RUVIC ile saf Rust probe, faststart remux, trim ve ADTS AAC / M4A ses çıkarımı
.\ruvic\target\release\ruvic.exe probe ".\yt.mp4" --json
.\ruvic\target\release\ruvic.exe remux ".\yt.mp4" -o ".\yt_faststart.mp4" --overwrite --json
.\ruvic\target\release\ruvic.exe trim ".\yt.mp4" --start 00:00:02 --duration 5 -o ".\yt_trim.mp4" --overwrite --json
.\ruvic\target\release\ruvic.exe audio ".\yt.mp4" -o ".\yt.aac" --overwrite --json
.\ruvic\target\release\ruvic.exe audio ".\yt.mp4" -o ".\yt.m4a" --overwrite --json
```

---

## 4. Son Oturum Degisiklikleri (2026-11-17)

- **Saf Rust TLS (rustls) gecisi tamamlandi:** `ruvid/Cargo.toml` icinde `reqwest` artik `default-features = false` + `rustls-tls` kullaniyor (varsayilan `charset`, `http2`, `system-proxy` ozellikleri acikca korundu). Boylece `default-tls -> native-tls -> openssl-sys` native C zinciri tamamen kaldirildi.
- **`deny.toml` sikiestirildi:** gecici `openssl-sys` wrapper istisnasi silindi; `openssl-sys`, `native-tls` ve `schannel` artik kosulsuz `deny`.
- **`scripts/verify-all.ps1`:** TLS denetimi artik bilgi notu degil, sert kapi (native TLS/OpenSSL crates -> FAIL). Ayrica NUL ile dolgulanmis / bozuk `Cargo.lock` dosyalarini tespit ediyor.
- **Onemli onarim:** `ruvi-native-host/Cargo.lock` depoya **733 adet sondaki NUL bayti** ile commit edilmisti; bu crate'te her `cargo` komutu TOML parse hatasi veriyordu. Dosya kirpildi ve yeniden uretildi.
- **`ruvi-gui` (Win11 cilasi):** `src/win11.rs` eklendi - DWM ile koyu baslik cubugu (`DWMWA_USE_IMMERSIVE_DARK_MODE`), yuvarlatilmis kose (`DWMWCP_ROUND`) ve Mica arka plan (`DWMSBT_MAINWINDOW`); tema degisiminde yeniden uygulanir. Sol menu hover/secim gecisleri Fluent tarzi yumusak animasyonlu (120 ms / 180 ms) hale getirildi. `--features mimalloc` opt-in global allocator `main.rs`'e eklendi.
- **Release profil optimizasyonu:** 4 crate'e `[profile.release] strip = true, lto = "fat", codegen-units = 1` eklendi. rustls/ring/webpki-roots ek yuku boylece geri kazanildi: `ruvid.exe` (LTO oncesi 8.62 MB) -> **7.01 MB**, yani rustls gecisi oncesine gore yalnizca **+0.14 MB**; `ruvic.exe` ise **0.24 MB daha kucuk** (3.49 MB).
- **Olculen guncel ikili boyutlari:** `ruvid.exe` 7,350,784 B / 7.01 MB - `ruvic.exe` 3,662,848 B / 3.49 MB - `NativeHost.exe` 5,125,632 B / 4.89 MB - `ruvi-studio.exe` 9,948,160 B / 9.49 MB. CLI toplami **~10.50 MB** (`BENCHMARK.md`, `BENCHMARK_REPORT.md`, `docs/ROADMAP_COMPARISON.md` ve SVG panosu bu degerlere gore guncellendi).
- **Dogrulama:** `scripts/verify-all.ps1` (4 crate x fmt + clippy `-D warnings` + test, lockfile & TLS denetimi) tamamen **PASS**. Canli HTTPS testi: `ruvid info "https://www.youtube.com/watch?v=jNQXAC9IVRw"` rustls uzerinden basarili.
- **Dikkat:** `BENCHMARK.md`, `HANDOFF.md`, `BENCHMARK_REPORT.md` ve `docs/ROADMAP_COMPARISON.md` dosyalari **UTF-8 degil** (Turkce metin codepage-1254). Bu dosyalari duzenlerken byte-koruyucu bir yol kullanin, aksi halde normal metin edit'leri sessizce eslesmez veya dosyayi bozar.
- **Kapsam disi birakilan:** depoda `antigravity-add-model/` adinda, kendi `.git` deposu ve `node_modules`'u olan ilgisiz bir kopya var; `.gitignore`'a eklendi.
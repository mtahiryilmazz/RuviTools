# RuviTools — Proje Durumu ve Handoff Notu

Son güncelleme: 2026-11-16

## Mevcut Durum

Her iki uygulama da harici araçlara (`yt-dlp`, `deno`, `ffmpeg`, `ffprobe`) bağımlı olmayan **%100 saf Rust** çekirdeği ile iki ayrı bağımsız klasörde geliştirildi, alt ajanlarla güvenlik/mimari/çalışma testlerinden geçirildi ve release `.exe` çıktıları üretildi:

- `ruvid/` (`d:\mtahiryilmazz\Github\RuviTools\ruvid`) — **RUVID (Rust Video Downloader)**
  - Release ikilisi: `ruvid\target\release\ruvid.exe`
  - Doğrulama: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (**17/17** entegrasyon testi geçti)
  - Saf Rust YouTube çözümleyici: `ytInitialPlayerResponse` HTML + YouTube Innertube `/youtubei/v1/player` (Android/iOS istemci profilleri), Direct HTTP/Range/Resume, HLS VOD, DASH VOD, Playlist, Altyazı ve v1 JSON Handoff Manifest.
- `ruvic/` (`d:\mtahiryilmazz\Github\RuviTools\ruvic`) — **RUVIC (Rust Video Converter)**
  - Release ikilisi: `ruvic\target\release\ruvic.exe`
  - Doğrulama: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (**21/21** entegrasyon testi geçti)
  - Saf Rust medya motoru (`NativeMediaBackend`): ISO-BMFF (MP4/MOV/M4A), EBML (MKV/WebM), MP3, FLAC, WAV, AAC ADTS, Ogg, MPEG-TS ve JPEG/PNG/WebP ikili ayrıştırma ve container/atom oluşturma; istenirse `--ffmpeg`/`--ffprobe` ile harici FFmpeg backend desteği de korunur.

## Önemli Not (Saf Rust Motor Kapsamı)

- `ruvic`'in yerleşik `NativeMediaBackend` motoru harici `ffmpeg`/`ffprobe` gerektirmeden container/atom analizi (`probe`), container paketleme/yeniden yapılandırma (`remux`, `convert`, `audio`, `trim`, `thumbnail`, `batch`) ve bütünlük doğrulamasını saf Rust ile gerçekleştirir.
- Kare bazlı tam video/ses codec yeniden kodlaması (ör. H.264 <-> VP9 piksel/örnek düzeyinde transcode) ileriki fazda saf Rust codec kütüphaneleriyle genişletilmek üzere modüler `MediaBackend` trait'i arkasında izole edilmiştir; bu arada `--ffmpeg`/`--ffprobe` bayraklarıyla harici encoder da bağlanabilir.

## Çalıştırma Örnekleri

```powershell
# YouTube bilgisini ve akışlarını listele
.\ruvid\target\release\ruvid.exe info "https://www.youtube.com/watch?v=jNQXAC9IVRw" --json

# İndir ve RUVI manifest üret
.\ruvid\target\release\ruvid.exe get "https://www.youtube.com/watch?v=jNQXAC9IVRw" -o ".\yt.mp4" --write-manifest ".\yt.ruvi.json" --json

# RUVIC ile analiz et, web-mp4, mkv, mp3, trim ve thumbnail üret
.\ruvic\target\release\ruvic.exe probe --manifest ".\yt.ruvi.json" --json
.\ruvic\target\release\ruvic.exe convert --manifest ".\yt.ruvi.json" --preset web-mp4 -o ".\yt_web.mp4" --json
.\ruvic\target\release\ruvic.exe remux ".\yt.mp4" -o ".\yt.mkv" --json
.\ruvic\target\release\ruvic.exe audio ".\yt.mp4" --to mp3 -o ".\yt.mp3" --json
.\ruvic\target\release\ruvic.exe trim ".\yt.mp4" --start 00:00:02 --duration 5 -o ".\yt_trim.mp4" --json
.\ruvic\target\release\ruvic.exe thumbnail ".\yt.mp4" --at 00:00:03 -o ".\yt_thumb.jpg" --json
```

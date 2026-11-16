# RuviTools — Proje Durumu ve Handoff Notu

Son güncelleme: 2026-11-16

## Mevcut Durum

Proje, isteğiniz doğrultusunda **iki tamamen bağımsız Rust projesi** olarak yapılandırıldı, test edildi ve release `.exe` ikilileri hazırlandı:

- `ruvid/` (`d:\mtahiryilmazz\Github\RuviTools\ruvid`) — **RUVID (Rust Video Downloader)**
  - Release ikilisi: `ruvid\target\release\ruvid.exe`
  - Doğrulama: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (9/9 entegrasyon testi geçti)
- `ruvic/` (`d:\mtahiryilmazz\Github\RuviTools\ruvic`) — **RUVIC (Rust Video Converter)**
  - Release ikilisi: `ruvic\target\release\ruvic.exe`
  - Doğrulama: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (7/7 entegrasyon testi geçti)

## Mimari Uyum ve Entegrasyon Notu

- `ruvid` ve `ruvic` birbirine kod/crate seviyesinde bağımlı değildir; yalnızca sürüm kontrollü JSON handoff sözleşmesi (`schema_version = "1"`, `--write-manifest` / `--manifest`) ve dosya yolu üzerinden çalışırlar.
- `ruvic`, medya işleme operasyonları için sistemdeki (veya `--ffmpeg` / `--ffprobe` ile belirtilen) harici `ffmpeg` ve `ffprobe` araçlarını subprocess olarak güvenli argüman dizisiyle çalıştırır. Testler sahte `ffmpeg.exe` / `ffprobe.exe` ile doğrulanmıştır; gerçek medya dosyaları üzerinde dönüştürme yapabilmek için ortamda `ffmpeg` / `ffprobe` kurulu olmalıdır (`ruvic doctor` ile kontrol edilebilir).

## Yapılandırma ve Çalıştırma Örnekleri

```powershell
.\ruvid\target\release\ruvid.exe --help
.\ruvid\target\release\ruvid.exe doctor --json
.\ruvic\target\release\ruvic.exe --help
.\ruvic\target\release\ruvic.exe preset list
.\ruvic\target\release\ruvic.exe doctor --json
```

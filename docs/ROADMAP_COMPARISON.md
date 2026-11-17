# RuviTools — Yol Haritası Kıyaslama ve Durum Analizi

> **Soru:** *"Bu dosyadaki iyileştirmeler bizde mevcut mu veya hangilerini eklemeliyiz?"*
> **Kapsam:** *"RuviTools — %100 Rust Yeni Nesil Mimari ve Yol Haritası"* ile mevcut deponun (`d:\mtahiryilmazz\Github\RuviTools`) dosya ve test bazlı teknik karşılaştırması.

---

## Özet Sonuç

RuviTools'un **%100 Saf Rust çekirdek yığını** (`ruvid`, `ruvic`, `ruvi-native-host`, `ruvi-gui`, `ruvitube` Chrome MV3 ve canlı benchmark raporları) halihazırda **tamamlanmış ve çalışır durumdadır**.

Yol haritasındaki iyileştirmelerin **P0 (sıfır çakışmalı, acil)** bölümü bu çalışma kapsamında depoya eklendi:
- Tedarik zinciri ve lisans denetimi (`deny.toml`)
- Çoklu crate CI/CD otomasyonu (`.github/workflows/ci.yml`)
- Eksik konsol kurulum/kaldırma betiklerinin onarımı (`ruvitube/installer/Install.ps1` & `Uninstall.ps1`)
- Birleşik doğrulama betiği (`scripts/verify-all.ps1`)

Geriye kalan **P1 (CLI zenginleştirmeleri, Firefox MV3 desteği, WiX v7 `.msi` paketleme, `rustls-tls` geçişi)** ve **P2 (`mimalloc` bellek ayırıcı — benchmark kapılı)** maddeleri ise aktif M1 UI/Jobs teslimatları sonrasında uygulanmak üzere planlanmıştır.

---

## 1. Halihazırda Mevcut Olanlar (Doğrulanmış Envanter)

| Bileşen | Durum | Depodaki Kanıtlar ve Öne Çıkan Yetenekler |
|---|---|---|
| **Saf Rust `ruvid` İndirme Motoru** | **Mevcut** | `ruvid/Cargo.toml`, `ruvid/src/resolver/platform/youtube.rs`, `ruvid/src/downloader/http_range.rs`, `ruvid/src/cli/args.rs`, `ruvid/docs/STATUS.md`<br>• Harici `yt-dlp` / `deno` olmadan HTML `ytInitialPlayerResponse` + Innertube iOS/Android çözümleme.<br>• Dinamik `Range: bytes=0-0` yoklaması, `--jobs N` paralel parça indirme, `.part` atomik commit ve `.part.ruvid.json` resume.<br>• DASH dual-stream eşleme + otomatik `ruvic remux`, altyazı (`.vtt`/`.srt`/`.sbv`), bölüm bölme (`--split-chapters`), klip (`--clip`), oynatma listesi (`playlist --items --concurrency`), SSRF koruması ve 103+ test. |
| **Saf Rust `ruvic` Medya Motoru** | **Mevcut** | `ruvic/Cargo.toml`, `ruvic/src/backend/native/mp4_atoms.rs`, `ebml.rs`, `container_probe.rs`, `processor.rs`, `ruvic/docs/STATUS.md`<br>• Harici `ffmpeg`/`ffprobe` gerektirmeyen `NativeMediaBackend` (isteğe bağlı `--ffmpeg`/`--ffprobe` destekli).<br>• 64 KB / 1 MB `BufReader`/`BufWriter` streaming (`copy_chunked`), `stco`/`co64` offset yeniden hesaplamalı faststart remux ve trim.<br>• ADTS AAC / M4A demux, çift girdili MP4/WebM DASH mux, 360/VR (`sv3d`, `proj`, EBML `0x7670`) ve HDR (`colr`, `mdcv`, `clli`, EBML `0x55B0`) koruma, `validate_moov_integrity` (`ExitCode::Integrity = 8`) ve 106+ test. |
| **`ruvi-native-host` MV3 Host** | **Mevcut** | `ruvi-native-host/Cargo.toml`, `ruvi-native-host/src/main.rs`<br>• `ruvid` + `ruvic` süreç içi (in-process) bağlı `NativeHost.exe`.<br>• 4-byte LE uzunluk önekli JSON Native Messaging protokolü, URL doğrulama ve `--jobs` (`1..=16`) aktarımı. |
| **`ruvi-gui` Masaüstü Stüdyosu** | **Mevcut** | `ruvi-gui/Cargo.toml`, `ruvi-gui/src/main.rs`, `app.rs`, `icons.rs`, `build.rs`, `assets/`<br>• `eframe 0.31` / `egui` tabanlı `ruvi-studio.exe`, sol yan panel navigasyonu, vektör simgeler, Dark/Light tema, `.exe` gömülü uygulama ikonu ve `--headless-self-test`. |
| **`ruvitube` Chrome MV3 Eklentisi** | **Mevcut** | `ruvitube/extension/manifest.json`, `background.js`, `popup.*`, `options.*`, `content.js`, `_locales/{tr,en}/messages.json`, `ruvitube/installer/install-core.ps1`<br>• Chrome MV3 (`minimum_chrome_version: "116"`), Türkçe/İngilizce yerelleştirme, `--jobs` seçici ve CDP otomatik yükleme (`Install-GUI.ps1`, `launch.ps1`). |
| **Ampirik Benchmark Raporları** | **Mevcut** | `BENCHMARK.md`, `BENCHMARK_REPORT.md`, `docs/assets/benchmark_dashboard.svg`<br>• İkili boyut: `ruvid.exe` ~6.87 MB + `ruvic.exe` ~3.73 MB = **10.60 MB** (sıfır bağımlılık).<br>• 235.8 MB YouTube akışı (`AKeUssuu3Is`): `ruvid --jobs 8` **8.505 sn (26.44 MB/s)** — `yt-dlp`'ye göre **5.34x** hızlı.<br>• 50.3 MB / 881k kare MP4 faststart remux: `ruvic` **0.287 sn** ve **3.49 MB özel RAM** — `ffmpeg`'e göre **46.8x** hızlı ve **49.5x** daha az RAM. |

---

## 2. P0 Kapsamında Şimdi Eklenenler (Sıfır Çakışmalı İyileştirmeler)

| Dosya | Çözülen Eksiklik | Teknik İçerik |
|---|---|---|
| `deny.toml` | Tedarik zinciri, lisans ve crate güvenlik politikası eksikti | Tüm 4 `Cargo.lock` analiz edilerek gerçek bağımlılık lisansları (`MIT`, `Apache-2.0`, `BSD-*`, `ISC`, `MPL-2.0`, `Unicode-3.0`, `Zlib`, `BSL-1.0`, `0BSD`, `CDLA-Permissive-2.0`, `egui` için `epaint_default_fonts` `OFL-1.1`/`Ubuntu-font-1.0` istisnası), RustSec `[advisories]`, `[sources]` (`crates.io` dışı reddi) ve `[bans]` (`wildcards = "deny"`, `multiple-versions = "warn"`, `openssl-sys` sadece `openssl` wrapper üzerinden geçici toleransla işaretli) tanımlandı. |
| `.github/workflows/ci.yml` | Otomatik CI/CD iş akışı yoktu | `ruvid` (`ubuntu-latest` + `windows-latest`), `ruvic` (`ubuntu-latest` + `windows-latest`), `ruvi-native-host` (`windows-latest`) ve `ruvi-gui` (`windows-latest`) matrisi için `fmt --check`, `clippy -D warnings`, `cargo test`, ayrıca 4 crate için `EmbarkStudios/cargo-deny-action@v2` ve PowerShell/JSON doğrulama adımları eklendi. |
| `ruvitube/installer/Install.ps1`<br>`ruvitube/installer/Uninstall.ps1` | `Kurulum.cmd:7` ve `Kaldir.cmd:2` var olmayan `.ps1` dosyalarını çağırıp hata veriyordu | `ruvitube/installer/install-core.ps1` dot-source edilerek `Invoke-Install` ve `Invoke-Uninstall` sözleşmelerine tam uyumlu, Türkçe konsol çıktılı ve 0/1 çıkış kodlu `Install.ps1` ve `Uninstall.ps1` oluşturuldu. |
| `scripts/verify-all.ps1` | Tek komutla çoklu-crate ve betik doğrulaması yoktu | 4 crate için `fmt` + `clippy` + `test` (paralel düzenlemelerde `-SkipRust` seçeneğiyle), `deny.toml` yapı kontrolü, tüm `Cargo.lock` dosyalarında `crates.io` dışı kaynak ve TLS envanteri denetimi, `ruvitube/extension/**/*.json` doğrulaması ve `.ps1` AST sözdizimi doğrulaması eklendi. |

---

## 3. Eklenmesi Önerilen Kalan İyileştirmeler (P1 & P2 Yol Haritası)

### P1 — M1 Teslimatları Tamamlanır Tamamlanmaz Uygulanmalı (Yüksek Değer)

1. **CLI Zenginleştirmeleri (`ruvid` & `ruvic`)**
   - *Mevcut boşluk:* `ruvid` ve `ruvic` içinde `clap_complete` yok; `ruvid/src/cli/args.rs` içinde `GetArgs` kuru çalıştırma (`--simulate`) içermiyor; `PlaylistArgs` yalnızca tek URL alıyor (`--batch-file` yok) ve `ruvid/src/cli/commands.rs` içindeki `handle_playlist` altyazı/ses/manifest bayraklarını sabit `false`/`None` bırakıyor.
   - *Eklenecekler:*
     - `ruvid` ve `ruvic` için `completions <shell>` alt komutu (`clap_complete`).
     - `ruvid get --simulate` (ağ indirmesi ve disk yazımı olmadan çözümleme/dosya adı önizlemesi).
     - `ruvid playlist --batch-file <PATH>` (`-` ile stdin desteği) ve `--subtitles`, `--subtitles-lang`, `--sub-format`, `--auto-subs`, `--audio-lang`, `--write-manifest`, `--split-chapters` geçişi.

2. **Firefox MV3 Desteği ve Çift Tarayıcı Kurulumu (`ruvitube`)**
   - *Mevcut boşluk:* `ruvitube/extension/manifest.json` yalnızca Chrome'a özgü (`service_worker`, `minimum_chrome_version`, `key`); `background.js` içinde `chrome://downloads/` açılıyor; `install-core.ps1` yalnızca `HKCU:\Software\Google\Chrome\NativeMessagingHosts` yazıyor. (`NativeHost.exe` dosyaları doğrudan `Downloads\GeTube` klasörüne taşıdığı için Firefox'un `file://` kısıtı engel olmaktan çıkmıştır.)
   - *Eklenecekler:*
     - `ruvitube/extension/manifest.firefox.json` (`background.scripts` + `browser_specific_settings.gecko.id`).
     - `background.js` içinde `chrome://downloads/` vs `about:downloads` tarayıcı koruması.
     - `install-core.ps1` içinde `HKCU:\Software\Mozilla\NativeMessagingHosts\com.ruvitube.nativehost` kaydı (`allowed_extensions`).

3. **Birleşik Windows Release Paketleyici & WiX v7 `.msi` Kurulumu**
   - *Mevcut boşluk:* Makinede `wix 7.0.0` kurulu olmasına rağmen `scripts/build-release.ps1` ve `.wxs` tanımı yok; `ruvitube/bin/` yalnızca `NativeHost.exe` içeriyor, `ruvi-studio.exe` dağıtım paketine dahil değil.
   - *Eklenecekler:*
     - `scripts/build-release.ps1`: 4 ikiliyi (`ruvi-studio.exe`, `ruvid.exe`, `ruvic.exe`, `NativeHost.exe`) release derleyip taşınabilir `.zip` ve Chrome/Firefox eklenti paketlerini üretmesi.
     - `installer/RuviTools.wxs`: Kullanıcı bazlı (`Scope="perUser"`) `.msi` kurulumu (Başlat Menüsü kısayolu, `PATH`, Chrome + Firefox Native Messaging kayıtları).

4. **`reqwest` Saf Rust TLS (`rustls-tls`) Geçişi**
   - *Mevcut boşluk:* 4 `Cargo.lock` dosyasından 3'ünde (`ruvid`, `ruvi-native-host`, `ruvi-gui`) `reqwest 0.12` varsayılan özellikleri nedeniyle `native-tls` → `openssl` → `openssl-sys` bağımlılık grafiğinde yer alıyor (aynı anda `rustls` de bulunuyor).
   - *Eklenecek:* `ruvid/Cargo.toml` içinde `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "stream", "json"] }` geçişi yapılıp `deny.toml` içindeki `openssl-sys` istisnası tamamen yasağa (`deny`) çevrilmeli.

### P2 — Benchmark Kapılı Optimizasyon

5. **`mimalloc` Global Bellek Ayırıcı**
   - *Mevcut boşluk:* Kaynak kodda `global_allocator` veya `mimalloc` kullanımı yok (`Cargo.lock` içinde yer almıyor; makinede VS 2022 `cl.exe` ve `crates.io` erişimi mevcut).
   - *Önerilen yaklaşım:* `lib.rs` içine eklemeden, yalnızca binary `main.rs` hedeflerinde (`ruvid`, `ruvic`, `ruvi-native-host`, `ruvi-gui`) `--features mimalloc` bayrağı altında denenmeli ve `BENCHMARK.md` içindeki 50.3 MB remux (**3.53 MB** özel RAM) ile `ruvid --jobs 8` (**8.505 sn**) taban çizgilerine karşı bellek/hız artışı ölçülerek varsayılan yapılıp yapılmayacağına karar verilmeli.

---

## 4. Hızlı Doğrulama Komutu

```powershell
# Hızlı denetim (JSON, PowerShell sözdizimi, Cargo.lock kaynakları, deny.toml):
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-all.ps1 -SkipRust

# Tam doğrulama (tüm 4 crate için fmt + clippy -D warnings + test dahil):
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-all.ps1
```

# RuviTools — Yol Haritası Kıyaslama ve Durum Analizi

> **Soru:** *"Bu dosyadaki iyileştirmeler bizde mevcut mu veya hangilerini eklemeliyiz?"*
> **Kapsam:** *"RuviTools — %100 Rust Yeni Nesil Mimari ve Yol Haritası"* ile mevcut deponun (`d:\mtahiryilmazz\Github\RuviTools`) dosya ve test bazlı teknik karşılaştırması.

---

## Özet Sonuç

RuviTools'un **%100 Saf Rust çekirdek yığını** (`ruvid`, `ruvic`, `ruvi-native-host`, `ruvi-gui`, `ruvitube` Chrome/Firefox MV3 ve canlı benchmark raporları) ile yol haritasındaki **P0, P1 ve P2** iyileştirmeleri **hayata geçirilmiş ve doğrulanmıştır**:
- **P0 (Güvenlik & Otomasyon):** `deny.toml`, `.github/workflows/ci.yml`, `ruvitube/installer/{Install,Uninstall}.ps1`, `scripts/verify-all.ps1`.
- **P1 (CLI, Firefox & Windows Paketleme):** `ruvid` + `ruvic` `completions <shell>` (`clap_complete`), `ruvid get --simulate`, `ruvid playlist -a/--batch-file` + tam bayrak geçişi (`--subtitles`, `--audio-lang`, `--write-manifest`, `--split-chapters`), `ruvitube/extension/manifest.firefox.json` + `background.js` `about:downloads` koruması + `install-core.ps1` Mozilla `NativeMessagingHosts` kaydı, `scripts/build-release.ps1` + WiX v7 `installer/RuviTools.wxs` (`.msi` + taşınabilir `.zip`).
- **P2 (Bellek Ayırıcı Optimizasyonu):** Tüm 4 binary hedefte (`ruvid`, `ruvic`, `ruvi-native-host`, `ruvi-gui`) `lib.rs`'e dokunmadan yalnızca `src/main.rs` içinde `--features mimalloc` bayrağıyla etkinleştirilebilen opt-in `mimalloc` global allocator.

İsteğe bağlı gelecek iyileştirme olarak `reqwest` için `rustls-tls` geçişi (`openssl-sys` bağımlılığının tamamen kaldırılması) `deny.toml` istisna notuyla takip edilmektedir.

---

## 1. Halihazırda Mevcut Olanlar (Doğrulanmış Envanter)

| Bileşen | Durum | Depodaki Kanıtlar ve Öne Çıkan Yetenekler |
|---|---|---|
| **Saf Rust `ruvid` İndirme Motoru** | **Mevcut** | `ruvid/Cargo.toml`, `ruvid/src/resolver/platform/youtube.rs`, `ruvid/src/downloader/http_range.rs`, `ruvid/src/cli/args.rs`, `ruvid/docs/STATUS.md`<br>• Harici `yt-dlp` / `deno` olmadan HTML `ytInitialPlayerResponse` + Innertube iOS/Android çözümleme.<br>• Dinamik `Range: bytes=0-0` yoklaması, `--jobs N` paralel parça indirme, `.part` atomik commit ve `.part.ruvid.json` resume.<br>• DASH dual-stream eşleme + otomatik `ruvic remux`, altyazı (`.vtt`/`.srt`/`.sbv`), bölüm bölme (`--split-chapters`), klip (`--clip`), oynatma listesi (`playlist --items --concurrency`), SSRF koruması ve 103+ test. |
| **Saf Rust `ruvic` Medya Motoru** | **Mevcut** | `ruvic/Cargo.toml`, `ruvic/src/backend/native/mp4_atoms.rs`, `ebml.rs`, `container_probe.rs`, `processor.rs`, `ruvic/docs/STATUS.md`<br>• Harici `ffmpeg`/`ffprobe` gerektirmeyen `NativeMediaBackend` (isteğe bağlı `--ffmpeg`/`--ffprobe` destekli).<br>• 64 KB / 1 MB `BufReader`/`BufWriter` streaming (`copy_chunked`), `stco`/`co64` offset yeniden hesaplamalı faststart remux ve trim.<br>• ADTS AAC / M4A demux, çift girdili MP4/WebM DASH mux, 360/VR (`sv3d`, `proj`, EBML `0x7670`) ve HDR (`colr`, `mdcv`, `clli`, EBML `0x55B0`) koruma, `validate_moov_integrity` (`ExitCode::Integrity = 8`) ve 106+ test. |
| **`ruvi-native-host` MV3 Host** | **Mevcut** | `ruvi-native-host/Cargo.toml`, `ruvi-native-host/src/main.rs`<br>• `ruvid` + `ruvic` süreç içi (in-process) bağlı `NativeHost.exe`.<br>• 4-byte LE uzunluk önekli JSON Native Messaging protokolü, URL doğrulama ve `--jobs` (`1..=16`) aktarımı. |
| **`ruvi-gui` Masaüstü Stüdyosu** | **Mevcut** | `ruvi-gui/Cargo.toml`, `ruvi-gui/src/main.rs`, `app.rs`, `icons.rs`, `build.rs`, `assets/`<br>• `eframe 0.31` / `egui` tabanlı `ruvi-studio.exe`, sol yan panel navigasyonu, vektör simgeler, Dark/Light tema, `.exe` gömülü uygulama ikonu ve `--headless-self-test`. |
| **`ruvitube` Chrome MV3 Eklentisi** | **Mevcut** | `ruvitube/extension/manifest.json`, `background.js`, `popup.*`, `options.*`, `content.js`, `_locales/{tr,en}/messages.json`, `ruvitube/installer/install-core.ps1`<br>• Chrome MV3 (`minimum_chrome_version: "116"`), Türkçe/İngilizce yerelleştirme, `--jobs` seçici ve CDP otomatik yükleme (`Install-GUI.ps1`, `launch.ps1`). |
| **Ampirik Benchmark Raporları** | **Mevcut** | `BENCHMARK.md`, `BENCHMARK_REPORT.md`, `docs/assets/benchmark_dashboard.svg`<br>• İkili boyut: `ruvid.exe` ~7.01 MB + `ruvic.exe` ~3.49 MB = **10.50 MB** (sıfır bağımlılık).<br>• 235.8 MB YouTube akışı (`AKeUssuu3Is`): `ruvid --jobs 8` **8.505 sn (26.44 MB/s)** — `yt-dlp`'ye göre **5.34x** hızlı.<br>• 50.3 MB / 881k kare MP4 faststart remux: `ruvic` **0.287 sn** ve **3.49 MB özel RAM** — `ffmpeg`'e göre **46.8x** hızlı ve **49.5x** daha az RAM. |

---

## 2. P0 Kapsamında Şimdi Eklenenler (Sıfır Çakışmalı İyileştirmeler)

| Dosya | Çözülen Eksiklik | Teknik İçerik |
|---|---|---|
| `deny.toml` | Tedarik zinciri, lisans ve crate güvenlik politikası eksikti | Tüm 4 `Cargo.lock` analiz edilerek gerçek bağımlılık lisansları (`MIT`, `Apache-2.0`, `BSD-*`, `ISC`, `MPL-2.0`, `Unicode-3.0`, `Zlib`, `BSL-1.0`, `0BSD`, `CDLA-Permissive-2.0`, `egui` için `epaint_default_fonts` `OFL-1.1`/`Ubuntu-font-1.0` istisnası), RustSec `[advisories]`, `[sources]` (`crates.io` dışı reddi) ve `[bans]` (`wildcards = "deny"`, `multiple-versions = "warn"`, `openssl-sys` sadece `openssl` wrapper üzerinden geçici toleransla işaretli) tanımlandı. |
| `.github/workflows/ci.yml` | Otomatik CI/CD iş akışı yoktu | `ruvid` (`ubuntu-latest` + `windows-latest`), `ruvic` (`ubuntu-latest` + `windows-latest`), `ruvi-native-host` (`windows-latest`) ve `ruvi-gui` (`windows-latest`) matrisi için `fmt --check`, `clippy -D warnings`, `cargo test`, ayrıca 4 crate için `EmbarkStudios/cargo-deny-action@v2` ve PowerShell/JSON doğrulama adımları eklendi. |
| `ruvitube/installer/Install.ps1`<br>`ruvitube/installer/Uninstall.ps1` | `Kurulum.cmd:7` ve `Kaldir.cmd:2` var olmayan `.ps1` dosyalarını çağırıp hata veriyordu | `ruvitube/installer/install-core.ps1` dot-source edilerek `Invoke-Install` ve `Invoke-Uninstall` sözleşmelerine tam uyumlu, Türkçe konsol çıktılı ve 0/1 çıkış kodlu `Install.ps1` ve `Uninstall.ps1` oluşturuldu. |
| `scripts/verify-all.ps1` | Tek komutla çoklu-crate ve betik doğrulaması yoktu | 4 crate için `fmt` + `clippy` + `test` (paralel düzenlemelerde `-SkipRust` seçeneğiyle), `deny.toml` yapı kontrolü, tüm `Cargo.lock` dosyalarında `crates.io` dışı kaynak ve TLS envanteri denetimi, `ruvitube/extension/**/*.json` doğrulaması ve `.ps1` AST sözdizimi doğrulaması eklendi. |

---

## 3. P1 & P2 Kapsamında Uygulanan ve Doğrulanan İyileştirmeler

| Aşama | Bileşen / Dosya | Durum | Uygulanan Teknik Kapsam |
|---|---|---|---|
| **P1** | **CLI Zenginleştirmeleri (`ruvid` & `ruvic`)**<br>`ruvid/src/cli/{args,commands,render}.rs`, `ruvid/src/downloader/engine.rs`, `ruvic/src/cli/{args,commands}.rs`, `*/tests/cli_tests.rs` | **Uygulandı** | • `clap_complete = "4"` ile `ruvid completions <shell>` ve `ruvic completions <shell>` (`bash`, `zsh`, `fish`, `powershell`, `elvish`).<br>• `ruvid get --simulate` (medya indirmeden ve disk dizini/dosyası oluşturmadan çözümleme, akış seçimi ve çıktı yolu önizlemesi; JSON ve insan okunur çıktı).<br>• `ruvid playlist -a/--batch-file <PATH>` (`-` ile stdin, `#` yorum ve boş satır filtreleme) + `--subtitles`, `--subtitles-lang`, `--sub-format`, `--auto-subs`, `--audio-lang`, `--write-manifest`, `--split-chapters` geçişleri.<br>• `ruvid/tests/cli_tests.rs` ve `ruvic/tests/cli_tests.rs` entegrasyon testleri. |
| **P1** | **Firefox MV3 & Çift Tarayıcı Kurulumu (`ruvitube`)**<br>`ruvitube/extension/manifest.firefox.json`, `background.js`, `ruvitube/installer/install-core.ps1` | **Uygulandı** | • `manifest.firefox.json`: `background.scripts`, `browser_specific_settings.gecko.id = "ruvitube@ruvitools.dev"` (`strict_min_version: "115.0"`), Chrome'a özgü `key`/`minimum_chrome_version` kaldırıldı.<br>• `background.js`: Worker B `--jobs` mantığı korunarak `chrome://downloads/` vs `about:downloads` (+ `downloads.showDefaultFolder()` geri dönüşü) koruması eklendi.<br>• `install-core.ps1`: `com.ruvitube.nativehost.firefox.json` (`allowed_extensions`) üretimi ve `HKCU:\Software\Mozilla\NativeMessagingHosts\com.ruvitube.nativehost` kayıt/doğrulama/kaldırma desteği eklendi. |
| **P1** | **Birleşik Windows Release Paketleyici & WiX v7 `.msi`**<br>`scripts/build-release.ps1`, `installer/RuviTools.wxs` | **Uygulandı** | • `installer/RuviTools.wxs`: `Scope="perUser"` MSI kurulumu (`ruvi-studio.exe`, `ruvid.exe`, `ruvic.exe`, `NativeHost.exe`, `ruvitube/extension/`, Başlat Menüsü `RuviStudio` kısayolu, kullanıcı `PATH`, Chrome + Firefox `NativeMessagingHosts` kayıtları).<br>• `scripts/build-release.ps1`: 4 crate release derlemesi, `ruvitube/bin` SHA256 senkronizasyonu, Chrome & Firefox `.zip`, taşınabilir `.zip` ve `wix build` ile `dist/RuviTools-v0.1.0-win64.msi` üretimi. |
| **P2** | **İsteğe Bağlı `mimalloc` Global Bellek Ayırıcı**<br>`{ruvid,ruvic,ruvi-native-host,ruvi-gui}/{Cargo.toml,src/main.rs}` | **Uygulandı** | • Tüm 4 crate için `mimalloc = { version = "0.1", optional = true }` ve `[features] mimalloc = ["dep:mimalloc"]` eklendi.<br>• Yalnızca binary `src/main.rs` dosyalarında `#[cfg(feature = "mimalloc")] #[global_allocator]` tanımlandı (`lib.rs` temiz tutuldu).<br>• Varsayılan derleme/testler ile `--features mimalloc` derleme/testleri ve `--version` kontrolleri doğrulandı. |
| **P2** | **`reqwest` Saf Rust TLS (`rustls-tls`) Geçişi**<br>`ruvid/Cargo.toml`, `deny.toml`, `scripts/verify-all.ps1` | **Uygulandı** | • `ruvid/Cargo.toml`: `reqwest = { version = "0.12", default-features = false, features = ["charset", "http2", "json", "rustls-tls", "stream", "system-proxy"] }` — varsayılan `default-tls` → `native-tls` → `openssl-sys` zinciri tamamen kaldırıldı, varsayılan özellik kümesi (charset/http2/system-proxy) korundu.<br>• `deny.toml`: geçici `openssl-sys` wrapper istisnası kaldırıldı; `openssl-sys`, `native-tls` ve `schannel` artık koşulsuz `deny` (native C TLS yığını grafiğe geri giremez).<br>• `scripts/verify-all.ps1`: TLS denetimi artık `openssl`/`openssl-sys`/`native-tls`/`schannel` bulunduğunda **FAIL** veriyor; NUL ile dolgulanmış/bozuk `Cargo.lock` dosyalarını da tespit ediyor (`ruvi-native-host/Cargo.lock` bu şekilde bozuktu ve onarıldı).<br>• Doğrulama: `ruvid`, `ruvi-native-host`, `ruvi-gui` yeniden derlendi; üç lockfile'da yalnızca `rustls`/`hyper-rustls` var, `openssl*`/`native-tls`/`schannel` yok. Boyut telafisi: 4 crate'e `[profile.release] strip = true, lto = "fat", codegen-units = 1` eklendi; �l��len `ruvid.exe` **7,350,784 B (~7.01 MB)**, `ruvic.exe` **3,662,848 B (~3.49 MB)**, `NativeHost.exe` **5,125,632 B (~4.89 MB)**, `ruvi-studio.exe` **9,948,160 B (~9.49 MB)**, CLI toplam� **~10.50 MB**. |

---

## 4. Hızlı Doğrulama Komutu

```powershell
# Hızlı denetim (JSON, PowerShell sözdizimi, Cargo.lock kaynakları, deny.toml):
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-all.ps1 -SkipRust

# Tam doğrulama (tüm 4 crate için fmt + clippy -D warnings + test dahil):
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-all.ps1
```

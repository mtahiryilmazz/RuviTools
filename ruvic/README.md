# ruvic — Saf Rust Medya Motoru (FFmpeg Alternatifi)

<p align="center">
  <strong>%100 Saf Rust · Sıfır harici ikili · Sıfır <code>ffmpeg</code>/<code>ffprobe</code> bağımlılığı · MIT</strong>
</p>

`ruvic`, RuviTools ekosisteminin medya motorudur: harici `ffmpeg`, `ffprobe`, `yt-dlp`, `openssl`
veya herhangi bir C kütüphanesi **çağırmadan** analiz, remux, DASH birleştirme, kesit kırpma,
ses dönüştürme, meta veri gömme ve toplu işlem yapan tek ikili bir CLI + Rust kütüphanesidir.

- **Sürüm:** `0.2.0`
- **Depo:** <https://github.com/mtahiryilmazz/RuviTools> (`ruvic/`)
- **Lisans:** MIT — temiz-oda (clean-room) tasarım, hiçbir GPL/LGPL kod içermez
- **İkili boyut:** ~3.7 MB (strip + LTO), çalışma zamanı bağımlılığı yok

> **Bağımlılık politikası:** `ruvic` çalışırken hiçbir dış programı çalıştırmaz. Depodaki
> `ffmpeg`/`ffprobe` yalnızca **test oracle**'ı olarak kullanılır (`tests/differential_ffmpeg_tests.rs`,
> varsayılan olarak kapalı, `RUVIC_DIFF=1` ile açılır) ve **asla** üretim koduna veya ikiliye girmez.
> İsteğe bağlı `--ffmpeg` / `--ffprobe` bayrakları bilinçli olarak kullanıcı isteğine bırakılmıştır.

## Komutlar

| Komut | Ne yapar |
| :--- | :--- |
| `ruvic probe <GİRDİ>` | Konteyner, akış, HDR/360° meta verisi ve bölümleri raporlar |
| `ruvic remux <GİRDİ> -o <ÇIKTI>` | Yeniden kodlamadan faststart remux; `--video` + `--audio` ile DASH çift akış birleştirme |
| `ruvic trim <GİRDİ> --start <ZAMAN> (--end <ZAMAN> \| --duration <SN>) -o <ÇIKTI>` | Anahtar kare hizalı, kayıpsız kesit kırpma |
| `ruvic audio <GİRDİ> -o <ÇIKTI>` | Ses çıkarma / MP3 (CBR–VBR) ve 16-bit WAV dönüştürme, ID3v2.3 kapak |
| `ruvic convert <GİRDİ> -o <ÇIKTI>` | Preset veya açık codec ile dönüştürme |
| `ruvic thumbnail <GİRDİ> --at <ZAMAN> -o <ÇIKTI>` | Kare çıkarma |
| `ruvic batch <GİRDİ_DİZİNİ> -o <ÇIKTI_DİZİNİ>` | Dizindeki eşleşen medyayı toplu işler |
| `ruvic preset\|config\|doctor` | Preset yönetimi, `ruvic.toml`, yetenek denetimi |

## Motor Yetenekleri

- **ISO-BMFF (`MP4`/`MOV`/`M4A`/`fMP4`):** `moov`/`mdat` kutu ayrıştırma, `stco` (32-bit) ve `co64`
  (64-bit) offset yeniden hesaplamalı faststart remux, tam örnek tablosu (`stts`, `ctts`, `stss`,
  `stsc`, `stsz`) yeniden inşası, `validate_moov_integrity` (yazma öncesi reddet, `ExitCode::Integrity = 8`).
- **DASH defragmenter:** Ayrı video/ses `moof+mdat` akışlarını tek standart Faststart MP4'e indirger.
- **WebM/Matroska (EBML):** Küme birleştirme, `TimestampScale` tabanlı zaman hesabı, kesit kırpma.
- **Ses:** Symphonia çözücü + `rusty_mp3` kodlayıcı (320k/VBR MP3), 16-bit RIFF/WAV, ADTS AAC ve M4A demux,
  ID3v2.3 (`APIC`) ve iTunes (`covr`) kapak/meta veri gömme.
- **Meta veri koruma:** HDR10 (`colr`, `mdcv`, `clli`), 360°/VR (`svil`, `proj`, EBML `0x7670`).
- **Akış (streaming) I/O:** 64 KB / 1 MB tamponlu `BufReader`/`BufWriter`; 50 MB remux'ta ~3.5 MB özel RAM.
- **Atomik yazma:** `.ruvic-tmp-*` geçici dosya + hata anında temizlik, yarım çıktı bırakmaz.

## Zaman Ölçekleme: Taşma-Güvenli Tam Sayı Aritmetiği (0.2.0)

Sürüm 0.2.0 ile `mvhd`/`mdhd`/`stts`/`elst`/EBML zaman dönüşümleri `f64` yerine
[`timescale.rs`](src/backend/native/timescale.rs) içindeki 128-bit tam sayı aritmetiğine taşındı:

- `f64` bir çarpım 2^53 tick'i aştığında kesirli kısmı sessizce kaybeder; 90 kHz saatte 2 saatlik
  bir dosya bu sınıra yaklaşır, 10+ saatlik dosyalar aşar. Artık `u128`/`i128` ile tam hesap yapılıyor.
- `30000/1001` gibi kesirli zaman tabanları tam kalır (bkz. `diff_ntsc_timescale_has_no_drift`).
- `stts`/`trun` süreleri `u64` toplanır; 32-bit sarması imkânsız.
- Bölme sıfır `timescale`'da panik yerine 1 Hz'e düşer; tüm dönüşümler doymali (`saturating`) çalışır.
- DASH parça sıralaması ve kesit sınırları, kayan nokta yerine çapraz çarpım karşılaştırmasıyla yapılır.

## Doğrulama

```powershell
cd ruvic
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test                    # hermetic: hiçbir dış program gerekmez
cargo build --release
```

### Diferansiyel "oracle" testleri (isteğe bağlı, yalnızca CI/geliştirici)

`tests/differential_ffmpeg_tests.rs`, gerçek `ffmpeg`/`ffprobe` çıktısını referans alıp native
motorun ürettiği dosyaları karşılaştırır ve her çıktıyı `ffmpeg -v error -f null -` çözme kapısından
geçirir. Varsayılan `cargo test` bunları **çalıştırmaz**:

```powershell
$env:RUVIC_DIFF="1"
cargo test --test differential_ffmpeg_tests -- --test-threads=1
```

Ayrıntı: [`docs/DIFFERENTIAL_TESTING.md`](docs/DIFFERENTIAL_TESTING.md)

### Mikro-benchmark'lar

Uçtan uca CLI süreleri yerine motorun kendisini ölçer (`criterion`):

```powershell
cargo bench --bench native_engine
cargo bench --bench native_engine -- --quick   # hızlı tur
```

## Kütüphane Olarak Kullanım

```rust
use ruvic::backend::native::NativeMediaBackend;
use ruvic::backend::MediaBackend;

# async fn demo() -> ruvic::RuviResult<()> {
let backend = NativeMediaBackend::new();
let info = backend.probe(std::path::Path::new("girdi.mp4")).await?;
println!("{} akış, {:?} sn", info.streams.len(), info.container.duration_seconds);
# Ok(()) }
```

## Dokümanlar

- [`docs/STATUS.md`](docs/STATUS.md) — güncel durum, test sayıları, doğrulama sonuçları
- [`docs/DIFFERENTIAL_TESTING.md`](docs/DIFFERENTIAL_TESTING.md) — oracle test metodolojisi
- [`RUVIC_ARCHITECTURE.md`](RUVIC_ARCHITECTURE.md) — modül mimarisi
- [`docs/RUVIC_COMMANDS.md`](docs/RUVIC_COMMANDS.md) — komut referansı

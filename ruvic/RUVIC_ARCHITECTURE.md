# RUVIC — Rust Video Converter
## Teknik Mimari, Klasör Yapısı, CLI ve MVP Planı

RUVIC, RUVI ürün ailesinin medya analiz ve dönüştürme aracıdır. Kontrol katmanı Rust; ilk sürüm medya işleme backend'i FFmpeg/ffprobe subprocess olacaktır.

## 1. Tasarım kararı

**Binary:** `ruvic`  
**Rust crate:** `ruvic-app`  
**Ana görev:** probe, remux, transcode, audio extraction, trim, thumbnail ve ileride filter/batch.

FFmpeg'i sıfırdan Rust ile yeniden yazmak hedeflenmemelidir. Codec/container dünyası çok geniştir; RUVIC'in değeri güvenli Rust orkestrasyonu, tutarlı CLI, preset, iş planlama, doğrulama ve kullanıcı deneyimidir.

FFmpeg dağıtım yöntemi (sistemden bulma veya paketleme), kullanılan build'in lisansı ve codec konfigürasyonu yayın öncesinde doğrulanmalıdır. Rust binding'leri MVP için zorunlu değildir.

## 2. Klasör yapısı

```text
apps/ruvic/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── app.rs
│   ├── error.rs
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── args.rs
│   │   ├── commands.rs
│   │   └── render.rs
│   ├── config/
│   │   ├── mod.rs
│   │   ├── schema.rs
│   │   └── loader.rs
│   ├── domain/
│   │   ├── mod.rs
│   │   ├── media_info.rs
│   │   ├── stream.rs
│   │   ├── conversion_job.rs
│   │   ├── output_spec.rs
│   │   └── preset.rs
│   ├── probe/
│   │   ├── mod.rs
│   │   ├── service.rs
│   │   └── normalize.rs
│   ├── planner/
│   │   ├── mod.rs
│   │   ├── conversion_plan.rs
│   │   ├── codec_selection.rs
│   │   └── validation.rs
│   ├── operations/
│   │   ├── mod.rs
│   │   ├── convert.rs
│   │   ├── remux.rs
│   │   ├── extract_audio.rs
│   │   ├── trim.rs
│   │   └── thumbnail.rs
│   ├── backend/
│   │   ├── mod.rs
│   │   ├── selector.rs
│   │   └── ffmpeg/
│   │       ├── mod.rs
│   │       ├── locator.rs
│   │       ├── command_builder.rs
│   │       ├── process.rs
│   │       ├── progress_parser.rs
│   │       └── capability.rs
│   ├── presets/
│   │   ├── mod.rs
│   │   ├── builtin.rs
│   │   └── loader.rs
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── output.rs
│   │   ├── temp_files.rs
│   │   └── atomic_commit.rs
│   └── telemetry/
│       ├── mod.rs
│       └── progress.rs
└── tests/
    ├── cli/
    ├── probe/
    ├── remux/
    ├── transcode/
    ├── audio/
    ├── trim/
    └── fixtures/
```

## 3. Pipeline

```text
Input -> Validate -> Probe -> Conversion Planner
      -> Codec/Container Validation -> Backend Selector
      -> FFmpeg Process Supervisor -> Temp Output
      -> Probe/Output Validation -> Atomic Commit -> Result
```

### Katmanlar

- `probe`: ffprobe çıktısını normalize edilmiş `MediaInfo` modeline dönüştürür.
- `planner`: kullanıcının istediği çıktı ile kaynak stream'leri karşılaştırır, uygulanabilir plan üretir.
- `operations`: convert/remux/audio/trim/thumbnail use-case'leri.
- `backend`: FFmpeg komutlarını kurar ve child process'i yönetir.
- `presets`: dahili ve kullanıcı preset'leri.
- `storage`: temp dosya, overwrite ve atomic output.
- `telemetry`: progress ve JSON sonuç.

## 4. FFmpeg backend güvenliği

1. `Command` ile executable ve argümanları ayrı ayrı ver; shell string çalıştırma.
2. `ffmpeg` ve `ffprobe` yolu PATH'ten bulunabilir veya config/CLI ile belirtilebilir.
3. `-version` ve capability sorgusu yapılır.
4. `-progress pipe:1` gibi makineye uygun progress kanalı kullanılır.
5. stderr sınırlı ve güvenli biçimde yakalanır; token/path gibi hassas veriler loglanmamalıdır.
6. Cancel/timeout halinde child process sonlandırılır ve beklenir.
7. Çıktı temp dosyaya yazılır; probe doğrulaması geçerse final path'e taşınır.
8. Input ve output aynı dosya ise reddedilir.
9. Mevcut dosya varsayılan olarak korunur; `--overwrite` açıkça istenir.
10. Donanım encoder yalnızca capability tespit edilirse seçilir; fallback davranışı raporlanır.

## 5. CLI

### Global seçenekler

`-v/--verbose`, `-q/--quiet`, `--json`, `--config PATH`, `--ffmpeg PATH`, `--ffprobe PATH`, `--no-color`, `--version`, `--help`.

### `ruvic probe <INPUT>`

```powershell
ruvic probe ".\input.mp4"
ruvic probe ".\input.mkv" --json
ruvic probe ".\input.mp4" --streams --chapters
```

### `ruvic convert <INPUT>`

```powershell
ruvic convert ".\input.mkv" -o ".\output.mp4"
ruvic convert ".\input.mkv" --video h264 --audio aac -o ".\output.mp4"
ruvic convert ".\input.mp4" --preset web-mp4 -o ".\web.mp4"
ruvic convert ".\input.mp4" --quality 23 --speed medium -o ".\output.mp4"
```

| Seçenek | Açıklama |
|---|---|
| `-o, --output <PATH>` | Zorunlu hedef |
| `--video <CODEC>` | Video codec |
| `--audio <CODEC>` | Audio codec |
| `--quality <VALUE>` | Codec'e özel kalite parametresi |
| `--speed <PRESET>` | Encoder hız/kalite tercihi |
| `--preset <NAME>` | Hazır profil |
| `--overwrite` | Üzerine yaz |
| `--threads <N>` | Encoder thread |
| `--hwaccel <MODE>` | Capability tabanlı hızlandırma |
| `--video-stream <INDEX>` | Video stream seçimi |
| `--audio-stream <INDEX>` | Audio stream seçimi |
| `--subtitle <MODE>` | Altyazı davranışı |

`--quality` evrensel bir değer değildir; codec'e göre CRF, QP veya bitrate gibi farklı kavramlara eşlenir. CLI bunu açıkça belgelemelidir.

### `ruvic remux <INPUT>`

```powershell
ruvic remux ".\input.mkv" -o ".\output.mp4"
ruvic remux ".\input.mp4" -o ".\output.mkv" --copy
```

Remux yeniden kodlama yapmaz. Codec hedef container ile uyumsuzsa işlem öncesi hata döner.

### `ruvic audio <INPUT>`

```powershell
ruvic audio ".\video.mp4" --to mp3 -o ".\audio.mp3"
ruvic audio ".\video.mp4" --to flac -o ".\audio.flac"
ruvic audio ".\video.mp4" --copy -o ".\audio.aac"
```

### `ruvic trim <INPUT>`

```powershell
ruvic trim ".\input.mp4" --start 00:00:10 --end 00:00:35 -o ".\clip.mp4"
ruvic trim ".\input.mp4" --start 00:01:00 --duration 30 -o ".\clip.mp4"
```

Stream-copy kesme keyframe sınırlarına bağlıdır; frame-accurate trim için re-encode gerekebilir.

### `ruvic thumbnail <INPUT>`

```powershell
ruvic thumbnail ".\input.mp4" --at 00:00:05 -o ".\thumb.jpg"
```

### `ruvic preset` ve `doctor`

```powershell
ruvic preset list
ruvic preset show web-mp4
ruvic preset validate ".\my-preset.toml"
ruvic doctor
ruvic doctor --json
```

## 6. Örnek preset

```toml
name = "web-mp4"
description = "Web uyumlu H.264/AAC MP4"

[container]
format = "mp4"
faststart = true

[video]
codec = "libx264"
quality = 23
preset = "medium"
pixel_format = "yuv420p"

[audio]
codec = "aac"
bitrate = "192k"
```

Preset değerleri backend capability ile doğrulanmalı; keyfi shell argümanı çalıştırma kapısı haline gelmemelidir.

## 7. Bağımlılıklar

- `clap`: CLI
- `serde`, `serde_json`, `toml`: config/model
- `tokio` veya `std::process`: process lifecycle
- `thiserror`, `anyhow`: hata
- `tracing`: log
- `directories`: platform config/cache
- `indicatif`: progress
- `tempfile`: temp output
- `assert_cmd`, `predicates`: CLI test

FFmpeg Rust binding'i MVP için gerekli değil. Önce subprocess backend'i test edin; native binding ancak performans veya API ihtiyacı kanıtlanırsa değerlendirilmelidir.

## 8. Çıkış kodları

| Kod | Anlam |
|---:|---|
| 0 | Başarılı |
| 1 | Genel hata |
| 2 | Argüman/config hatası |
| 4 | Desteklenmeyen format/codec |
| 5 | Dosya sistemi/disk hatası |
| 6 | FFmpeg/backend hatası |
| 7 | İptal |
| 8 | Çıktı doğrulama hatası |

## 9. MVP yol haritası

1. CLI/config/doctor.
2. FFmpeg ve ffprobe discovery.
3. Probe + JSON.
4. Remux.
5. Tek preset ile convert.
6. Process cancellation/progress/timeout.
7. Output validation ve atomic commit.
8. Audio extraction, trim, thumbnail.
9. Preset yönetimi, batch ve hardware acceleration.

## 10. Test planı

- FFmpeg bulunamadı / uyumsuz sürüm.
- ffprobe JSON parse.
- Codec/container uyumsuzluğu.
- Başarılı remux ve transcode.
- İptal/timeout.
- Aynı input/output path reddi.
- Mevcut dosya üzerine yazma koruması.
- Bozuk çıktı ve temp cleanup.
- Donanım encoder yokken doğru fallback veya açık hata.

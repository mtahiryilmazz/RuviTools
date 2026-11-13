# RUVID — Rust Video Downloader
## Teknik Mimari, Klasör Yapısı, CLI ve MVP Planı

Bu belge, RUVI ürün ailesindeki RUVID uygulamasının bağımsız teknik tasarımıdır. RUVID'in görevi medya kaynağını çözümlemek ve indirmektir. Codec dönüştürme RUVIC'in sorumluluğudur.

## 1. Sınırlar ve hedefler

**Binary:** `ruvid`  
**Rust crate:** `ruvid-app` (workspace içindeki `apps/ruvid`)  
**İş tipi:** I/O-bound, asenkron ağ işlemi.

Destek hedefleri:
- HTTP/HTTPS doğrudan medya dosyaları
- HLS VOD (`.m3u8`)
- DASH VOD (`.mpd`) temel profilleri
- Sonraki fazda platforma özel resolver adaptörleri
- Resume/checkpoint, retry, rate limit, JSON manifest

Kapsam dışı:
- Transcoding, filtre, codec değiştirme
- DRM veya erişim kontrolünü aşma
- Kaynak sunucu limitlerini ihlal edecek kontrolsüz paralellik

## 2. Workspace içindeki klasör yapısı

```text
apps/ruvid/
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
│   │   ├── media_source.rs
│   │   ├── media_manifest.rs
│   │   ├── stream.rs
│   │   ├── download_job.rs
│   │   └── selection.rs
│   ├── resolver/
│   │   ├── mod.rs
│   │   ├── registry.rs
│   │   ├── direct_http.rs
│   │   ├── hls/
│   │   │   ├── mod.rs
│   │   │   ├── parser.rs
│   │   │   ├── master_playlist.rs
│   │   │   └── media_playlist.rs
│   │   ├── dash/
│   │   │   ├── mod.rs
│   │   │   ├── parser.rs
│   │   │   ├── mpd.rs
│   │   │   └── segment_template.rs
│   │   └── platform/
│   │       ├── mod.rs
│   │       └── README.md
│   ├── downloader/
│   │   ├── mod.rs
│   │   ├── engine.rs
│   │   ├── scheduler.rs
│   │   ├── http_range.rs
│   │   ├── segment_worker.rs
│   │   ├── retry.rs
│   │   ├── throttle.rs
│   │   └── cancellation.rs
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── output.rs
│   │   ├── temp_files.rs
│   │   ├── checkpoint.rs
│   │   ├── naming.rs
│   │   └── atomic_commit.rs
│   ├── assembly/
│   │   ├── mod.rs
│   │   ├── segment_join.rs
│   │   └── handoff.rs
│   └── telemetry/
│       ├── mod.rs
│       └── progress.rs
└── tests/
    ├── cli/
    ├── direct_http/
    ├── hls/
    ├── dash/
    ├── resume/
    └── fixtures/
```

## 3. Pipeline

```text
URL -> Resolver Registry -> Normalized Media Manifest
    -> Stream Selector -> Download Planner
    -> Scheduler -> HTTP/Segment Workers
    -> Checkpoint/Retry/Throttle
    -> Segment Join / Atomic Commit
    -> File + Optional RUVI JSON Manifest
```

### Modül sözleşmeleri

- `resolver`: URL/manifest çözümleme; ağdan medya indirmez.
- `domain`: protokolden bağımsız medya ve iş modelleri.
- `downloader`: byte range veya segment indirme.
- `scheduler`: global ve host bazlı concurrency limitleri.
- `storage`: checkpoint, temp dosya, güvenli adlandırma.
- `assembly`: segment sıralama/birleştirme; codec encode etmez.
- `handoff`: RUVIC'in okuyabileceği sürümlü manifest.

### Kritik uygulama kuralları

1. `HEAD` zorunlu varsayılmamalı; kontrollü GET/range fallback uygulanmalı.
2. Paralel HTTP indirme için `206`, `Content-Range`, ETag/Last-Modified ve toplam boyut tutarlılığı doğrulanmalı.
3. Sunucu Range desteklemiyorsa tek akış indirmeye dönülmeli.
4. Checkpoint, kaynağın kimliği değişmişse devam ettirilmemeli.
5. Segment index/sequence korunmalı; tamamlanma bilgisi crash-safe yazılmalı.
6. HLS/DASH manifest boyutu, segment adedi, redirect ve toplam indirme sınırları olmalı.
7. TLS doğrulaması kapatılmamalı; header/token loglanmamalı.
8. `--stdout` medya verisi içindir; log/progress `stderr`'e yazılır. JSON manifest aynı stdout kanalına karıştırılmaz.
9. Dosya adına dönüştürülen başlık sanitize edilmeli; path traversal önlenmeli.
10. DRM veya erişim kontrolünü aşma desteği eklenmemeli.

## 4. CLI komutları

### Global

`-v/--verbose`, `-q/--quiet`, `--json`, `--config PATH`, `--no-color`, `--no-progress`, `--version`, `--help`.

### `ruvid get <URL>`

```powershell
ruvid get "https://example.com/video.mp4"
ruvid get "<URL>" -o ".\Downloads\video.mp4"
ruvid get "<URL>" -f best --jobs 8 --resume
ruvid get "<URL>" --limit-rate 5M --retry 5
ruvid get "<URL>" --write-manifest ".\job.ruvi.json"
```

| Argüman | Varsayılan | Açıklama |
|---|---:|---|
| `-o, --output <TEMPLATE>` | Kaynak adına göre | Hedef yol/şablon |
| `-f, --format <SPEC>` | `best` | Akış seçimi |
| `--jobs <N>` | 4 | Eşzamanlı segment/chunk işi |
| `--resume` | açık | Checkpoint'ten devam |
| `--no-resume` | — | Resume kapalı |
| `--limit-rate <RATE>` | sınırsız | `5M`, `500K` |
| `--retry <N>` | 5 | Retry sınırı |
| `--timeout <SECONDS>` | 30 | İstek timeout |
| `--proxy <URL>` | yok | Proxy |
| `--header <KEY:VALUE>` | yok | Tekrarlanabilir özel header |
| `--overwrite` | kapalı | Üzerine yazma izni |
| `--write-manifest <PATH>` | yok | JSON handoff |
| `--subtitles` | kapalı | Desteklenen altyazı |
| `--subtitles-lang <LANG>` | varsayılan | Dil seçimi |

### `ruvid info <URL>`

```powershell
ruvid info "<URL>"
ruvid info "<URL>" --json
ruvid info "<URL>" --all-formats
```

### `ruvid playlist <URL>` (MVP sonrası)

```powershell
ruvid playlist "<URL>" --items "1-5,8" --concurrency 2
ruvid playlist "<URL>" --skip-errors
```

### `ruvid config` ve `doctor`

```powershell
ruvid config path
ruvid config show
ruvid config validate
ruvid config init
ruvid doctor
ruvid doctor --json
```

## 5. Örnek config

```toml
[download]
output_dir = "./Downloads"
jobs = 4
resume = true
overwrite = false
retry_count = 5
timeout_seconds = 30

[network]
user_agent = "RUVID/0.1"
connect_timeout_seconds = 10
limit_rate = ""

[output]
progress = true
color = "auto"
json = false
```

## 6. Bağımlılıklar

- `tokio`: async runtime
- `reqwest`: HTTP/TLS/connection pool
- `clap`: CLI
- `serde`, `serde_json`, `toml`: model/config/manifest
- `m3u8-rs`: HLS parser (prototipte doğrula)
- `quick-xml`: DASH MPD parser
- `url`: URL doğrulama
- `futures`, `bytes`: stream/byte işleri
- `tracing`, `thiserror`: log ve hata
- `indicatif`: progress
- `tempfile`, `assert_cmd`, `wiremock`: test

## 7. Çıkış kodları

| Kod | Anlam |
|---:|---|
| 0 | Başarılı |
| 1 | Genel hata |
| 2 | Argüman/config hatası |
| 3 | Ağ/kaynak erişim hatası |
| 4 | Desteklenmeyen URL/manifest |
| 5 | Dosya sistemi/disk hatası |
| 7 | İptal |
| 8 | Bütünlük/doğrulama hatası |

## 8. MVP sırası

1. CLI, config, error, logging, doctor.
2. Direct HTTP tek akış indirme.
3. Retry, timeout, progress ve güvenli output.
4. Range download ve resume.
5. HLS VOD.
6. DASH VOD temel profili.
7. JSON manifest ve RUVIC handoff.
8. Playlist ve platform adaptörleri.

Her fazda unit + integration + CLI testleri bulunmalı. HLS/DASH için gerçek ağ yerine sabit fixture ve mock server kullanılmalı.

# RUVI — Rust Video Tools
## RUVID Downloader + RUVIC Converter
### Birleşik Ürün Mimarisi, Teknik Tasarım ve Geliştirme Planı

**Belge sürümü:** 1.0  
**Durum:** Başlangıç mimarisi / MVP taslağı  
**Ürün ailesi:** RUVI  
**Uygulamalar:** RUVID (`ruvid`) ve RUVIC (`ruvic`)  
**Birincil dil:** Rust  
**Hedef:** Windows öncelikli, Linux ve macOS'a taşınabilir CLI

---

# 1. Ürün ailesi ve sorumluluk sınırları

RUVI, medya edinme ve medya işleme görevlerini iki bağımsız komuta ayıran küçük bir Rust araç ailesidir.

| Ürün | Komut | Sorumluluk | Yapmayacağı iş |
|---|---|---|---|
| RUVID | `ruvid` | Kaynak keşfi, medya akışı çözümleme, indirme, segment/chunk birleştirme, resume | Codec dönüştürme, filtreleme, yeniden kodlama |
| RUVIC | `ruvic` | Medya analizi, remux, transcode, ses çıkarma, kırpma, filtre, metadata işlemleri | Web platformu çözümleme veya medya kaynağı indirme |
| RUVI ortak katmanı | Rust crates | Ortak şema, yapılandırma, hata sözleşmesi, log/telemetri ilkeleri | İki uygulamanın görevlerini tek motorda birleştirme |

**Ana tasarım kararı:** RUVID ve RUVIC ayrı binary ve ayrı ürünlerdir. İlk geliştirme aşamasında tek Git deposunda Cargo workspace kullanmak; sürümleme olgunlaştığında ayrı depolara ayırabilmek önerilir.

## 1.1 Veri akışı

```text
URL
 |
 v
RUVID
 |-- URL / platform çözümleme
 |-- HLS / DASH / HTTP indirme
 |-- resume / checkpoint
 |-- akışları orijinal codec ile birleştirme (gerektiğinde remux)
 |
 +----> media file
 |
 +----> JSON job manifest
 |
 +----> stdout stream (yalnızca desteklenen tekil akışlarda)
             |
             v
           RUVIC
             |-- probe
             |-- remux
             |-- transcode
             |-- extract audio
             |-- trim / filters
             v
         output media
```

`stdout` üzerinden aktarım büyük dosyalarda pipe back-pressure ve hata yönetimi gerektirir. MVP'de dosya yolu veya JSON manifest aktarımı ana yöntem olmalıdır. Pipe desteği ikinci aşamada, binary-safe ve test edilmiş biçimde eklenmelidir.

## 1.2 Ortak ilkeler

- Her iki uygulama da Rust kütüphanesi + CLI binary olarak tasarlanır.
- CLI arayüzü makine tarafından okunabilir JSON çıktısını destekler.
- İnsan odaklı ilerleme ve tanılama mesajları `stderr` kanalına yazılır; JSON veya medya verisi `stdout` kanalına yazılabilir.
- Varsayılan davranış güvenlidir: mevcut dosyanın üzerine yazılmaz; `--overwrite` açıkça istenir.
- Ağ, dosya sistemi ve harici proses sınırlarında timeout, boyut sınırı ve hata kontrolü bulunur.
- DRM veya erişim kontrolünü aşmaya yönelik mekanizma ürün kapsamına alınmaz. Kullanıcının erişim yetkisi olan ve teknik olarak desteklenen kaynaklar hedeflenir.
- Platform adaptörleri, platform şartları ve yürürlükteki hukuk çerçevesinde geliştirilir.
- Hiçbir kullanıcı kimlik bilgisi, cookie veya token loglara yazılmaz.

---

# 2. Depo ve workspace mimarisi

## 2.1 Önerilen monorepo

```text
ruvi/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── .gitignore
├── .editorconfig
├── LICENSE
├── README.md
├── CONTRIBUTING.md
├── SECURITY.md
├── CHANGELOG.md
├── docs/
│   ├── architecture/
│   │   ├── RUVI_OVERVIEW.md
│   │   ├── RUVID_ARCHITECTURE.md
│   │   └── RUVIC_ARCHITECTURE.md
│   ├── decisions/
│   │   ├── ADR-001-workspace-layout.md
│   │   ├── ADR-002-media-backend.md
│   │   └── ADR-003-json-contract.md
│   ├── cli/
│   │   ├── RUVID_COMMANDS.md
│   │   └── RUVIC_COMMANDS.md
│   └── development/
│       ├── BUILD.md
│       ├── TESTING.md
│       └── RELEASE.md
├── apps/
│   ├── ruvid/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   └── main.rs
│   │   └── tests/
│   └── ruvic/
│       ├── Cargo.toml
│       ├── src/
│       │   └── main.rs
│       └── tests/
├── crates/
│   ├── ruvi-common/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs
│   │       ├── error.rs
│   │       ├── manifest.rs
│   │       ├── paths.rs
│   │       ├── logging.rs
│   │       └── version.rs
│   ├── ruvi-cli/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── output.rs
│   │       └── progress.rs
│   ├── ruvi-media-model/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── container.rs
│   │       ├── stream.rs
│   │       ├── codec.rs
│   │       └── metadata.rs
│   ├── ruvi-media-backend/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── backend.rs
│   │       ├── ffmpeg/
│   │       │   ├── mod.rs
│   │       │   ├── discover.rs
│   │       │   ├── command.rs
│   │       │   ├── parser.rs
│   │       │   └── capabilities.rs
│   │       └── errors.rs
│   └── ruvi-testkit/
│       ├── Cargo.toml
│       └── src/lib.rs
├── fixtures/
│   ├── ruvid/
│   │   ├── hls/
│   │   ├── dash/
│   │   └── http/
│   └── ruvic/
│       ├── media/
│       └── expected/
├── scripts/
│   ├── dev.ps1
│   ├── test.ps1
│   ├── build-release.ps1
│   └── package.ps1
└── .github/
    └── workflows/
        ├── ci.yml
        └── release.yml
```

## 2.2 Cargo workspace

Kök `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
  "apps/ruvid",
  "apps/ruvic",
  "crates/ruvi-common",
  "crates/ruvi-cli",
  "crates/ruvi-media-model",
  "crates/ruvi-media-backend",
  "crates/ruvi-testkit",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
rust-version = "1.85"

[workspace.dependencies]
anyhow = "1"
async-trait = "0.1"
clap = { version = "4", features = ["derive", "env", "wrap_help"] }
directories = "6"
futures = "0.3"
indicatif = "0.17"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "fs", "io-util", "sync", "time", "process"] }
toml = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
```

Sürüm numaraları başlangıç önerisidir; uygulama oluşturulurken güncel kararlı Rust toolchain ve crate sürümleri doğrulanmalıdır. Gereksiz bağımlılık eklemeyin; her crate yalnızca kullandığı bağımlılığı tanımlasın.

---

# 3. Ortak altyapı sözleşmeleri

## 3.1 Yapılandırma dizinleri

Platforma uygun standart dizinler kullanılmalıdır. Sabit `~/.config` yolunu Windows'ta zorunlu kılmayın.

Önerilen mantıksal yollar:

- Config: `ruvi/ruvid.toml`, `ruvi/ruvic.toml`
- Cache: `ruvi/ruvid/`, `ruvi/ruvic/`
- State: indirme checkpoint ve iş durumları
- Logs: varsayılan olarak dosyaya log yazmak zorunlu değil; kullanıcı isterse açılır.

Rust `directories` crate'i ile işletim sisteminin standart dizinleri bulunur. İki uygulama aynı `ruvi` kökünü kullanır, ancak alt dizinleri ayrıdır.

## 3.2 Ortak hata kodları

| Kod | Kategori |
|---:|---|
| 0 | Başarılı |
| 1 | Beklenmeyen / genel hata |
| 2 | CLI argümanı veya yapılandırma hatası |
| 3 | Ağ / kaynak erişimi hatası (RUVID) |
| 4 | Desteklenmeyen kaynak / format |
| 5 | Dosya sistemi / izin / disk alanı |
| 6 | Harici medya backend hatası (RUVIC) |
| 7 | İptal edildi |
| 8 | Doğrulama / bütünlük hatası |

Hata kodları belgelenmeli ve geriye dönük uyumluluk korunmalıdır. Alt seviyedeki gerçek hatayı JSON hata nesnesinde ayrıca taşıyın.

## 3.3 JSON handoff sözleşmesi

RUVID `--write-manifest` ile JSON dosyası üretir. RUVIC `--manifest` ile bu dosyayı okuyabilir.

Örnek sözleşme:

```json
{
  "schema_version": "1",
  "producer": {
    "name": "ruvid",
    "version": "0.1.0"
  },
  "source": {
    "url": "https://example.invalid/media",
    "title": "Example media"
  },
  "output": {
    "path": "downloads/example.mp4",
    "container": "mp4",
    "size_bytes": 12345678
  },
  "streams": [
    {
      "kind": "video",
      "codec": "h264",
      "width": 1920,
      "height": 1080
    },
    {
      "kind": "audio",
      "codec": "aac",
      "language": "und"
    }
  ],
  "created_at": "2026-01-01T00:00:00Z"
}
```

Bu yalnızca örnek şemadır. Gerçek uygulamada `schema_version`, opsiyonel alanlar, path çözümleme, unknown-field toleransı ve semver uyumluluk politikası tanımlanmalıdır. Manifestte cookie, Authorization header veya gizli URL token'ı bulunmamalıdır. Kaynak URL hassas token içerebiliyorsa varsayılan olarak redakte edilmelidir.

---

# 4. RUVID — Rust Video Downloader

## 4.1 Görev ve kapsam

RUVID; doğrudan HTTP medya dosyaları, HLS ve DASH gibi desteklenen kaynakları analiz edip indiren, yarıda kalan indirmeleri sürdüren ve sonuçları dosya/manifest olarak sunan ağ ağırlıklı Rust uygulamasıdır.

### MVP kapsamı

1. Doğrudan HTTP/HTTPS dosya indirme.
2. HTTP Range destekleniyorsa kontrollü paralel parça indirme.
3. HLS VOD playlist indirme ve segmentleri sıralı birleştirme.
4. DASH VOD temel desteği; segment şablonları ve representation seçimi.
5. Akış/format listeleme (`info`).
6. Resume/checkpoint.
7. İlerleme, hız, ETA ve JSON çıktı.
8. Güvenli dosya adı ve çakışma politikası.
9. İsteğe bağlı RUVIC manifest handoff.

### Sonraki aşama

- Platforma özel extractor adaptörleri (her biri ayrı modül ve test).
- Playlist/batch indirme.
- Altyazı ve thumbnail indirme.
- Proxy, özel header ve kimlik doğrulama profilleri.
- Canlı yayın desteği (VOD'dan ayrı tasarım ve test).
- Parçalı indirmede checksum ve gelişmiş bütünlük kontrolü.

### Kapsam dışı / açık sınır

- DRM çözme veya DRM korumasını aşma.
- Yetkisiz erişim veya erişim kontrolünü atlatma.
- RUVID içinde codec dönüştürme.
- Kaynak tarafından izin verilmeyen sınırsız paralel istek.

## 4.2 RUVID klasör yapısı

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

## 4.3 RUVID mimari akışı

```text
CLI
 |
 v
Application / Download Use Case
 |
 +--> Source Resolver Registry
 |      +--> Direct HTTP Resolver
 |      +--> HLS Resolver
 |      +--> DASH Resolver
 |      +--> Platform Resolver (plugin boundary)
 |
 +--> Stream Selector
 |
 +--> Download Planner
 |
 +--> Scheduler / Workers
 |      +--> HTTP Range Worker
 |      +--> HLS Segment Worker
 |      +--> DASH Segment Worker
 |
 +--> Checkpoint + Retry + Throttle
 |
 +--> Assembler / Atomic Output Commit
 |
 +--> JSON Manifest / Terminal Result
```

## 4.4 Temel modül sorumlulukları

- `resolver`: URL veya manifesti analiz edip normalize edilmiş kaynak tanımı döndürür. Resolver indirme yapmaz.
- `domain`: HTTP detaylarından bağımsız medya ve iş modellerini tutar.
- `downloader`: planlanan byte aralıklarını veya segmentleri indirir.
- `scheduler`: toplam bağlantı, kaynak başına bağlantı ve eşzamanlı iş sayısını sınırlar.
- `storage`: geçici dosyalar, checkpoint, dosya adı ve atomik sonlandırma.
- `assembly`: yalnızca byte/segment birleştirme ve gerektiğinde remux backend çağrısı. Codec encode etmez.
- `handoff`: ortak JSON manifestini üretir.

### Önemli teknik düzeltmeler

- `HEAD` isteği her sunucuda güvenilir değildir. Resolver önce kontrollü `GET`/range veya uygun fallback stratejisi kullanmalıdır.
- `Accept-Ranges` tek başına paralel indirme garantisi değildir. `206 Partial Content`, `Content-Range`, ETag/Last-Modified ve tutarlı toplam boyut doğrulanmalıdır.
- Segmentleri dosya sonuna rastgele sırayla yazmayın. Segment index/sequence bilgisi korunmalı, tamamlanma atomik kaydedilmelidir.
- HLS AES-128 gibi standart şifreleme ile DRM aynı şey değildir; destek kararı ayrı güvenlik/hukuk incelemesine tabi olmalı. DRM korumasını aşan işlev geliştirilmemelidir.
- Ayrı ses/video akışlarının container içine alınması remux işlemidir. İlk sürümde güvenilir backend kullanın; özel muxer yazmayı MVP'ye koymayın.
- `--stdout` ile `--emit-manifest` aynı stdout kanalını paylaşmamalıdır. Manifest dosyaya yazılmalı veya JSON Lines gibi açıkça tanımlı bir protokol kullanılmalıdır.

## 4.5 RUVID CLI tasarımı

### Global seçenekler

| Seçenek | Açıklama |
|---|---|
| `-v, --verbose` | Ayrıntılı log; tekrarlanabilir (`-vv`) |
| `-q, --quiet` | İnsan odaklı çıktı ve progress gizlenir |
| `--json` | Makine tarafından okunabilir sonuç |
| `--config <PATH>` | Özel yapılandırma dosyası |
| `--no-color` | Renkleri kapatır |
| `--no-progress` | İlerleme çubuğunu kapatır |
| `--version` | Sürüm bilgisi |
| `-h, --help` | Yardım |

### Komutlar

#### `ruvid get <URL>`

```powershell
ruvid get "https://example.com/video.mp4"
ruvid get "<URL>" -o ".\Downloads\video.mp4"
ruvid get "<URL>" -f best -o ".\Downloads\%(title)s.%(ext)s"
ruvid get "<URL>" --jobs 8 --resume
ruvid get "<URL>" --limit-rate 5M
ruvid get "<URL>" --write-manifest ".\job.ruvi.json"
```

| Seçenek | Varsayılan | Açıklama |
|---|---:|---|
| `-o, --output <TEMPLATE>` | Kaynak adına göre | Hedef dosya veya ad şablonu |
| `-f, --format <SPEC>` | `best` | Akış seçimi |
| `--jobs <N>` | `4` | Kaynak başına azami eşzamanlı iş |
| `--resume` | açık | Uygun checkpoint'ten devam |
| `--no-resume` | — | Checkpoint kullanma |
| `--limit-rate <RATE>` | sınırsız | Örn. `5M`, `500K` |
| `--retry <N>` | `5` | Yeniden deneme sayısı |
| `--timeout <SECONDS>` | `30` | İstek zaman aşımı |
| `--proxy <URL>` | yok | Proxy |
| `--header <KEY:VALUE>` | yok | Tekrarlanabilir özel header |
| `--overwrite` | kapalı | Mevcut dosyanın üzerine yaz |
| `--write-manifest <PATH>` | yok | RUVI JSON manifesti |
| `--subtitles` | kapalı | Desteklenen altyazıları al |
| `--subtitles-lang <LANG>` | kaynak varsayılanı | Altyazı dili |

Not: Şablon sözdizimi ilk sürümde sınırlı ve belgelenmiş olmalıdır. Kullanıcı girdisi doğrudan dosya yoluna kontrolsüz eklenmemeli; path traversal ve geçersiz dosya adı karakterleri temizlenmelidir.

#### `ruvid info <URL>`

```powershell
ruvid info "<URL>"
ruvid info "<URL>" --json
ruvid info "<URL>" --all-formats
```

| Seçenek | Açıklama |
|---|---|
| `--all-formats` | Kullanılabilir tüm temsil/akışları göster |
| `--json` | Akış kataloğunu JSON yaz |

#### `ruvid playlist <URL>`

```powershell
ruvid playlist "<URL>" --items "1-5,8"
ruvid playlist "<URL>" --items "1-10" --concurrency 2
ruvid playlist "<URL>" --skip-errors
```

Playlist desteği MVP sonrasına bırakılabilir. `--concurrency` video başına eşzamanlı indirme sayısından ayrıdır.

#### `ruvid config`

```powershell
ruvid config path
ruvid config show
ruvid config validate
ruvid config init
```

İlk sürümde `set`/`reset` komutları zorunlu değildir. Config dosyasını elle düzenlemek yeterlidir; daha sonra güvenli anahtar bazlı düzenleme eklenebilir.

#### `ruvid doctor`

```powershell
ruvid doctor
ruvid doctor --json
```

Ağ erişimi, config, yazma izinleri, cache ve desteklenen özellikleri kontrol eder.

## 4.6 RUVID örnek yapılandırma

`ruvid.toml`:

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

Secret değerleri TOML'a düz metin olarak koymayın. Kimlik doğrulama desteği eklenirse işletim sistemi credential store veya kullanıcı tarafından sağlanan güvenli dosya izinleri değerlendirilmelidir.

---

# 5. RUVIC — Rust Video Converter

## 5.1 Görev ve kapsam

RUVIC; yerel medya dosyalarını analiz eden, remux veya transcode yapan, ses/video akışlarını çıkaran ve temel medya iş akışlarını yöneten Rust tabanlı bir CLI uygulamasıdır.

**Motor kararı:** RUVIC'in uygulama mantığı Rust'tır. İlk sürümde FFmpeg/ffprobe harici süreç olarak çağrılan medya backend'i olur. Böylece codec, container, filtre ve donanım hızlandırma desteğini sıfırdan yazmak gerekmez.

FFmpeg'in dağıtımı ve lisans uyumluluğu ayrıca ele alınmalıdır. Kullanıcıya sunulan binary paketinde kullanılan FFmpeg build'inin lisans/codec konfigürasyonu açıkça belgelenmelidir. Sistem FFmpeg'i kullanma veya ayrı paketleme seçenekleri ürün kararıdır.

### MVP kapsamı

1. `probe`: container, codec, süre, çözünürlük, FPS, bitrate, audio stream bilgisi.
2. `convert`: format/codec dönüştürme.
3. `remux`: yeniden kodlamadan container değiştirme.
4. `audio`: videodan ses çıkarma.
5. `thumbnail`: tek kare veya zaman damgasından görsel alma.
6. `trim`: başlangıç/bitiş aralığı ile kırpma.
7. `--json` ve progress çıktısı.
8. FFmpeg/ffprobe bulunabilirlik ve capability kontrolü.
9. Güvenli output, overwrite ve geçici dosya yönetimi.

### Sonraki aşama

- Preset profilleri ve kullanıcı tanımlı preset.
- Batch/queue ve klasör işleme.
- Altyazı ve metadata düzenleme.
- Filtre zinciri (scale, crop, fps, normalize vb.).
- Donanım hızlandırma (NVENC, QSV, AMF, VideoToolbox) capability detection ile.
- Çoklu çıktı ve görev manifesti.
- GUI için kararlı library API.

## 5.2 RUVIC klasör yapısı

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

## 5.3 RUVIC mimari akışı

```text
CLI
 |
 v
Application / Use Case
 |
 +--> Input Validation
 |
 +--> Probe Service ----> ffprobe backend
 |
 +--> Conversion Planner
 |      +--> codec/container compatibility
 |      +--> stream mapping
 |      +--> preset resolution
 |      +--> hardware capability check
 |
 +--> Backend Selector
 |      +--> FFmpeg process backend (MVP)
 |      +--> Future native backend (optional)
 |
 +--> Process Supervisor
 |      +--> cancellation
 |      +--> timeout
 |      +--> progress parser
 |      +--> stderr capture
 |
 +--> Temp Output / Validation
 |
 +--> Atomic Commit
 |
 v
Result / JSON
```

## 5.4 Backend arayüzü

Uygulama operasyonları doğrudan `Command::new("ffmpeg")` çağırmamalıdır. Bunun yerine backend trait'i kullanılmalıdır. Örnek kavramsal arayüz:

```rust
pub trait MediaBackend: Send + Sync {
    fn capabilities(&self) -> BackendCapabilities;
    async fn probe(&self, input: &Path) -> Result<MediaInfo, BackendError>;
    async fn execute(
        &self,
        plan: ConversionPlan,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<ExecutionReport, BackendError>;
}
```

Bu örnek tasarım taslağıdır; async trait yaklaşımı ve cancellation crate seçimi implementasyon sırasında netleştirilmelidir.

### FFmpeg proses güvenliği

- Komut satırı tek bir shell string'i olarak oluşturulmaz. Program ve argümanlar ayrı ayrı `Command` argümanları olarak verilir.
- Kullanıcı girdisi shell'e eklenmez.
- FFmpeg binary yolu açıkça bulunur veya kullanıcı config ile belirler.
- `ffmpeg -version` ve `ffprobe -version` kontrol edilir.
- Timeout, cancellation ve child process cleanup uygulanır.
- Progress için `-progress pipe:1` gibi makineye uygun kanal tercih edilir; stderr logları progress parse ile karıştırılmaz.
- Çıktı önce geçici dosyaya yazılır, probe/validation geçerse hedefe atomik taşınır.
- Çıktı dosyası input ile aynıysa güvenli biçimde reddedilir.
- Başarısız işlem yarım çıktıyı final dosya gibi bırakmaz.

## 5.5 RUVIC CLI tasarımı

### Global seçenekler

| Seçenek | Açıklama |
|---|---|
| `-v, --verbose` | Ayrıntılı tanılama |
| `-q, --quiet` | Bilgi mesajlarını azalt |
| `--json` | JSON sonuç |
| `--config <PATH>` | Özel config |
| `--ffmpeg <PATH>` | FFmpeg binary yolu |
| `--ffprobe <PATH>` | ffprobe binary yolu |
| `--no-color` | Renk kapalı |
| `--version` | Sürüm |

### Komutlar

#### `ruvic probe <INPUT>`

```powershell
ruvic probe ".\input.mp4"
ruvic probe ".\input.mkv" --json
ruvic probe ".\input.mp4" --streams
```

| Seçenek | Açıklama |
|---|---|
| `--json` | Normalize edilmiş medya bilgisini JSON yaz |
| `--streams` | Tüm stream ayrıntılarını göster |
| `--chapters` | Bölümleri göster |
| `--format` | Container bilgisi |

#### `ruvic convert <INPUT>`

```powershell
ruvic convert ".\input.mkv" -o ".\output.mp4"
ruvic convert ".\input.mkv" --video h264 --audio aac -o ".\output.mp4"
ruvic convert ".\input.mp4" --preset web-mp4 -o ".\web.mp4"
ruvic convert ".\input.mp4" --quality 23 --speed medium -o ".\output.mp4"
```

| Seçenek | Varsayılan | Açıklama |
|---|---|---|
| `-o, --output <PATH>` | zorunlu | Çıktı yolu |
| `--video <CODEC>` | preset/container kararı | Video codec |
| `--audio <CODEC>` | preset/container kararı | Audio codec |
| `--quality <VALUE>` | preset | Codec'e göre kalite değeri |
| `--speed <PRESET>` | `medium` | Encoder hız/kalite dengesi |
| `--preset <NAME>` | yok | Hazır dönüşüm profili |
| `--overwrite` | kapalı | Üzerine yaz |
| `--threads <N>` | backend | İş parçacığı sayısı |
| `--hwaccel <MODE>` | `auto`/kapalı | Desteklenen donanım hızlandırma |
| `--video-stream <INDEX>` | otomatik | Video stream seç |
| `--audio-stream <INDEX>` | otomatik | Audio stream seç |
| `--subtitle <MODE>` | kapalı | Altyazı aktarım davranışı |

Codec parametreleri codec'e göre değişir. `--quality` her codec için aynı anlama gelmez; CLI yardımında seçilen codec'e özel açıklama gösterilmelidir.

#### `ruvic remux <INPUT>`

```powershell
ruvic remux ".\input.mkv" -o ".\output.mp4"
ruvic remux ".\input.mp4" -o ".\output.mkv" --copy
```

Remux, stream'leri yeniden kodlamadan yeni container'a taşır. Hedef container seçilen stream codec'lerini desteklemiyorsa işlem başlamadan uyumluluk hatası verilmelidir.

#### `ruvic audio <INPUT>`

```powershell
ruvic audio ".\video.mp4" --to mp3 -o ".\audio.mp3"
ruvic audio ".\video.mp4" --to flac -o ".\audio.flac"
ruvic audio ".\video.mp4" --copy -o ".\audio.aac"
```

#### `ruvic trim <INPUT>`

```powershell
ruvic trim ".\input.mp4" --start 00:00:10 --end 00:00:35 -o ".\clip.mp4"
ruvic trim ".\input.mp4" --start 00:01:00 --duration 30 -o ".\clip.mp4"
```

Stream copy ile yapılan kırpma keyframe hassasiyetine bağlıdır. Kare hassasiyeti gerektiğinde yeniden kodlama gerekebileceği kullanıcıya açıklanmalıdır.

#### `ruvic thumbnail <INPUT>`

```powershell
ruvic thumbnail ".\input.mp4" --at 00:00:05 -o ".\thumb.jpg"
```

#### `ruvic preset`

```powershell
ruvic preset list
ruvic preset show web-mp4
ruvic preset validate ".\my-preset.toml"
```

#### `ruvic doctor`

```powershell
ruvic doctor
ruvic doctor --json
```

FFmpeg/ffprobe bulunabilirliğini, sürümünü, temel encoder ve muxer kabiliyetlerini raporlar.

## 5.6 RUVIC preset örneği

`presets/web-mp4.toml`:

```toml
name = "web-mp4"
description = "Web uyumlu H.264/AAC MP4 çıktı profili"

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

Preset alanları backend'e göre doğrulanmalıdır. Bilinmeyen FFmpeg argümanlarını config'ten kontrolsüz geçirerek güvenlik ve tekrarlanabilirlik kaybedilmemelidir.

---

# 6. Bağımlılık ve teknoloji önerileri

## 6.1 Ortak

| Teknoloji | Kullanım |
|---|---|
| Rust stable | Ana geliştirme dili |
| Cargo workspace | Tek depo içinde bağımsız uygulamalar |
| `clap` | CLI |
| `serde`, `serde_json`, `toml` | Veri ve config |
| `thiserror` | Kütüphane hataları |
| `anyhow` | Uygulama sınırında hata bağlamı |
| `tracing` | Yapısal log |
| `directories` | Platforma uygun config/cache yolları |
| `indicatif` | Terminal progress |
| `assert_cmd`, `predicates` | CLI entegrasyon testleri |
| `tempfile` | İzole geçici dosyalar |

## 6.2 RUVID'e özgü

| Teknoloji | Kullanım |
|---|---|
| `tokio` | Async runtime |
| `reqwest` | HTTP istemcisi |
| `futures` | Async akışlar |
| `m3u8-rs` veya eşdeğeri | HLS parse |
| `quick-xml` | DASH XML parse |
| `url` | URL parse/normalize |
| `bytes` | Byte buffer |
| `tokio-util` | Cancellation/stream yardımcıları |

Parser crate'lerinin HLS/DASH standardının ihtiyaç duyulan alt kümesini gerçekten desteklediği prototipte doğrulanmalıdır.

## 6.3 RUVIC'e özgü

| Teknoloji | Kullanım |
|---|---|
| `tokio::process` veya std process | FFmpeg/ffprobe child process |
| `serde_json` | ffprobe JSON parse |
| `regex` (gerekiyorsa) | Sınırlı progress parse |
| `which` veya özel locator | Binary keşfi |
| `tokio-util` | Cancellation token (seçilen özelliklerle) |

FFmpeg Rust binding'leri (`ffmpeg-next` vb.) ilk sürümde zorunlu tutulmamalıdır. Native binding'ler build, ABI, dağıtım ve platform karmaşıklığı ekleyebilir. Önce subprocess backend ile ürün davranışı doğrulanmalıdır.

---

# 7. Güvenlik, sağlamlık ve kalite gereksinimleri

## RUVID

- URL şeması allowlist: `http` ve `https`.
- Localhost/private network erişimi varsayılan olarak sınırlandırılmalı; kullanıcı tarafından açıkça izin verilmedikçe SSRF benzeri riskler önlenmeli.
- Redirect zincirleri ve hedef host değişimi kontrol edilmeli.
- Maksimum manifest boyutu, segment sayısı, playlist derinliği ve indirme boyutu sınırlandırılmalı.
- Dosya adı sanitize edilmeli; path traversal engellenmeli.
- Retry yalnızca geçici hatalarda; 4xx gibi kalıcı hatalarda sınırlı/akıllı davranış.
- Resume sırasında kaynak kimliği (ETag/Last-Modified/size) doğrulanmalı.
- TLS doğrulaması kapatılmamalı.
- Kullanıcı header/token değerleri loglanmamalı.
- Segment ve checkpoint yazımı crash-safe olmalı.

## RUVIC

- Harici proses shell üzerinden başlatılmamalı.
- Input/output path ve aynı dosyaya yazma kontrolü.
- FFmpeg argümanları tipli plan nesnesinden üretilmeli.
- Kaynak dosya boyutu/süre ve işlem timeout sınırları opsiyonel olarak konfigüre edilmeli.
- Çıktı tamamlandıktan sonra ffprobe ile doğrulanmalı.
- Hata durumunda temp dosyası temizlenmeli veya teşhis için açıkça korunmalı.
- Donanım hızlandırma yalnızca capability doğrulandıktan sonra seçilmeli.
- Kullanıcıya gerçek encoder seçimi ve fallback nedeni raporlanmalı.

---

# 8. Test stratejisi

## 8.1 Test piramidi

1. Unit: parser, selector, path sanitizer, config validation, command builder.
2. Component: mock HTTP server, HLS/DASH fixtures, fake FFmpeg executable.
3. Integration: gerçek FFmpeg mevcutsa küçük örnek medya ile smoke test.
4. CLI: `assert_cmd` ile exit code, stdout/stderr ve JSON schema.
5. Regression: her hata için fixture ve tekrar üretilebilir test.
6. Cross-platform: Windows, Linux; macOS destek hedefleniyorsa CI.

## 8.2 RUVID test senaryoları

- Normal HTTP 200 indirme.
- Range 206 ve Content-Range doğrulama.
- Range desteklenmeyen sunucuya fallback.
- ETag değiştiğinde resume reddi/yeniden başlatma.
- Bozuk veya aşırı büyük HLS/DASH manifesti.
- Segment sırası ve eksik segment.
- 429/5xx retry ve backoff.
- Disk dolması ve yazma izni hatası.
- İptal sonrası geçici dosya/checkpoint tutarlılığı.
- Dosya adı çakışması ve overwrite politikası.

## 8.3 RUVIC test senaryoları

- FFmpeg bulunamadı.
- FFmpeg sürümü/capability parse.
- Probe JSON parse ve eksik alanlar.
- Geçerli remux.
- Geçersiz container/codec kombinasyonu.
- Transcode sırasında iptal.
- Encoder hatası ve stderr raporu.
- Aynı input/output path reddi.
- Bozuk output doğrulaması.
- Donanım encoder yokken CPU fallback veya açık hata politikası.

---

# 9. Geliştirme sırası ve MVP yol haritası

## Faz 0 — Workspace ve ortak temel

- Cargo workspace ve iki binary.
- `ruvi-common`, config, error, logging.
- CLI help/version/doctor iskeleti.
- CI: fmt, clippy, test.
- JSON sözleşmesi v1.

**Çıkış kriteri:** `ruvid --help`, `ruvic --help`, `cargo test --workspace` çalışır.

## Faz 1 — RUVIC temel medya motoru

- FFmpeg/ffprobe discovery.
- `probe`.
- `remux`.
- `convert` için tek temel preset.
- Temp output + output validation.
- Progress ve cancellation.

**Çıkış kriteri:** Küçük test dosyasında probe, remux ve H.264/AAC transcode başarılı.

## Faz 2 — RUVID doğrudan indirme

- HTTP client.
- Direct resolver.
- Tek akış indirme.
- Dosya adı ve atomic output.
- Progress, retry, timeout.
- JSON sonuç.

**Çıkış kriteri:** Test sunucusundan dosya güvenilir biçimde indirilir.

## Faz 3 — RUVID resume ve paralellik

- Range doğrulaması.
- Chunk scheduler.
- Checkpoint.
- Rate limit.
- Retry/backoff.
- İptal ve devam testleri.

**Çıkış kriteri:** Kontrollü kesinti sonrası doğru dosya bütünlüğüyle devam eder.

## Faz 4 — HLS / DASH

- HLS VOD.
- DASH VOD temel profili.
- Stream selection.
- Segment birleştirme.
- Eksik/bozuk segment davranışı.

**Çıkış kriteri:** Fixture setindeki desteklenen örnekler doğru çıktı verir.

## Faz 5 — RUVI entegrasyonu ve dağıtım

- RUVID manifest export.
- RUVIC manifest input (opsiyonel convenience).
- Windows x64 release.
- Linux/macOS CI.
- Kurulum ve FFmpeg bağımlılık belgesi.
- İmzalama ve checksum stratejisi.

**Çıkış kriteri:** Temiz Windows ortamında kurulum ve `ruvid -> ruvic` iş akışı belgelenmiş şekilde çalışır.

## Faz 6 — Platform adaptörleri ve gelişmiş özellikler

- Platform adapter API.
- Playlist.
- Subtitle.
- Batch conversion.
- Hardware acceleration.
- Preset yönetimi.

Platform adaptörleri temel HTTP/HLS/DASH çekirdeğinden ayrı tutulmalıdır.

---

# 10. Örnek uçtan uca kullanım

```powershell
# Kaynağı analiz et
ruvid info "https://example.com/media"

# İndir
ruvid get "https://example.com/media" -o ".\Downloads\source.mp4" --resume

# Dosyayı analiz et
ruvic probe ".\Downloads\source.mp4"

# Yeniden kodlamadan container değiştir
ruvic remux ".\Downloads\source.mp4" -o ".\Downloads\source.mkv"

# Web uyumlu MP4 üret
ruvic convert ".\Downloads\source.mkv" --preset web-mp4 -o ".\Exports\final.mp4"

# JSON handoff
ruvid get "https://example.com/media" -o ".\Downloads\source.mp4" --write-manifest ".\job.ruvi.json"
ruvic convert --manifest ".\job.ruvi.json" --preset web-mp4 -o ".\Exports\final.mp4"
```

`--manifest` ile otomatik input seçimi convenience özelliğidir; ilk sürümde zorunlu değildir. En basit ve sağlam entegrasyon dosya yolunu açıkça vermektir.

---

# 11. Mimari karar kayıtları (ADR)

## ADR-001: Tek workspace, iki bağımsız binary

**Karar:** Başlangıçta tek Cargo workspace.  
**Neden:** Ortak şema, test ve sürüm koordinasyonu kolaylaşır.  
**Alternatif:** İki ayrı repo. Bağımsız sürümleme ihtiyacı belirginleşince değerlendir.

## ADR-002: RUVIC'te FFmpeg subprocess backend

**Karar:** MVP'de Rust orkestrasyon + FFmpeg/ffprobe subprocess.  
**Neden:** Codec/container desteğini yeniden yazma maliyetini ve riskini önler.  
**Risk:** FFmpeg dağıtımı, lisans ve binary bulunabilirliği.  
**Azaltım:** Sistem FFmpeg'i kullanma veya lisansı doğrulanmış paketleme modeli.

## ADR-003: RUVID resolver ve downloader ayrımı

**Karar:** Resolver yalnızca kaynak/manifest çözümlemesi yapar; downloader planı yürütür.  
**Neden:** Platform adaptörleri ve protokol motoru birbirinden ayrılır.

## ADR-004: Manifest schema versioning

**Karar:** RUVID-RUVIC JSON handoff sürümlü şema kullanır.  
**Neden:** İki uygulama farklı sürümlerdeyken uyumluluk korunur.

---

# 12. Masaüstü kodlama ajanı için uygulama talimatı

Aşağıdaki talimatı kodlama ajanına verin:

1. Önce bu mimariyi oku; kod yazmaya başlamadan önce workspace, crate sınırları ve CLI sözleşmesini özetle.
2. Fazları sırayla uygula. Tüm özellikleri tek seferde üretmeye çalışma.
3. İlk committe yalnızca workspace, iki CLI iskeleti, ortak hata/config/logging ve CI oluştur.
4. `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` ve `cargo test --workspace` her aşamada çalışmalı.
5. Derlenmeyen örnek kod, sahte test veya uygulanmamış özellik varmış gibi gösterilen CLI komutu bırakma.
6. RUVID indirme kodunu RUVIC'e; RUVIC codec işlemlerini RUVID'e koyma.
7. FFmpeg komutlarını shell string'i olarak çalıştırma. Program ve argümanları ayrı ver.
8. DRM aşma, erişim kontrolü atlatma veya kullanıcı gizli bilgilerini loglama özelliği ekleme.
9. Her faz sonunda `docs/STATUS.md` dosyasını güncelle: tamamlananlar, testler, bilinen sorunlar ve sonraki işler.
10. Kullanıcı onayı olmadan geniş kapsamlı bağımlılık, lisans veya mimari değişikliği yapma.

**İlk ajan görevi:** Faz 0'ı tamamla; RUVID ve RUVIC için yalnızca çalışan CLI iskeleti üret. Medya indirme veya dönüştürme özelliğine henüz başlama.

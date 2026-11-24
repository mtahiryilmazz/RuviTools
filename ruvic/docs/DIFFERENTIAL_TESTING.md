# Diferansiyel "Oracle" Testleri (`ruvic`)

Bu belge, `ruvic` native motorunun çıktısını gerçek `ffmpeg`/`ffprobe` çıktısıyla karşılaştıran
**isteğe bağlı** test paketini açıklar.

> **Önemli:** `ruvic` bir FFmpeg alternatifidir ve çalışma zamanında hiçbir dış ikili çağırmaz.
> `ffmpeg`/`ffprobe` burada yalnızca bir **test oracle**'ıdır (bağımsız referans uygulama).
> Paket varsayılan olarak kapalıdır, normal `cargo test` çalıştırmasında hiçbir dış program
> aranmaz ve üretim ikilisine tek satır kod girmez.

## Neden

`tests/common/synthetic_media.rs` içindeki sentetik medya testleri hızlı ve hermetiktir, ancak
"bizim yazdığımız dosyayı bizim ayrıştırıcımız okuyor" döngüsünden çıkamaz. Gerçek dünyada
bozulan şeyler — `elst` uzunluğu, 64-bit `co64` sınırı, `30000/1001` zaman tabanı, 10+ saatlik
`timescale` taşmaları, `ctts` (B-frame) sırası — ancak **bağımsız bir okuyucuyla** yakalanır.

## Ne karşılaştırılıyor

| Test | `ruvic` tarafı | Oracle tarafı | Kapı (gate) |
| :--- | :--- | :--- | :--- |
| `diff_probe_matches_ffprobe` | `ruvic probe --json` | `ffprobe -show_format -show_streams` | süre ±50 ms, akış sayısı, codec, çözünürlük, örnekleme hızı, kanal |
| `diff_remux_preserves_sample_tables` | `ruvic remux` | `ffprobe` | süre ±20 ms, `nb_frames` birebir, çözme kapısı |
| `diff_trim_matches_stream_copy` | `ruvic trim --start 2 --duration 2` | `ffmpeg -ss 2 -t 2 -c copy` | uzunluk ±0.5 sn, iki akış da yaşıyor, `start_time ≈ 0` |
| `diff_dual_mux_rebuilds_dash_streams` | `ruvic remux --video --audio` | `ffprobe` | tam 1 video + 1 ses, süre ±150 ms, `nb_frames` birebir |
| `diff_ntsc_timescale_has_no_drift` | `ruvic remux` (30000/1001) | `ffprobe` | süre ±1 ms, kare sayısı birebir, `time_base` korunmuş |

**Çözme kapısı:** Her `ruvic` çıktısı ayrıca

```text
ffmpeg -v error -i <çıktı> -f null -
```

ile çözülür; çıkış kodu 0 olmalı ve `stderr` **tamamen boş** kalmalıdır. Yeniden inşa edilen
`stts`/`ctts`/`stsc`/`stco` tablosunda tek bir tutarsızlık varsa ffmpeg burada uyarı basar.

## Nasıl çalıştırılır

```powershell
cd ruvic

# Paket kapalıyken hiçbir şey yapmaz (varsayılan cargo test davranışı)
cargo test --test differential_ffmpeg_tests

# Oracle ile çalıştır
$env:RUVIC_DIFF="1"
cargo test --test differential_ffmpeg_tests -- --test-threads=1 --nocapture
```

Linux/macOS:

```bash
RUVIC_DIFF=1 cargo test --test differential_ffmpeg_tests -- --test-threads=1
```

### Ortam değişkenleri

| Değişken | Anlamı |
| :--- | :--- |
| `RUVIC_DIFF=1` | Paketi etkinleştirir (kapalıyken testler sessizce atlanır) |
| `RUVIC_DIFF_FFMPEG` | `ffmpeg` ikilisinin tam yolu (PATH'te yoksa) |
| `RUVIC_DIFF_FFPROBE` | `ffprobe` ikilisinin tam yolu |

Fixture'lar test sırasında `ffmpeg` ile üretilir (`lavfi` `testsrc2` + `sine`, libx264 ultrafast /
AAC, 320×240 — hızlı ve deterministik). Testler `ruvic` çağrılarını **boş `PATH`** ile yapar, böylece
native motordan sessizce ffmpeg arka ucuna düşme ihtimali ortadan kalkar.

## CI

`.github/workflows/ci.yml` içindeki `differential-ffmpeg` işi Ubuntu üzerinde `apt-get install ffmpeg`
yapıp `RUVIC_DIFF=1` ile paketi çalıştırır ve `cargo bench --no-run` ile benchmark hedeflerinin
derlendiğini doğrular. Diğer tüm işler hermetiktir; orada `ffmpeg` kurulmaz.

## Yeni bir vaka eklerken

1. Bozulma senaryosunu **önce** gerçek medyada yeniden üret (ffmpeg ile üretilmiş bir fixture veya
   `ffmpeg -movflags ...` ile üretilmiş bozuk/kenar dosya).
2. `Oracle` üzerinden referansı al, `ruvic` çıktısını üret.
3. Karşılaştırmayı toleranslı ama anlamlı yaz (süre ±ms, kare sayısı birebir).
4. Çözme kapısını **atlamayın**; asıl sinyal orada.
5. Testin adı `diff_` ile başlasın ve `#[test]` gövdesinde `require_oracle!()` olsun.

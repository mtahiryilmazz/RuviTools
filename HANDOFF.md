# RuviTools — Devam / Handoff Notu

Son güncelleme: 2026-11-13

## Mevcut Durum

Proje şu anda **tasarım ve mimari dokümantasyon** aşamasındadır. Henüz Rust workspace veya kaynak kodu oluşturulmamıştır.

Depodaki dosyalar:
- `RUVI_MASTER_ARCHITECTURE.md` — Monorepo, Cargo workspace, ortak sözleşmeler, RUVID/RUVIC mimarisi, ADR'ler ve Faz 0–6 yol haritası.
- `RUVID_ARCHITECTURE.md` — `ruvid` (Rust Video Downloader) teknik tasarımı ve MVP planı.
- `RUVIC_ARCHITECTURE.md` — `ruvic` (Rust Video Converter) teknik tasarımı ve MVP planı.
- `.gitignore` — Rust, build çıktıları, medya fixture'ları, editör ve ortam dosyaları için kurallar.
- `RuviTools.code-workspace` — VS Code workspace tanımı.

## Ortam Durumu

- `rustc`: `1.99.1 (58cf8f9c3 2026-10-13)`
- `cargo`: `1.99.1 (5f94df478 2026-08-27)`
- Git deposu başlatıldı ve ilk commit ile kaydedildi.

## Sonraki Oturumda Yapılacaklar (Faz 0)

`RUVI_MASTER_ARCHITECTURE.md` içindeki **Faz 0 — Workspace ve ortak temel** adımından devam edilecek:
1. Kök `Cargo.toml` (Cargo workspace) oluşturma.
2. Ortak crate'lerin iskeletini kurma (`crates/ruvi-core`, `crates/ruvi-manifest`, `crates/ruvi-cli-common`).
3. Uygulama crate'lerinin iskeletini kurma (`apps/ruvid`, `apps/ruvic`).
4. `cargo check` / `cargo test` ile workspace doğrulaması.

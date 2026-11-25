# Changelog

Tüm önemli değişiklikler bu dosyada tutulur. Biçim: [Keep a Changelog](https://keepachangelog.com/tr/1.1.0/), sürümleme: SemVer.

## [2.6.0] — 2026-11-25

### Düzeltildi
- MP3/WAV dönüştürme ilerlemesi native host üzerinden eklentiye aktarılır; uzun dönüşümler boyunca durum ve ilerleme görünür kalır.
- GitHub sürüm denetimi hem `main` manifestini hem yayımlanmış son Release'i karşılaştırır; yeni sürüm bulunduğunda masaüstü bildirimi gösterir.
- NativeHost, yalnızca RuviTools GitHub adreslerinden gelen ZIP'i doğrular, unpacked eklenti klasörünü yedekleyerek değiştirir ve başarıdan sonra eklentiyi yeniden yükler.
- Release paketi yoksa `main` kaynak arşivi kullanılır; All-in-One kurucusu güncelleme PowerShell yardımcısını NativeHost yanına kurar.
- Sürüm denetimi için `alarms`, `notifications`, `api.github.com` ve `raw.githubusercontent.com` izinleri eklendi.
- Release paketleyici eklenti ZIP adlarını manifest sürümünden üretir; All-in-One kurucusu yeni eklenti paketini indirir.

## [1.2.0] — 2026-11-13

### Eklendi
- **All-in-one GUI kurulum sihirbazı** (`installer/Install-GUI.ps1`, WinForms, Türkçe): Kur / Kaldır / günlük görüntüleme; adımlar canlı akar.
- **Eklentinin otomatik yüklenmesi:** Kurulum Chrome'u GeTube profiliyle açar ve `Extensions.loadUnpacked` (CDP) ile eklentiyi kendisi yükler — elle "Paketlenmemiş öğe yükle" gerekmez. Profil kalıcıdır, sonraki açılışlarda eklenti yüklü kalır.
- Masaüstü **GeTube kısayolu** (kendi Chrome profiliyle açar; günlük profiliniz etkilenmez).
- `tools/test-installed.ps1` (kurulu sistemle gerçek MP4+MP3 indirme testi), `tools/diag-chrome.mjs` (Chrome içi native messaging tanısı), `tools/cdp-load-unpacked.mjs`.

### Düzeltildi
- **`Add-Type` ile `ConsoleApplication` derlenemiyordu** (PowerShell 7/.NET Core): NativeHost.exe hiç üretilmiyor, bu yüzden "Native Host bağlantısı kurulamadı" hatası alınıyordu. Derleme artık `csc.exe` ile yapılıyor, `Add-Type` yalnızca yedek.
- **`COMSPEC` eksikliği:** Chrome, COMSPEC olmayan bir ortamdan başlatıldığında native host'u açamıyor ("Failed to start native messaging host"). Kurulum Chrome'u eksiksiz ortam değişkenleriyle başlatıyor.
- Deno kurulumu "doğrulanamadı" diye dosyayı siliyordu; artık sürüm kontrolü ayrı yapılıyor ve dosya korunuyor.
- Ortam değişkenleri boş olan kabuklarda `$env:LOCALAPPDATA` yüzünden kurulumun düşmesi giderildi (.NET `GetFolderPath` yedeği).

### Doğrulama
- Chrome içinden native messaging: `{"ok":true,"version":"1.2.0","port":37841}`.
- Kurulu sistemle gerçek indirmeler: `Me at the zoo [480p].mp4` (464 KB, 19.0 sn) ve `Me at the zoo [192kbps].mp3` (447 KB, 19.0 sn) → `Downloads\GeTube`.

## [1.1.0] — 2026-11-12

### Eklendi
- **Zaman aralığı (klip) indirme:** ✂ düğmesi ve kesit paneli; başlangıç/bitiş zamanı, `+15sn/+30sn/+1dk/+3dk` çipleri, canlı aralık özeti. MP4 ve MP3 olarak indirme (`--download-sections`, videoda `--force-keyframes-at-cuts`).
- **Kesit zamanı ayrıştırıcı** (`extension/shared/clip-time.js`): `83`, `1:23`, `01:23:45`, `1:23.5` biçimleri; 1 sn – 6 saat aralık doğrulaması.
- **Offscreen native köprüsü** (`offscreen.html/js`): native messaging portu service worker'dan bağımsız yaşar, MV3 30 sn idle sorunu ortadan kalkar.
- **Ayarlar sayfası** (`options.html/js`): indirme sonrası davranış, varsayılan kaliteler, varsayılan kesit süresi, yüzde göstergesi.
- **Popup** (`popup.html/js`): son işler, ilerleme, ayarlar ve İndirilenler kısayolu.
- **i18n**: `_locales/tr`, `_locales/en`; manifest `__MSG_extName__` / `__MSG_extDesc__`.
- Kuyruk: aynı anda en fazla 2 indirme, kalanlar `queued`.
- İş kayıtları `chrome.storage.session` üzerinde; service worker yeniden başlarsa kaybolmaz.
- `tools/verify.ps1`, `tools/package.ps1`, `tests/clip-time.test.mjs`.

### Değişti
- **Tek düğme arayüzü:** YouTube sayfasında yalnızca ✂ simgesi gösterilir; MP4/MP3 düğmeleri ve açılır menüler kaldırıldı. Tüm akış kesit panelinde: kalite seçimi, MP4/MP3 indirme ve "Tam videoyu indir".
- **Arayüz tamamen Türkçe:** metinler `_locales` yerine bileşenlere gömülü Türkçe sözlükten gelir; tarayıcı dili İngilizce olsa bile arayüz Türkçe kalır. Manifest adı/açıklaması sabit Türkçe.
- Ayarlar `localStorage` yerine `chrome.storage.local` (`getube.settings`) üzerinde; `chrome.storage.onChanged` ile tüm sekmeler eşitlenir.
- Dosya adı şablonu `%(title).100B [kalite]` — 260 karakter yol sınırına karşı başlık kırpılır.
- Kesit dosya adı: `... [clip 00-01-23--00-02-45].mp4` (Windows'ta yasak `:` yerine `-`).
- Native host indirmeyi ayrı iş parçacığında çalıştırır; stdin döngüsü kilitlenmez.

### Düzeltildi
- **[kullanıcı bildirimi]** Offscreen document içinde `chrome.i18n.getMessage` çağrılıyordu; offscreen yalnızca `chrome.runtime` kullanabildiğinden "Cannot read properties of undefined (reading 'getMessage')" hatasıyla kesit indirmesi yıkılıyordu. Metinler offscreen'de sabit tutulmaya alındı ve `tools/verify.ps1` bu sınıf hatayı yakalayan "offscreen API sınırı" kontrolü kazandı.
- `NativeHost.Q()` JSON kaçışı: kontrol karakterleri, tırnak ve ters bölü artık `\u00XX` / `\"` / `\\` olarak yazılır (önceki sürümde geçersiz JSON üretilebiliyordu).
- Windows komut satırı kaçışı (CommandLineToArgvW kuralları) uygulanır.
- URL deseni ve kalite değerleri beyaz liste ile doğrulanır.
- yt-dlp 30 dakikada bitmezse süreç ağacı sonlandırılır.
- `nativehost.log` 2 MB'ta döner; sınırsız büyüme durur.
- İndirme token'ı 10 dakika TTL ve tek kullanımlık; staging'de biriken dosyalar temizlenir.
- Kurulumda Deno hatası artık kurulumu durdurmaz, uyarı olarak loglanır.
- ffprobe.exe de kurulur; v1.0.8 yalnızca ffmpeg.exe kopyaladığı için `-x` (MP3) zinciri kırılgan olabiliyordu.
- Arayüz, seçilen bitiş video süresini aşıyorsa bitişi süreye kırpar ve "(kırpılacak)" notu gösterir.
- `tabs` izni kaldırıldı (kullanılmıyordu); manifest artık nativeMessaging, downloads, storage, offscreen ile sınırlı.
- Native host argümanlarında URL'den önce `--` kullanılır.
- Her iş kendi staging klasörünü kullanır (`staging/<jobId>`); eski dosyanın servis edilmesi riski kalkar.
- "Dosyayı aç / klasörü aç" artık `chrome.downloads.open` / `show` ile yapılır; Chrome indirme klasörü değiştirilmiş olsa da çalışır.
- 40 dakikadır güncellenmeyen işler `failed` olarak işaretlenir (takılı düğme kalmaz).
- **[gerçek testte bulundu]** `Send()` her mesajda stdout handle'ını kapatıyordu; native messaging kanalı kopuyordu. Stream artık bir kez açılıp yeniden kullanılıyor.
- **[gerçek testte bulundu]** yt-dlp alt süreci native messaging stdin'ini miras alıyordu ve sessizce bekliyordu (indirme hiç başlamıyordu). Alt süreç için stdin ayrıca yönlendirilip kapatılıyor.
- **[gerçek testte bulundu]** MP3 kesiti `--download-sections` ile 10 sn'ye kadar kayıyordu (40 sn istendi, 50 sn geldi). Ses kesiti artık tam ses indirilip yerel ffmpeg ile yapılıyor (ölçüm: 40.03 sn).
- yt-dlp çıktısı `PYTHONUNBUFFERED=1` ile canlı okunur; ilerleme yüzdesi artık indirme sırasında akıyor (öncesinde yalnızca süreç bitince görünüyordu).

### Kaldırıldı
- `content.js` içindeki `localStorage` tabanlı ayar anahtarları (`yt_dl_open_downloads` vb.) — v1.0.8 ayarları ilk açılışta varsayılana döner.

### Notlar
- v1.0.8 klasörü değiştirilmedi; geri dönüş için kurulumu eski klasörden tekrar çalıştırmak yeterlidir.
- Manuel doğrulama gerektiren testler: gerçek klip indirmesi (T8), service worker ömrü (T9), kurulum (T10). Ayrıntı: `../WORKFLOW_ROADMAP.md`.

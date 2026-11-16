GeTube v1.2.0 — tek tıkla kurulum (GUI), otomatik Chrome yükleme, tek ✂ arayüzü

Bu sürüm, v1.0.8'in çalışan indirme hattını korur ve üç büyük yetenek ekler:
seçilen zaman aralığını MP4/MP3 olarak indirme, MV3 service worker ömrüne dayanıklı
native messaging köprüsü, ve kalıcı ayarlar + çok dilli arayüz.

YENİLER
- Zaman aralığı (klip) indirme: video sayfasındaki tek ✂ düğmesi kesit panelini açar`n  (MP4/MP3 düğmeleri kaldırıldı; kalite seçimi ve tam indirme de panelde).
  Başlangıç/bitiş zamanı "1:23" veya "01:23:45" biçiminde girilir; "+15sn, +30sn,
  +1dk, +3dk" çipleri bitişi hesaplar. Aynı panelden MP4 veya MP3 olarak indirilir.
  İndirilen dosya: "Başlık [720p] [clip 00-01-23--00-02-45].mp4"
- Kesit indirme videoda yt-dlp'nin --download-sections seçeneği ile yapılır;
  --force-keyframes-at-cuts kullanıldığı için kesim hassastır (yeniden kodlama).
- MP3 kesitinde önce tam ses indirilir, kesim yerel ffmpeg ile yapılır. DASH ses
  akışında --download-sections kesimi kayıyordu (40 sn istendi, 50 sn geliyordu).
  Ölçülen sonuç: 40.03 sn.
- Native messaging bağlantısı artık offscreen document üzerinde tutuluyor; service
  worker 30 saniye boşta kalıp sonlandırılsa bile indirme kopmuyor.
- İş kayıtları chrome.storage.session üzerinde: iş listesi service worker yeniden
  başladığında kaybolmuyor. Aynı anda en fazla 2 indirme çalışır, kalanlar kuyruğa girer.
- Ayarlar sayfası (chrome://extensions -> GeTube -> Uzantı seçenekleri):
  indirme sonrası davranış (dosyayı aç / klasörü aç / İndirilenler sayfası),
  varsayılan kaliteler, varsayılan kesit süresi, yüzde göstergesi.
- Ayarlar artık localStorage yerine chrome.storage.local üzerinde; tüm YouTube
  sekmeleri aynı ayarı görür.
- Arayüz tamamen Türkçedir; tarayıcı dili İngilizce olsa bile Türkçe gösterir.
- Popup: son işler, ilerleme, ayarlar ve İndirilenler kısayolları.
- Düğme üzerinde yüzde göstergesi ve hız/ETA ipucu (title) korunur.

NATIVE HOST (1.1.0)
- JSON üreteci kaçış tablosu düzeltildi: tırnak, ters bölü, satır sonu, sekme ve
  0x20 altı karakterler \u00XX olarak yazılır. Önceki sürümde kontrol karakterleri
  içeren yt-dlp hata mesajları geçersiz JSON üretebiliyordu.
- Gelen alanlar doğrulanır: URL yalnızca https://www.youtube.com/watch?... desenine
  uyarsa kabul edilir; kalite beyaz listeden seçilir; kesit zamanı 0 <= baş < bit,
  en fazla 6 saat ve 24 saat sınırı içinde olmalıdır.
- Windows komut satırı kaçışı (CommandLineToArgvW kuralları) uygulanır; kullanıcı
  girdisi argümanı bölemez.
- yt-dlp 30 dakika içinde bitmezse süreç ağacı taskkill /T /F ile kapatılır.
- İndirme ayrı iş parçacığında çalışır: indirme sürerken "klasörü aç" gibi istekler
  aynı host sürecinde kilitlenmez.
- Log rotasyonu: nativehost.log 2 MB'ı geçince nativehost.log.1 olarak döner.
- İndirme token'ı 10 dakika geçerlidir ve tek kullanımlıktır; süresi geçen
  staging dosyaları temizlenir. Açılışta 24 saatten eski geçici dosyalar silinir.
- Dosya adı şablonu %(title).100B ile kırpılır; 260 karakter yol sınırı korunur.

ALL-IN-ONE KURULUM (v1.2.0)
- installer\Kurulum.cmd tek tıkla GUI sihirbazını açar (Türkçe): Kur / Kaldır / günlük.
- Sihirbaz sırayla: yt-dlp, Deno, FFmpeg + ffprobe indirir; NativeHost.exe'yi derler;
  native messaging kaydını yazar ve doğrular; eklentiyi %LOCALAPPDATA%\GeTube\extension
  klasörüne kopyalar; Chrome'u "GeTube" profiliyle açar ve eklentiyi CDP
  (Extensions.loadUnpacked) ile OTOMATİK yükler; masaüstüne GeTube kısayolu koyar.
- GUI çalışmazsa installer\Kurulum-Konsol.cmd aynı işi konsolda yapar.
- Chrome 136+ varsayılan profilde hata ayıklamayı engellediği için GeTube kendi
  profilini kullanır (masaüstündeki GeTube kısayolu). Günlük Chrome profiliniz etkilenmez.
- Native messaging Windows'ta COMSPEC ortam değişkenine ihtiyaç duyar; Chrome bu
  değişken olmadan açılırsa "Failed to start native messaging host" hatası verir.
  Kurulum artık Chrome'u eksiksiz ortamla başlatır.

KURULUM / GÜNCELLEME
1. ZIP dosyasını tamamen çıkarın (v1.0.8 klasörünün üzerine yazmayın).
2. installer\Kurulum.cmd dosyasını çalıştırın. İnternet bağlantısı gerekir.
3. Kurulum yt-dlp'yi günceller, FFmpeg ve Deno'yu kontrol eder, NativeHost.exe'yi
   derler ve native messaging kaydını doğrular.
4. Chrome'da chrome://extensions -> Geliştirici modu -> "Paketlenmemiş öğe yükle"
   ve %LOCALAPPDATA%\GeTube\extension klasörünü gösterin.
5. YouTube sekmesini yenileyin. Video başlığının yanında MP4 / MP3 / ✂ düğmeleri görünür.

Not: MP3 dönüşümü ffprobe.exe de gerektirir. Kurulum ffmpeg arşivinden ffmpeg.exe ve
ffprobe.exe dosyalarını birlikte kopyalar (v1.0.8 yalnızca ffmpeg.exe kopyalıyordu).

KALDIRMA
installer\Kaldir.cmd — kayıt defteri girdisini, uygulama klasörünü ve çalışan host
süreçlerini temizler; indirilen dosyaları silmek için sorar.

BİLİNEN SINIRLAR
- Kesit, videoda yeniden kodlama gerektirir (--force-keyframes-at-cuts); uzun
  kesitler tam indirmeden daha yavaş olabilir.
- Seçilen bitiş video süresini aşıyorsa arayüz bitişi video süresine kırpar ve
  "(kırpılacak)" notu gösterir; yt-dlp de aynı şekilde davranır.
- Canlı yayınlarda zaman aralığı desteklenmez.
- Seçilen aralık videonun süresini aşıyorsa yt-dlp aralığı kırpar; dosya süresi
  beklenenden kısa olur.
- Deno kurulamazsa bazı YouTube indirmeleri başarısız olabilir; kurulum bunu
  uyarı olarak loglar (v1.0.8'de kurulum duruyordu).
- Chrome Web Store dağıtımı bu sürümün kapsamında değildir.

DOĞRULAMA
- Statik doğrulama: pwsh -File tools\verify.ps1 (manifest, JSON, JS sözdizimi,
  i18n anahtar eşitliği, PowerShell parse, C# derleme öncesi yapısal kontroller).
- Birim testleri: node tests\clip-time.test.mjs (36 kontrol).
- Native host protokolü: pwsh -File tools\test-native-host.ps1 (5 senaryo, derleme dahil).
- Gerçek klip indirmesi (ağ gerekir): pwsh -File tools\test-clip-download.ps1
  [-Type MP3] [-Start 30] [-End 70]. MP4 ve MP3 ksitleri bu makinede doğrulandı.
- Chrome 157 ve üzeri --load-extension bayrağını yok sayar; otomatik tarayıcı testi
  bu sürümde yapılamaz, eklenti chrome://extensions üzerinden elle yüklenir.
- Gerçek indirme provası (Chrome + Windows) bu paketle birlikte elle yapılmalıdır;
  kesit süresi doğrulaması için 01:23–02:45 aralığı önerilir.

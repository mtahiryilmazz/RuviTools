# GeTube — başlatıcı: Chrome'u GeTube profiliyle açar ve eklentiyi yükler.
# Masaüstü kısayolu bu dosyayı çağırır. Chrome, unpacked eklentileri her açılışta
# kendiliğinden yüklemediği için (developer mode kapalıysa) eklenti CDP ile yüklenir.
# UTF-8 BOM
$ErrorActionPreference = 'Continue'

. (Join-Path $PSScriptRoot 'install-core.ps1')

function Invoke-Log {
  param([string]$Message)
  $line = "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $Message"
  try { Add-Content -LiteralPath (Join-Path $AppDir 'launch.log') -Value $line -Encoding UTF8 } catch {}
}

try {
  if (!(Test-Path (Join-Path $ExtDir 'manifest.json'))) { throw "Eklenti bulunamadı: $ExtDir (kurulumu çalıştırın)" }

  Stop-GeTubeChrome
  Enable-DeveloperMode | Out-Null

  $session = Start-GeTubeChrome -ExtraArgs @('https://www.youtube.com')
  Invoke-Log ("Chrome açıldı, port {0}" -f $session.Port)

  try {
    Invoke-CdpCommand -Port $session.Port -WsPath $session.WsPath -Message @{
      id = 1; method = 'Extensions.loadUnpacked'; params = @{ path = $ExtDir }
    } | Out-Null
    Invoke-Log 'Eklenti yüklendi (CDP).'
  } catch {
    if ($_.Exception.Message -match 'already|loaded|exists') { Invoke-Log 'Eklenti zaten yüklüydü.' }
    else { Invoke-Log ("Eklenti yüklenemedi: " + $_.Exception.Message) }
  }

  if (Test-ExtensionLoaded -Port $session.Port -TimeoutSec 15) { Invoke-Log 'Doğrulama: eklenti yüklü.' }
  else { Invoke-Log 'Doğrulama: eklenti görünmedi.' }
} catch {
  Invoke-Log ("BAŞLATMA HATASI: " + $_.Exception.Message)
  # Son çare: eklentisiz de olsa Chrome'u aç.
  $chrome = Find-Chrome
  if ($chrome) { Start-Process -FilePath $chrome -ArgumentList "--user-data-dir=`"$ChromeProfileDir`" --no-first-run https://www.youtube.com" | Out-Null }
}

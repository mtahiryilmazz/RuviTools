# RuviTube — All-in-One PowerShell Kurulumu (irm | iex uyumlu)
# UTF-8 BOM

$ErrorActionPreference = 'Stop'
try {
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
} catch {}

Write-Host ''
Write-Host 'RuviTube All-in-One Kurulumu baslatiliyor...' -ForegroundColor Cyan

$AppDir       = Join-Path $env:LOCALAPPDATA 'RuviTube'
$ExtDir       = Join-Path $AppDir 'extension'
$NativeExe    = Join-Path $AppDir 'NativeHost.exe'
$HostName     = 'com.ruvitube.nativehost'
$ExtId        = 'npbogohemcamnnhpmbnialffkfkbeimn'
$FirefoxExtId = 'ruvitube@ruvitools.dev'
$BaseUrl      = 'https://github.com/mtahiryilmazz/RuviTools/releases/latest/download'
$Tools        = @('NativeHost.exe', 'ruvid.exe', 'ruvic.exe')

New-Item -ItemType Directory -Force -Path $AppDir | Out-Null

foreach ($tool in $Tools) {
  $url = "$BaseUrl/$tool"
  $dst = Join-Path $AppDir $tool
  $tmp = "$dst.download"
  Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
  Write-Host ("Indiriliyor: {0} -> {1}" -f $tool, $dst) -ForegroundColor Cyan
  $wc = New-Object System.Net.WebClient
  $wc.Headers['User-Agent'] = 'RuviTube-AllInOne/2.4.0'
  try {
    $wc.DownloadFile($url, $tmp)
  } finally {
    $wc.Dispose()
  }
  if (-not (Test-Path -LiteralPath $tmp) -or (Get-Item -LiteralPath $tmp).Length -lt 1024) {
    Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    throw "Indirilen dosya gecersiz: $tool"
  }
  Move-Item -LiteralPath $tmp -Destination $dst -Force
  try { Unblock-File -LiteralPath $dst -ErrorAction SilentlyContinue } catch {}
  Write-Host ("[OK] {0} kuruldu." -f $tool) -ForegroundColor Green
}

# Eklenti paketini indir ve %LOCALAPPDATA%\RuviTube\extension altina ac
$extZipUrl = "$BaseUrl/RuviTube-v2.4.0-chrome.zip"
$extZipTmp = Join-Path $AppDir 'RuviTube-chrome.zip.download'
try {
  Remove-Item -LiteralPath $extZipTmp -Force -ErrorAction SilentlyContinue
  Write-Host ("Indiriliyor: RuviTube-v2.4.0-chrome.zip -> {0}" -f $ExtDir) -ForegroundColor Cyan
  $wc = New-Object System.Net.WebClient
  $wc.Headers['User-Agent'] = 'RuviTube-AllInOne/2.4.0'
  try {
    $wc.DownloadFile($extZipUrl, $extZipTmp)
  } finally {
    $wc.Dispose()
  }
  if ((Test-Path -LiteralPath $extZipTmp) -and (Get-Item -LiteralPath $extZipTmp).Length -ge 1024) {
    if (Test-Path -LiteralPath $ExtDir) {
      Remove-Item -LiteralPath $ExtDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    New-Item -ItemType Directory -Force -Path $ExtDir | Out-Null
    Expand-Archive -LiteralPath $extZipTmp -DestinationPath $ExtDir -Force
    Write-Host ("[OK] Eklenti dosyalari hazir: {0}" -f $ExtDir) -ForegroundColor Green
  }
} catch {
  Write-Host ("UYARI: Eklenti ZIP indirilemedi ({0}). Native Host kaydi yine de tamamlaniyor." -f $_.Exception.Message) -ForegroundColor Yellow
} finally {
  Remove-Item -LiteralPath $extZipTmp -Force -ErrorAction SilentlyContinue
}

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$chromeManifest = Join-Path $AppDir ($HostName + '.json')
$chromeJson = @"
{
  "name": "$HostName",
  "description": "RuviTube native host",
  "path": "$($NativeExe.Replace('\', '\\'))",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://$ExtId/"]
}
"@
[IO.File]::WriteAllText($chromeManifest, $chromeJson, $utf8NoBom)

$ffManifest = Join-Path $AppDir ($HostName + '.firefox.json')
$ffJson = @"
{
  "name": "$HostName",
  "description": "RuviTube native host",
  "path": "$($NativeExe.Replace('\', '\\'))",
  "type": "stdio",
  "allowed_extensions": ["$FirefoxExtId"]
}
"@
[IO.File]::WriteAllText($ffManifest, $ffJson, $utf8NoBom)

$chromiumRegs = @(
  "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName",
  "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$HostName",
  "HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\$HostName",
  "HKCU:\Software\Vivaldi\NativeMessagingHosts\$HostName"
)
foreach ($reg in $chromiumRegs) {
  New-Item -Path $reg -Force | Out-Null
  New-ItemProperty -Path $reg -Name '(Default)' -Value $chromeManifest -PropertyType String -Force | Out-Null
}

$ffReg = "HKCU:\Software\Mozilla\NativeMessagingHosts\$HostName"
New-Item -Path $ffReg -Force | Out-Null
New-ItemProperty -Path $ffReg -Name '(Default)' -Value $ffManifest -PropertyType String -Force | Out-Null

Write-Host ''
Write-Host 'Kurulum tamamlandi!' -ForegroundColor Green
Write-Host ("Eklenti klasoru: {0}" -f $ExtDir) -ForegroundColor Cyan
Write-Host 'Chrome / Edge / Brave uzerinde chrome://extensions -> Gelistirici modu -> Paketlenmemis oge yukle adimi ile bu klasoru secebilirsiniz.' -ForegroundColor White
Write-Host ''

@echo off
chcp 65001 >nul
setlocal
title RuviTube All-in-One Kurulumu
echo.
echo   RuviTube All-in-One Kurulumu baslatiliyor...
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$c = (Get-Content -LiteralPath '%~f0' -Raw -Encoding UTF8) -split '::___PS_BEGIN___\r?\n', 2; if ($c.Count -lt 2) { throw 'Kurulum betigi okunamadi.' }; Invoke-Expression $c[1]"
set "RC=%ERRORLEVEL%"
echo.
pause
exit /b %RC%

::___PS_BEGIN___
$ErrorActionPreference = 'Stop'
try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13 } catch {}

$AppDir       = Join-Path $env:LOCALAPPDATA 'RuviTube'
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
  $wc.Headers['User-Agent'] = 'RuviTube-AllInOne/2.2.0'
  try {
    $wc.DownloadFile($url, $tmp)
  } finally {
    $wc.Dispose()
  }
  if (!(Test-Path -LiteralPath $tmp) -or (Get-Item -LiteralPath $tmp).Length -lt 1024) {
    Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    throw "Indirilen dosya gecersiz: $tool"
  }
  Move-Item -LiteralPath $tmp -Destination $dst -Force
  try { Unblock-File -LiteralPath $dst -ErrorAction SilentlyContinue } catch {}
  Write-Host ("[OK] {0} kuruldu." -f $tool) -ForegroundColor Green
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
Write-Host 'Kurulum tamamlandi! Tarayiciyi yeniden baslatmadan RuviTube''u hemen kullanabilirsiniz.' -ForegroundColor Green

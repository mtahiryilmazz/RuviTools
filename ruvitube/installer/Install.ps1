# RuviTube — Konsol Kurulum Betiği (Saf Rust ruvid + ruvic + NativeHost)
# Kurulum.cmd tarafından GUI başarısız olursa veya doğrudan konsoldan çağrılır.
# UTF-8 BOM

[CmdletBinding()]
param(
  [switch]$SkipAutoLoad
)

$ErrorActionPreference = 'Stop'

$corePath = Join-Path $PSScriptRoot 'install-core.ps1'
if (-not (Test-Path -LiteralPath $corePath -PathType Leaf)) {
  Write-Error "Çekirdek kurulum dosyası bulunamadı: $corePath"
  exit 1
}

. $corePath

Write-Host '============================================================' -ForegroundColor Cyan
Write-Host ' RuviTube — Saf Rust (ruvid + ruvic + NativeHost) Kurulumu' -ForegroundColor Cyan
Write-Host '============================================================' -ForegroundColor Cyan
Write-Host "Hedef klasör   : $AppDir"
Write-Host "Eklenti kimliği: $ExtId"
Write-Host ''

try {
  $result = Invoke-Install -SkipAutoLoad:$SkipAutoLoad

  Write-Host ''
  if ($result -and $result.AutoLoaded) {
    Write-Host 'KURULUM TAMAMLANDI: Eklenti Chrome RuviTube profiline otomatik yüklendi.' -ForegroundColor Green
  } else {
    Write-Host 'KURULUM TAMAMLANDI: Motor ve Native Messaging kaydı kuruldu.' -ForegroundColor Yellow
    Write-Host "Chrome içinde elle yükleme: chrome://extensions -> Geliştirici modu -> Paketlenmemiş öğe yükle -> $ExtDir" -ForegroundColor Yellow
  }
  exit 0
} catch {
  Write-Host ''
  Write-Host "HATA: Kurulum başarısız: $($_.Exception.Message)" -ForegroundColor Red
  if ($Log -and (Test-Path -LiteralPath $Log)) {
    Write-Host "Ayrıntılar için kurulum günlüğü: $Log" -ForegroundColor DarkGray
  }
  exit 1
}

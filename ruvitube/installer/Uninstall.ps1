# RuviTube — Konsol Kaldırma Betiği
# Kaldir.cmd tarafından veya doğrudan konsoldan çağrılır.
# UTF-8 BOM

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$corePath = Join-Path $PSScriptRoot 'install-core.ps1'
if (-not (Test-Path -LiteralPath $corePath -PathType Leaf)) {
  Write-Error "Çekirdek kurulum dosyası bulunamadı: $corePath"
  exit 1
}

. $corePath

Write-Host '============================================================' -ForegroundColor Cyan
Write-Host ' RuviTube — Kaldırma İşlemi' -ForegroundColor Cyan
Write-Host '============================================================' -ForegroundColor Cyan
Write-Host "Hedef klasör: $AppDir"
Write-Host ''

try {
  Invoke-Uninstall
  Write-Host ''
  Write-Host 'RuviTube başarıyla kaldırıldı.' -ForegroundColor Green
  exit 0
} catch {
  Write-Host ''
  Write-Host "HATA: Kaldırma başarısız: $($_.Exception.Message)" -ForegroundColor Red
  exit 1
}

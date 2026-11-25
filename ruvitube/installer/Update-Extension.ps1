param(
  [Parameter(Mandatory = $true)][string]$Version,
  [Parameter(Mandatory = $true)][string]$Url
)

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw 'Güncelleme sürümü geçersiz.'
}

$sourceUrl = 'https://github.com/mtahiryilmazz/RuviTools/archive/refs/heads/main.zip'
$releaseUrl = "https://github.com/mtahiryilmazz/RuviTools/releases/download/v$Version/RuviTube-v$Version-chrome.zip"
if (($Url -cne $sourceUrl) -and ($Url -cne $releaseUrl)) {
  throw 'Güncelleme adresi RuviTools GitHub deposuna ait değil.'
}

$appDir = [IO.Path]::GetFullPath($PSScriptRoot)
$extensionDir = [IO.Path]::GetFullPath((Join-Path $appDir 'extension'))
$workDir = [IO.Path]::GetFullPath((Join-Path $appDir ('.update-' + [guid]::NewGuid().ToString('N'))))
$stageDir = [IO.Path]::GetFullPath((Join-Path $appDir ('.extension-stage-' + [guid]::NewGuid().ToString('N'))))
$backupDir = [IO.Path]::GetFullPath((Join-Path $appDir ('.extension-backup-' + [guid]::NewGuid().ToString('N'))))
$prefix = $appDir.TrimEnd('\') + '\'

foreach ($candidate in @($extensionDir, $workDir, $stageDir, $backupDir)) {
  if (-not $candidate.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Güncelleme yolu RuviTube kurulum klasörünün dışına çıkıyor.'
  }
}

$movedOldExtension = $false
try {
  New-Item -ItemType Directory -Path $workDir | Out-Null
  $zipPath = Join-Path $workDir 'update.zip'
  Invoke-WebRequest -Uri $Url -OutFile $zipPath -UseBasicParsing
  if (-not (Test-Path -LiteralPath $zipPath) -or (Get-Item -LiteralPath $zipPath).Length -lt 1024) {
    throw 'GitHub güncelleme arşivi boş veya eksik.'
  }

  $extractDir = Join-Path $workDir 'unpacked'
  Expand-Archive -LiteralPath $zipPath -DestinationPath $extractDir -Force
  $manifestCandidates = @(Get-ChildItem -LiteralPath $extractDir -Filter 'manifest.json' -File -Recurse)
  $manifestFile = $manifestCandidates | Where-Object {
    $_.FullName -match '[\\/]ruvitube[\\/]extension[\\/]manifest\.json$'
  } | Select-Object -First 1
  if (-not $manifestFile) {
    $manifestFile = $manifestCandidates | Where-Object {
      $_.DirectoryName -ne $null -and $_.DirectoryName.StartsWith($extractDir, [StringComparison]::OrdinalIgnoreCase)
    } | Select-Object -First 1
  }
  if (-not $manifestFile) { throw 'Arşivde eklenti manifest.json dosyası bulunamadı.' }

  $sourceExtensionDir = $manifestFile.DirectoryName
  $manifest = Get-Content -LiteralPath $manifestFile.FullName -Raw | ConvertFrom-Json
  if ([string]$manifest.version -cne $Version) {
    throw "Arşiv sürümü ($($manifest.version)) istenen sürümle ($Version) eşleşmiyor."
  }

  New-Item -ItemType Directory -Path $stageDir | Out-Null
  Copy-Item -Path (Join-Path $sourceExtensionDir '*') -Destination $stageDir -Recurse -Force
  $stagedManifest = Get-Content -LiteralPath (Join-Path $stageDir 'manifest.json') -Raw | ConvertFrom-Json
  if ([string]$stagedManifest.version -cne $Version) {
    throw 'Hazırlanan eklenti sürümü doğrulanamadı.'
  }

  if (Test-Path -LiteralPath $extensionDir) {
    Move-Item -LiteralPath $extensionDir -Destination $backupDir
    $movedOldExtension = $true
  }
  try {
    Move-Item -LiteralPath $stageDir -Destination $extensionDir
  } catch {
    if ($movedOldExtension -and -not (Test-Path -LiteralPath $extensionDir)) {
      Move-Item -LiteralPath $backupDir -Destination $extensionDir
      $movedOldExtension = $false
    }
    throw
  }

  if ($movedOldExtension -and (Test-Path -LiteralPath $backupDir)) {
    Remove-Item -LiteralPath $backupDir -Recurse -Force
    $movedOldExtension = $false
  }
  Write-Output "RuviTube extension v$Version installed."
} finally {
  if (Test-Path -LiteralPath $stageDir) { Remove-Item -LiteralPath $stageDir -Recurse -Force }
  if (Test-Path -LiteralPath $workDir) { Remove-Item -LiteralPath $workDir -Recurse -Force }
}

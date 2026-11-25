# RuviTools — Unified Windows Release Packager & WiX v7 MSI Builder
# UTF-8 BOM

[CmdletBinding()]
param(
  [string]$Version = '0.3.0',
  [switch]$WithMimalloc,
  [switch]$SkipBuild,
  [switch]$SkipMsi
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$DistDir = Join-Path $RepoRoot 'dist'
$StagingDir = Join-Path $DistDir 'staging'
$BinStage = Join-Path $StagingDir 'bin'
$ExtStage = Join-Path $StagingDir 'extension'
$PortableStage = Join-Path $StagingDir 'portable'
$RuviTubeBin = Join-Path $RepoRoot 'ruvitube\bin'
$ExtensionVersion = (Get-Content -LiteralPath (Join-Path $RepoRoot 'ruvitube\extension\manifest.json') -Raw | ConvertFrom-Json).version

$Crates = @(
  @{ Name = 'ruvid'; Bin = 'ruvid.exe' },
  @{ Name = 'ruvic'; Bin = 'ruvic.exe' },
  @{ Name = 'ruvi-native-host'; Bin = 'NativeHost.exe' },
  @{ Name = 'ruvi-gui'; Bin = 'ruvi-studio.exe' }
)

Write-Host "RuviTools Release Packager v$Version" -ForegroundColor Cyan

if (-not $SkipBuild) {
  foreach ($c in $Crates) {
    $manifest = Join-Path $RepoRoot "$($c.Name)\Cargo.toml"
    $cargoArgs = @('build', '--release', '--manifest-path', $manifest)
    if ($WithMimalloc) {
      $cargoArgs += @('--features', 'mimalloc')
    }
    Write-Host ("Building {0} (release{1})..." -f $c.Name, $(if ($WithMimalloc) { ' + mimalloc' } else { '' })) -ForegroundColor Yellow
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
      throw "cargo build failed for $($c.Name) (exit $LASTEXITCODE)"
    }
  }
}

# Prepare clean staging & dist layout
if (Test-Path -LiteralPath $StagingDir) {
  Remove-Item -LiteralPath $StagingDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $BinStage, $ExtStage, $PortableStage, $RuviTubeBin | Out-Null

# Collect compiled binaries and sync ruvitube/bin
foreach ($c in $Crates) {
  $builtExe = Join-Path $RepoRoot "$($c.Name)\target\release\$($c.Bin)"
  if (-not (Test-Path -LiteralPath $builtExe -PathType Leaf)) {
    throw "Expected release binary not found: $builtExe"
  }
  Copy-Item -LiteralPath $builtExe -Destination (Join-Path $BinStage $c.Bin) -Force
}

foreach ($tool in @('NativeHost.exe', 'ruvid.exe', 'ruvic.exe')) {
  $src = Join-Path $BinStage $tool
  $dst = Join-Path $RuviTubeBin $tool
  Copy-Item -LiteralPath $src -Destination $dst -Force
  $hSrc = (Get-FileHash -LiteralPath $src -Algorithm SHA256).Hash
  $hDst = (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash
  if ($hSrc -ne $hDst) {
    throw "SHA256 mismatch syncing $tool to ruvitube/bin"
  }
  Write-Host "[SYNC] ruvitube/bin/$tool SHA256=$hDst" -ForegroundColor Green
}

# Write Native Messaging manifests for MSI installation (relative path resolves beside NativeHost.exe)
$chromeHostManifest = @'
{
  "name": "com.ruvitube.nativehost",
  "description": "RuviTube native host",
  "path": "NativeHost.exe",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://npbogohemcamnnhpmbnialffkfkbeimn/"]
}
'@
$firefoxHostManifest = @'
{
  "name": "com.ruvitube.nativehost",
  "description": "RuviTube native host",
  "path": "NativeHost.exe",
  "type": "stdio",
  "allowed_extensions": ["ruvitube@ruvitools.dev"]
}
'@
[IO.File]::WriteAllText((Join-Path $BinStage 'com.ruvitube.nativehost.json'), $chromeHostManifest, (New-Object Text.UTF8Encoding($false)))
[IO.File]::WriteAllText((Join-Path $BinStage 'com.ruvitube.nativehost.firefox.json'), $firefoxHostManifest, (New-Object Text.UTF8Encoding($false)))

# Stage extension files
Copy-Item -Path (Join-Path $RepoRoot 'ruvitube\extension\*') -Destination $ExtStage -Recurse -Force

# Build Chrome extension zip (excludes manifest.firefox.json)
$chromeExtStage = Join-Path $StagingDir 'ext-chrome'
New-Item -ItemType Directory -Force -Path $chromeExtStage | Out-Null
Copy-Item -Path (Join-Path $ExtStage '*') -Destination $chromeExtStage -Recurse -Force
Remove-Item -LiteralPath (Join-Path $chromeExtStage 'manifest.firefox.json') -Force -ErrorAction SilentlyContinue
$chromeZip = Join-Path $DistDir "RuviTube-v$ExtensionVersion-chrome.zip"
if (Test-Path -LiteralPath $chromeZip) { Remove-Item -LiteralPath $chromeZip -Force }
Compress-Archive -Path (Join-Path $chromeExtStage '*') -DestinationPath $chromeZip -CompressionLevel Optimal

# Build Firefox extension zip (promotes manifest.firefox.json -> manifest.json)
$firefoxExtStage = Join-Path $StagingDir 'ext-firefox'
New-Item -ItemType Directory -Force -Path $firefoxExtStage | Out-Null
Copy-Item -Path (Join-Path $ExtStage '*') -Destination $firefoxExtStage -Recurse -Force
$ffManifestSrc = Join-Path $firefoxExtStage 'manifest.firefox.json'
if (-not (Test-Path -LiteralPath $ffManifestSrc -PathType Leaf)) {
  throw "Missing manifest.firefox.json in $firefoxExtStage"
}
Move-Item -LiteralPath $ffManifestSrc -Destination (Join-Path $firefoxExtStage 'manifest.json') -Force
$firefoxZip = Join-Path $DistDir "RuviTube-v$ExtensionVersion-firefox.zip"
if (Test-Path -LiteralPath $firefoxZip) { Remove-Item -LiteralPath $firefoxZip -Force }
Compress-Archive -Path (Join-Path $firefoxExtStage '*') -DestinationPath $firefoxZip -CompressionLevel Optimal

# Build portable bundle zip
Copy-Item -LiteralPath (Join-Path $BinStage 'ruvi-studio.exe') -Destination $PortableStage -Force
Copy-Item -LiteralPath (Join-Path $BinStage 'ruvid.exe') -Destination $PortableStage -Force
Copy-Item -LiteralPath (Join-Path $BinStage 'ruvic.exe') -Destination $PortableStage -Force
Copy-Item -LiteralPath (Join-Path $BinStage 'NativeHost.exe') -Destination $PortableStage -Force
foreach ($doc in @('README.md', 'BENCHMARK_REPORT.md')) {
  $docPath = Join-Path $RepoRoot $doc
  if (Test-Path -LiteralPath $docPath -PathType Leaf) {
    Copy-Item -LiteralPath $docPath -Destination $PortableStage -Force
  }
}
$portableRuviTube = Join-Path $PortableStage 'ruvitube'
New-Item -ItemType Directory -Force -Path $portableRuviTube | Out-Null
Copy-Item -Path (Join-Path $RepoRoot 'ruvitube\extension') -Destination $portableRuviTube -Recurse -Force
Copy-Item -Path (Join-Path $RepoRoot 'ruvitube\installer') -Destination $portableRuviTube -Recurse -Force
Copy-Item -Path $RuviTubeBin -Destination $portableRuviTube -Recurse -Force

$portableZip = Join-Path $DistDir "RuviTools-v$Version-win64-portable.zip"
if (Test-Path -LiteralPath $portableZip) { Remove-Item -LiteralPath $portableZip -Force }
Compress-Archive -Path (Join-Path $PortableStage '*') -DestinationPath $portableZip -CompressionLevel Optimal

# Build WiX v7 MSI
$msiPath = Join-Path $DistDir "RuviTools-v$Version-win64.msi"
if (-not $SkipMsi) {
  $wixCmd = Get-Command wix -ErrorAction SilentlyContinue
  if (-not $wixCmd) {
    throw 'WiX Toolset v7 CLI (wix.exe) not found in PATH.'
  }
  $wxsPath = Join-Path $RepoRoot 'installer\RuviTools.wxs'
  $iconPath = Join-Path $RepoRoot 'ruvi-gui\assets\ruvi-studio.ico'
  if (Test-Path -LiteralPath $msiPath) { Remove-Item -LiteralPath $msiPath -Force }
  Write-Host "Building MSI with WiX v7 -> $msiPath" -ForegroundColor Yellow
  & wix --acceptEula wix7 build `
    -arch x64 `
    -d "Version=$Version" `
    -d "BinDir=$BinStage" `
    -d "ExtDir=$ExtStage" `
    -d "IconPath=$iconPath" `
    -out $msiPath `
    $wxsPath
  if ($LASTEXITCODE -ne 0) {
    throw "wix build failed with exit code $LASTEXITCODE"
  }
}

Write-Host ''
Write-Host '=== Release Artifacts ===' -ForegroundColor Cyan
Get-ChildItem -LiteralPath $DistDir -File | ForEach-Object {
  $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
  Write-Host ("{0,-36} {1,10:N0} bytes  SHA256={2}" -f $_.Name, $_.Length, $hash) -ForegroundColor Green
}

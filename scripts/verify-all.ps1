# RuviTools — Birleşik Doğrulama Betiği (Supply-Chain, JSON, PowerShell AST, Rust Gates)
# UTF-8 BOM

[CmdletBinding()]
param(
  [switch]$SkipRust,
  [string[]]$Crates = @('ruvid', 'ruvic', 'ruvi-native-host', 'ruvi-gui'),
  [switch]$FailOnMultipleVersions
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$failures = New-Object System.Collections.Generic.List[string]
$results = New-Object System.Collections.Generic.List[pscustomobject]

function Add-CheckResult {
  param(
    [string]$Check,
    [bool]$Passed,
    [string]$Detail
  )
  $status = if ($Passed) { 'PASS' } else { 'FAIL' }
  $color = if ($Passed) { 'Green' } else { 'Red' }
  Write-Host ("[{0}] {1}: {2}" -f $status, $Check, $Detail) -ForegroundColor $color
  $results.Add([pscustomobject]@{
    Check  = $Check
    Status = $status
    Detail = $Detail
  })
  if (-not $Passed) {
    $failures.Add("${Check}: ${Detail}")
  }
}

Write-Host "RuviTools Verification Root: $RepoRoot" -ForegroundColor Cyan
Write-Host ''

# 1. Supply-chain policy (deny.toml)
$denyPath = Join-Path $RepoRoot 'deny.toml'
if (-not (Test-Path -LiteralPath $denyPath -PathType Leaf)) {
  Add-CheckResult -Check 'deny.toml' -Passed $false -Detail 'Missing root deny.toml'
} else {
  $denyRaw = Get-Content -LiteralPath $denyPath -Raw -Encoding UTF8
  $requiredMarkers = @(
    '[graph]',
    '[advisories]',
    '[licenses]',
    '[bans]',
    '[sources]',
    'unknown-registry = "deny"',
    'unknown-git = "deny"'
  )
  $missing = @($requiredMarkers | Where-Object { $denyRaw -notmatch [regex]::Escape($_) })
  if ($missing.Count -eq 0) {
    Add-CheckResult -Check 'deny.toml' -Passed $true -Detail 'All required policy sections and source denials present'
  } else {
    Add-CheckResult -Check 'deny.toml' -Passed $false -Detail ("Missing markers: " + ($missing -join ', '))
  }
}

# 2. Cargo.lock supply-chain source & TLS audit across all crates
$allowedRegistry = 'registry+https://github.com/rust-lang/crates.io-index'
foreach ($crate in $Crates) {
  $lockPath = Join-Path $RepoRoot "$crate\Cargo.lock"
  if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
    Add-CheckResult -Check "Cargo.lock ($crate)" -Passed $false -Detail "Missing $lockPath"
    continue
  }

  $raw = Get-Content -LiteralPath $lockPath -Raw -Encoding UTF8
  # A lockfile padded with NUL bytes (or truncated) silently breaks every cargo command.
  $nulCount = ([regex]::Matches($raw, "`0")).Count
  if ($nulCount -gt 0) {
    Add-CheckResult -Check "Cargo.lock ($crate)" -Passed $false -Detail "$nulCount NUL byte(s) found; regenerate the lockfile (corrupted on disk)"
    continue
  }
  $blocks = $raw -split '\r?\n\[\[package\]\]\r?\n'
  $pkgCount = 0
  $badSources = New-Object System.Collections.Generic.List[string]
  $byName = @{}
  $nativeTls = New-Object System.Collections.Generic.List[string]
  $hasRustls = $false

  foreach ($block in $blocks) {
    if ($block -match 'name = "([^"]+)"\r?\nversion = "([^"]+)"') {
      $pkgCount++
      $name = $Matches[1]
      $ver = $Matches[2]
      if (-not $byName.ContainsKey($name)) { $byName[$name] = New-Object System.Collections.Generic.List[string] }
      $byName[$name].Add($ver)

      # Pure-Rust TLS policy (deny.toml): no native OpenSSL / platform TLS backends.
      if ($name -in @('openssl', 'openssl-sys', 'native-tls', 'schannel')) { $nativeTls.Add("${name}@${ver}") }
      if ($name -eq 'rustls') { $hasRustls = $true }

      if ($block -match 'source = "([^"]+)"') {
        $src = $Matches[1]
        if ($src -ne $allowedRegistry) {
          $badSources.Add("${name}@${ver} -> ${src}")
        }
      }
    }
  }

  if ($pkgCount -eq 0) {
    Add-CheckResult -Check "Cargo.lock ($crate)" -Passed $false -Detail "No [[package]] entries parsed; lockfile is malformed"
    continue
  }

  $dupCount = @($byName.GetEnumerator() | Where-Object { $_.Value.Count -gt 1 }).Count
  $tlsNote = if ($hasRustls) { 'rustls' } elseif ($nativeTls.Count -gt 0) { 'native' } else { 'none' }

  if ($badSources.Count -gt 0) {
    Add-CheckResult -Check "Cargo.lock ($crate)" -Passed $false -Detail ("Disallowed sources: " + ($badSources -join '; '))
  } elseif ($nativeTls.Count -gt 0) {
    Add-CheckResult -Check "Cargo.lock ($crate)" -Passed $false -Detail ("Native TLS/OpenSSL crates present (use reqwest rustls-tls): " + ($nativeTls -join ', '))
  } elseif ($FailOnMultipleVersions -and $dupCount -gt 0) {
    Add-CheckResult -Check "Cargo.lock ($crate)" -Passed $false -Detail "$pkgCount pkgs, $dupCount duplicate crate names (FailOnMultipleVersions enabled)"
  } else {
    Add-CheckResult -Check "Cargo.lock ($crate)" -Passed $true -Detail "$pkgCount pkgs, crates.io-only, dups=$dupCount, tls=$tlsNote"
  }
}

# 3. Chrome/Extension JSON validation
$extRoot = Join-Path $RepoRoot 'ruvitube\extension'
if (-not (Test-Path -LiteralPath $extRoot -PathType Container)) {
  Add-CheckResult -Check 'Extension JSON' -Passed $false -Detail "Directory not found: $extRoot"
} else {
  $jsonFiles = Get-ChildItem -LiteralPath $extRoot -Recurse -Filter '*.json' -File
  if ($jsonFiles.Count -eq 0) {
    Add-CheckResult -Check 'Extension JSON' -Passed $false -Detail 'No .json files found under ruvitube/extension'
  } else {
    $jsonErrors = New-Object System.Collections.Generic.List[string]
    foreach ($jf in $jsonFiles) {
      try {
        $parsed = Get-Content -LiteralPath $jf.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($null -eq $parsed) { $jsonErrors.Add("$($jf.Name): null JSON") }
      } catch {
        $jsonErrors.Add("$($jf.FullName): $($_.Exception.Message)")
      }
    }
    if ($jsonErrors.Count -eq 0) {
      Add-CheckResult -Check 'Extension JSON' -Passed $true -Detail "$($jsonFiles.Count) JSON files valid"
    } else {
      Add-CheckResult -Check 'Extension JSON' -Passed $false -Detail ($jsonErrors -join ' | ')
    }
  }
}

# 3b. Extension JavaScript syntax validation (via Node if available)
$nodeCmd = Get-Command node -ErrorAction SilentlyContinue
if (-not $nodeCmd -and (Test-Path 'C:\Program Files\nodejs\node.exe')) {
  $nodeCmd = Get-Command 'C:\Program Files\nodejs\node.exe' -ErrorAction SilentlyContinue
}
if ($nodeCmd -and (Test-Path -LiteralPath $extRoot -PathType Container)) {
  $jsFiles = Get-ChildItem -LiteralPath $extRoot -Filter '*.js' -File -Recurse -ErrorAction SilentlyContinue
  $jsErrors = @()
  $prevEap = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  foreach ($jf in $jsFiles) {
    $out = & $nodeCmd.Source --check $jf.FullName 2>&1
    if ($LASTEXITCODE -ne 0) {
      $jsErrors += "$($jf.Name): $(($out | Out-String).Trim())"
    }
  }
  $ErrorActionPreference = $prevEap
  if ($jsErrors.Count -eq 0) {
    Add-CheckResult -Check 'Extension JS Syntax' -Passed $true -Detail "$($jsFiles.Count) JS files valid"
  } else {
    Add-CheckResult -Check 'Extension JS Syntax' -Passed $false -Detail ($jsErrors -join ' | ')
  }
}

# 4. Installer entrypoints & PowerShell AST syntax validation
$requiredInstallerFiles = @(
  'ruvitube\installer\Kurulum.cmd',
  'ruvitube\installer\Kaldir.cmd',
  'ruvitube\installer\install-core.ps1',
  'ruvitube\installer\Install.ps1',
  'ruvitube\installer\Uninstall.ps1',
  'ruvitube\installer\Install-GUI.ps1',
  'ruvitube\installer\launch.ps1'
)
$missingInstaller = @($requiredInstallerFiles | Where-Object { -not (Test-Path -LiteralPath (Join-Path $RepoRoot $_) -PathType Leaf) })
if ($missingInstaller.Count -eq 0) {
  Add-CheckResult -Check 'Installer Entrypoints' -Passed $true -Detail 'All .cmd and .ps1 installer files present'
} else {
  Add-CheckResult -Check 'Installer Entrypoints' -Passed $false -Detail ('Missing: ' + ($missingInstaller -join ', '))
}

$ps1Dirs = @(
  (Join-Path $RepoRoot 'ruvitube\installer'),
  (Join-Path $RepoRoot 'scripts')
)
$ps1Files = foreach ($d in $ps1Dirs) {
  if (Test-Path -LiteralPath $d -PathType Container) {
    Get-ChildItem -LiteralPath $d -Filter '*.ps1' -File
  }
}
$parseErrorsList = New-Object System.Collections.Generic.List[string]
foreach ($ps1 in $ps1Files) {
  $tokens = $null
  $parseErrors = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile($ps1.FullName, [ref]$tokens, [ref]$parseErrors)
  if ($parseErrors -and $parseErrors.Count -gt 0) {
    foreach ($err in $parseErrors) {
      $parseErrorsList.Add("$($ps1.Name):L$($err.Extent.StartLineNumber): $($err.Message)")
    }
  }
}
if ($parseErrorsList.Count -eq 0) {
  Add-CheckResult -Check 'PowerShell AST' -Passed $true -Detail "$($ps1Files.Count) .ps1 scripts parsed with zero errors"
} else {
  Add-CheckResult -Check 'PowerShell AST' -Passed $false -Detail ($parseErrorsList -join ' | ')
}

# 5. Rust fmt, clippy, and test gates (unless -SkipRust)
if ($SkipRust) {
  Write-Host '[SKIP] Rust fmt/clippy/test gates skipped (-SkipRust).' -ForegroundColor Yellow
} else {
  foreach ($crate in $Crates) {
    $manifest = Join-Path $RepoRoot "$crate\Cargo.toml"
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
      Add-CheckResult -Check "Rust ($crate)" -Passed $false -Detail "Missing $manifest"
      continue
    }

    & cargo fmt --manifest-path $manifest --all -- --check
    Add-CheckResult -Check "cargo fmt ($crate)" -Passed ($LASTEXITCODE -eq 0) -Detail "exit=$LASTEXITCODE"

    & cargo clippy --manifest-path $manifest --all-targets -- -D warnings
    Add-CheckResult -Check "cargo clippy ($crate)" -Passed ($LASTEXITCODE -eq 0) -Detail "exit=$LASTEXITCODE"

    & cargo test --manifest-path $manifest
    Add-CheckResult -Check "cargo test ($crate)" -Passed ($LASTEXITCODE -eq 0) -Detail "exit=$LASTEXITCODE"
  }
}

Write-Host ''
Write-Host '=== Verification Summary ===' -ForegroundColor Cyan
$results | Format-Table -AutoSize

if ($failures.Count -gt 0) {
  Write-Host "FAILED ($($failures.Count) issue(s)):" -ForegroundColor Red
  foreach ($f in $failures) { Write-Host " - $f" -ForegroundColor Red }
  exit 1
}

Write-Host 'ALL CHECKS PASSED.' -ForegroundColor Green
exit 0

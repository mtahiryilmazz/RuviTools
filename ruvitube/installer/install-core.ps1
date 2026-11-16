# RuviTube — kurulum çekirdeği (konsol + GUI + başlatıcı tarafından paylaşılır)
# Doğrudan çalıştırılmaz; Install.ps1 / Install-GUI.ps1 / launch.ps1 dot-source eder.
# UTF-8 BOM

$AppName          = 'RuviTube'
$LocalAppData     = $env:LOCALAPPDATA
if (-not $LocalAppData) { $LocalAppData = [Environment]::GetFolderPath('LocalApplicationData') }
$UserProfileDir   = $env:USERPROFILE
if (-not $UserProfileDir) { $UserProfileDir = [Environment]::GetFolderPath('UserProfile') }
$Windir           = $env:WINDIR
if (-not $Windir) { $Windir = $env:SystemRoot }
if (-not $Windir) { $Windir = 'C:\Windows' }

$AppDir           = Join-Path $LocalAppData $AppName
$ExtDir           = Join-Path $AppDir 'extension'
$NativeExe        = Join-Path $AppDir 'NativeHost.exe'
$HostName         = 'com.ruvitube.nativehost'
$ExtId            = 'npbogohemcamnnhpmbnialffkfkbeimn'
$ChromeProfileDir = Join-Path $AppDir 'chrome-profile'
$ChromeDebugPortFile = Join-Path $ChromeProfileDir 'DevToolsActivePort'
$ShortcutPath     = Join-Path ([Environment]::GetFolderPath('Desktop')) 'RuviTube.lnk'
$Root             = Split-Path -Parent $PSScriptRoot
$Log              = Join-Path $AppDir 'install.log'
$LegacyDirs = @(
  (Join-Path $LocalAppData 'GeTube'),
  (Join-Path $LocalAppData 'YouTubeEgitimIndiriciPRO'),
  (Join-Path $LocalAppData 'YouTube Egitim Indirici PRO'),
  (Join-Path $LocalAppData 'YouTube_Egitim_Indirici_PRO')
)

function Invoke-Log {
  param([string]$Message)
  $line = "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $Message"
  try { Add-Content -LiteralPath $Log -Value $line -Encoding UTF8 } catch {}
  Write-Host $line
}

function Ensure-Environment {
  # Bazi kabuklarda COMSPEC/SystemRoot/PATH bos gelir; Chrome native messaging
  # Windows'ta COMSPEC'e ihtiyac duyar (yoksa host baslatilamaz).
  if (-not $env:WINDIR) { $env:WINDIR = $Windir }
  if (-not $env:SystemRoot) { $env:SystemRoot = $Windir }
  if (-not $env:COMSPEC) { $env:COMSPEC = (Join-Path $Windir 'System32\cmd.exe') }
  if (-not $env:PATH) { $env:PATH = (Join-Path $Windir 'System32') + ';' + $Windir }
}

function Existing-File([string]$name){
  $candidates = New-Object System.Collections.Generic.List[string]
  [void]$candidates.Add((Join-Path $AppDir $name))
  foreach($d in $LegacyDirs){ if($d){ [void]$candidates.Add((Join-Path $d $name)) } }
  try {
    $cmd = Get-Command $name -CommandType Application -ErrorAction SilentlyContinue
    if($cmd -and $cmd.Source){ [void]$candidates.Add($cmd.Source) }
  } catch {}
  if($LocalAppData){
    [void]$candidates.Add((Join-Path $LocalAppData ('Programs\'+$name)))
    [void]$candidates.Add((Join-Path $LocalAppData ('Programs\'+($name -replace '\.exe$','')+'\'+$name)))
  }
  if($UserProfileDir){
    [void]$candidates.Add((Join-Path $UserProfileDir ('Downloads\'+$name)))
    [void]$candidates.Add((Join-Path $UserProfileDir ('Desktop\'+$name)))
  }
  if($name -eq 'ffmpeg.exe' -and $env:ProgramFiles){
    [void]$candidates.Add((Join-Path $env:ProgramFiles 'ffmpeg\bin\ffmpeg.exe'))
  }
  foreach($candidate in $candidates){
    if($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)){
      try { return (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path } catch { return $candidate }
    }
  }
  return $null
}

function Download-File([string]$url,[string]$dest){
  Invoke-Log "Eksik bileşen indiriliyor: $url"
  $tmp = "$dest.download"
  Remove-Item $tmp -Force -ErrorAction SilentlyContinue
  try {
    $wc = New-Object System.Net.WebClient
    $wc.Headers['User-Agent']='Mozilla/5.0 GeTube/1.2.2'
    $wc.DownloadFile($url,$tmp)
    $wc.Dispose()
    if(!(Test-Path $tmp) -or (Get-Item $tmp).Length -lt 1024){ throw 'İndirilen dosya geçersiz.' }
    Move-Item $tmp $dest -Force
  } catch {
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
    throw "İndirme başarısız: $($_.Exception.Message)"
  }
}

function Ensure-RuviEngine{
  $binDir = Join-Path $Root 'bin'
  foreach($legacy in @('yt-dlp.exe','deno.exe','ffmpeg.exe','ffprobe.exe')){
    $old = Join-Path $AppDir $legacy
    if(Test-Path -LiteralPath $old){
      Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
      Invoke-Log "✓ Eski harici bileşen temizlendi: $legacy"
    }
  }

  foreach($tool in @('NativeHost.exe','ruvid.exe','ruvic.exe')){
    $src = Join-Path $binDir $tool
    $dst = Join-Path $AppDir $tool
    if(Test-Path -LiteralPath $src -PathType Leaf){
      Copy-Item -LiteralPath $src -Destination $dst -Force
      try { Unblock-File -LiteralPath $dst -ErrorAction SilentlyContinue } catch {}
      Invoke-Log "✓ Saf Rust bileşeni kuruldu: $tool"
    } else {
      $url = "https://github.com/mtahiryilmazz/RuviTools/releases/latest/download/$tool"
      Download-File $url $dst
      try { Unblock-File -LiteralPath $dst -ErrorAction SilentlyContinue } catch {}
      Invoke-Log "✓ GitHub Release üzerinden kuruldu: $tool"
    }
  }
}

function Install-Extension{
  if(Test-Path $ExtDir){ Remove-Item $ExtDir -Recurse -Force }
  Copy-Item (Join-Path $Root 'extension') $ExtDir -Recurse -Force
  Invoke-Log "✓ GeTube Chrome eklentisi kopyalandı. ID: $ExtId"
}

function Register-Host{
  $manifest=Join-Path $AppDir ($HostName+'.json')
  $json=@"
{
  "name": "$HostName",
  "description": "RuviTube native host",
  "path": "$($NativeExe.Replace('\','\\'))",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://$ExtId/"]
}
"@
  [IO.File]::WriteAllText($manifest,$json,(New-Object Text.UTF8Encoding($false)))
  $reg="HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName"
  New-Item -Path $reg -Force | Out-Null
  New-ItemProperty -Path $reg -Name '(Default)' -Value $manifest -PropertyType String -Force | Out-Null
  Invoke-Log '✓ Native Messaging kaydı tamamlandı.'
}

function Verify-Registration{
  $manifest=Join-Path $AppDir ($HostName+'.json')
  $reg='HKCU:\Software\Google\Chrome\NativeMessagingHosts\'+$HostName
  if(!(Test-Path -LiteralPath $manifest)){ throw 'Native Messaging manifest dosyası bulunamadı.' }
  $value=(Get-ItemProperty -Path $reg -Name '(Default)' -ErrorAction SilentlyContinue).'(default)'
  if(!$value){ throw 'Native Messaging kayıt defteri kaydı yok.' }
  try {
    $cfg=Get-Content -LiteralPath $manifest -Raw -Encoding UTF8 | ConvertFrom-Json
    if($cfg.path -ne $NativeExe){ throw "Manifest path beklenenden farklı: $($cfg.path)" }
    if(($cfg.allowed_origins -notcontains "chrome-extension://$ExtId/")){ throw 'allowed_origins eşleşmiyor.' }
  } catch { throw "Manifest doğrulanamadı: $($_.Exception.Message)" }
  Invoke-Log '✓ Native Messaging kaydı doğrulandı.'
}

function Clear-Staging{
  $staging=Join-Path $AppDir 'staging'
  if(!(Test-Path -LiteralPath $staging)){ return }
  try {
    Get-ChildItem -LiteralPath $staging -File -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    Get-ChildItem -LiteralPath $staging -Directory -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
  } catch { Invoke-Log "UYARI: staging temizlenemedi: $($_.Exception.Message)" }
}

function Find-Chrome{
  # $env:ProgramFiles bazi kabuklarda bos olabiliyor: Join-Path'e null vermemek icin
  # adaylar tek tek ve guvenli degerlerle kurulur.
  $candidates = New-Object System.Collections.Generic.List[string]

  $pf = $env:ProgramFiles
  if (-not $pf) { $pf = [Environment]::GetFolderPath('ProgramFiles') }
  if (-not $pf) { $pf = 'C:\Program Files' }
  [void]$candidates.Add((Join-Path $pf 'Google\Chrome\Application\chrome.exe'))

  $pf86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
  if ($pf86) { [void]$candidates.Add((Join-Path $pf86 'Google\Chrome\Application\chrome.exe')) }
  if ($LocalAppData) { [void]$candidates.Add((Join-Path $LocalAppData 'Google\Chrome\Application\chrome.exe')) }

  try {
    $ap=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe' -ErrorAction SilentlyContinue).'(default)'
    if($ap){ [void]$candidates.Add($ap) }
  } catch {}

  foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path -LiteralPath $candidate)) { return $candidate }
  }
  return $null
}
function Stop-GeTubeChrome{
  try {
    $procs = Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" -ErrorAction SilentlyContinue |
      Where-Object { $_.CommandLine -and $_.CommandLine -like "*$ChromeProfileDir*" }
    foreach($p in $procs){ try { Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }
    if($procs){ Start-Sleep -Seconds 2 }
  } catch {}
}

function Start-GeTubeChrome{
  param([string[]]$ExtraArgs = @())
  Ensure-Environment
  $chrome = Find-Chrome
  if(!$chrome){ throw 'Google Chrome bulunamadı. Lütfen Chrome kurun.' }
  New-Item -ItemType Directory -Force -Path $ChromeProfileDir | Out-Null
  if (Test-Path -LiteralPath $ChromeDebugPortFile) { Remove-Item -LiteralPath $ChromeDebugPortFile -Force -ErrorAction SilentlyContinue }

  $chromeArgs = @(
    "--user-data-dir=$ChromeProfileDir",
    '--remote-debugging-port=0',
    '--no-first-run',
    '--no-default-browser-check',
    '--enable-logging',
    '--v=1'
  ) + $ExtraArgs
  if ($ExtraArgs.Count -eq 0) { $chromeArgs += 'about:blank' }

  $quoted = ($chromeArgs | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }) -join ' '
  Start-Process -FilePath $chrome -ArgumentList $quoted | Out-Null

  $deadline=(Get-Date).AddSeconds(30)
  while(!(Test-Path -LiteralPath $ChromeDebugPortFile)){
    if((Get-Date) -gt $deadline){ throw 'Chrome hata ayıklama portu açılamadı.' }
    Start-Sleep -Milliseconds 300
  }
  $lines = Get-Content -LiteralPath $ChromeDebugPortFile
  $port = [int]$lines[0]
  $wsPath = $lines[1]

  $probeDeadline = (Get-Date).AddSeconds(25)
  while ($true) {
    try {
      Invoke-RestMethod -Uri ("http://127.0.0.1:{0}/json/version" -f $port) -TimeoutSec 3 | Out-Null
      break
    } catch {
      if ((Get-Date) -gt $probeDeadline) { throw "Chrome hata ayıklama ucu yanıt vermiyor (port $port)." }
      Start-Sleep -Milliseconds 400
    }
  }
  return @{ Chrome=$chrome; Port=$port; WsPath=$wsPath }
}

function Invoke-CdpCommand{
  param([int]$Port,[string]$WsPath,[hashtable]$Message,[int]$TimeoutSec=25)
  $ws = New-Object System.Net.WebSockets.ClientWebSocket
  $ct = [System.Threading.CancellationToken]::None
  $ws.ConnectAsync([Uri]("ws://127.0.0.1:$Port$WsPath"), $ct).Wait($TimeoutSec*1000) | Out-Null
  if($ws.State -ne [System.Net.WebSockets.WebSocketState]::Open){ throw 'CDP bağlantısı kurulamadı.' }

  $json = $Message | ConvertTo-Json -Depth 6 -Compress
  $bytes = [Text.Encoding]::UTF8.GetBytes($json)
  $ws.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $ct).Wait($TimeoutSec*1000) | Out-Null

  $sb = New-Object System.Text.StringBuilder
  $buffer = New-Object byte[] (64*1024)
  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  try {
    while($true){
      if((Get-Date) -gt $deadline){ throw 'CDP yanıtı zaman aşımı.' }
      $cts = New-Object System.Threading.CancellationTokenSource(2500)
      try {
        $task = $ws.ReceiveAsync([ArraySegment[byte]]::new($buffer), $cts.Token)
        if(-not $task.Wait(3000)){ $cts.Dispose(); continue }
        $res = $task.Result
      } catch { if($cts){ $cts.Dispose() }; continue }

      if($res.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close){ throw 'CDP bağlantısı kapandı.' }
      [void]$sb.Append([Text.Encoding]::UTF8.GetString($buffer, 0, $res.Count))
      if($res.EndOfMessage){
        $text = $sb.ToString()
        [void]$sb.Clear()
        try { $obj = $text | ConvertFrom-Json } catch { continue }
        if($obj.id -eq $Message.id){
          if($obj.error){ throw ('CDP hatası: ' + $obj.error.message) }
          return $obj.result
        }
      }
    }
  } finally { try { $ws.Dispose() } catch {} }
}

function Enable-DeveloperMode{
  # Unpacked eklentinin açılışta yüklenmesi için profil anahtarı. Preferences dosyası
  # TOPTAN yazılmaz (Chrome profili bozuluyor); yalnızca tek anahtar eklenir.
  $pref = Join-Path (Join-Path $ChromeProfileDir 'Default') 'Preferences'
  if (!(Test-Path -LiteralPath $pref)) { return $false }
  try {
    $raw = Get-Content -LiteralPath $pref -Raw -Encoding UTF8
    if ($raw -match '"developer_mode"\s*:\s*true') { return $true }
    if ($raw -match '"ui"\s*:\s*\{') {
      $patched = [regex]::Replace($raw, '("ui"\s*:\s*\{)', '$1"developer_mode":true,', 1)
    } else {
      $patched = [regex]::Replace($raw, '("extensions"\s*:\s*\{)', '$1"ui":{"developer_mode":true},', 1)
    }
    if ($patched -and $patched -ne $raw) {
      Set-Content -LiteralPath $pref -Value $patched -Encoding UTF8 -NoNewline
      Invoke-Log '✓ Chrome profilinde geliştirici modu açıldı.'
      return $true
    }
  } catch {
    Invoke-Log "UYARI: geliştirici modu yazılamadı: $($_.Exception.Message)"
  }
  return $false
}

function Test-ExtensionLoaded{
  param([int]$Port, [int]$TimeoutSec = 20)
  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  while ((Get-Date) -lt $deadline) {
    try {
      $targets = Invoke-RestMethod -Uri ("http://127.0.0.1:{0}/json/list" -f $Port) -TimeoutSec 3
      $hit = $targets | Where-Object { $_.url -like "*chrome-extension://$ExtId/*" }
      if ($hit) { return $true }
    } catch { }
    Start-Sleep -Milliseconds 500
  }
  return $false
}

function Enable-ExtensionViaCdp{
  # Chrome'un Extensions.loadUnpacked komutuyla eklentiyi yükler ve doğrular.
  Enable-DeveloperMode | Out-Null
  Invoke-Log '[otomatik] Chrome GeTube profili başlatılıyor...'
  $session = Start-GeTubeChrome
  Invoke-Log ("[otomatik] CDP portu: {0}" -f $session.Port)

  try {
    $result = Invoke-CdpCommand -Port $session.Port -WsPath $session.WsPath -Message @{
      id = 1; method = 'Extensions.loadUnpacked'; params = @{ path = $ExtDir }
    }
    $loadedId = $result.id
    if ($loadedId -ne $ExtId) { throw "Eklenti beklenmeyen kimlikle yüklendi: $loadedId" }
    Invoke-Log "✓ Eklenti yüklendi (kimlik: $loadedId)."
  } catch {
    if ($_.Exception.Message -match 'already|loaded|exists') {
      Invoke-Log '  Eklenti bu oturumda zaten yüklü.'
    } else { throw }
  }

  if (Test-ExtensionLoaded -Port $session.Port -TimeoutSec 20) { Invoke-Log '✓ Doğrulama: eklenti yüklü.' }
  else { Invoke-Log 'UYARI: eklenti listede görünmedi.' }

  try {
    Invoke-CdpCommand -Port $session.Port -WsPath $session.WsPath -Message @{
      id = 2; method = 'Target.createTarget'; params = @{ url = 'https://www.youtube.com' }
    } | Out-Null
  } catch { Invoke-Log "UYARI: YouTube sekmesi açılamadı: $($_.Exception.Message)" }
  return $session
}

function Install-LauncherFiles{
  $src1 = Join-Path $PSScriptRoot 'install-core.ps1'
  $src2 = Join-Path $PSScriptRoot 'launch.ps1'
  if (Test-Path -LiteralPath $src1) { Copy-Item -LiteralPath $src1 -Destination (Join-Path $AppDir 'install-core.ps1') -Force }
  if (Test-Path -LiteralPath $src2) {
    Copy-Item -LiteralPath $src2 -Destination (Join-Path $AppDir 'launch.ps1') -Force
    Invoke-Log '✓ Başlatıcı kuruldu (launch.ps1).'
  }
}

function New-DesktopShortcut{
  param([string]$ChromePath)
  try {
    $launcher = Join-Path $AppDir 'launch.ps1'
    $shell = New-Object -ComObject WScript.Shell
    $sc = $shell.CreateShortcut($ShortcutPath)
    if (Test-Path -LiteralPath $launcher) {
      $sc.TargetPath = Join-Path $Windir 'System32\WindowsPowerShell\v1.0\powershell.exe'
      $sc.Arguments = "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$launcher`""
    } else {
      $sc.TargetPath = $ChromePath
      $sc.Arguments = "--user-data-dir=`"$ChromeProfileDir`" --no-first-run --no-default-browser-check https://www.youtube.com"
    }
    $sc.Description = 'GeTube — YouTube indirme (özel Chrome profili)'
    if ($ChromePath) { $sc.IconLocation = "$ChromePath,0" }
    $sc.Save()
    Invoke-Log "✓ Masaüstü kısayolu oluşturuldu: $ShortcutPath"
  } catch { Invoke-Log "UYARI: Kısayol oluşturulamadı: $($_.Exception.Message)" }
}

function Invoke-Install{
  param([switch]$SkipAutoLoad)
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue'
  New-Item -ItemType Directory -Force -Path $AppDir | Out-Null
  Ensure-Environment

  if($PSVersionTable.PSVersion.Major -lt 5){ throw 'Windows PowerShell 5 veya daha yeni gerekli.' }
  Invoke-Log '[1/5] Sistem kontrolü tamamlandı'
  Invoke-Log '[2/5] Saf Rust RuviTools (NativeHost + ruvid + ruvic) motoru kuruluyor'
  Ensure-RuviEngine
  Invoke-Log '[3/5] Chrome eklentisi ve Native Messaging kaydı'
  Install-Extension
  Register-Host
  Verify-Registration
  Install-LauncherFiles
  Invoke-Log '[4/5] Kurulum doğrulanıyor'
  Clear-Staging
  if(!(Test-Path (Join-Path $AppDir 'ruvid.exe')) -or !(Test-Path (Join-Path $AppDir 'ruvic.exe')) -or !(Test-Path $NativeExe)){
    throw 'Kurulum doğrulaması başarısız: ruvid.exe, ruvic.exe veya NativeHost.exe eksik.'
  }

  Invoke-Log '[5/5] Eklenti Chrome''a yükleniyor'
  $autoLoaded = $false
  $chromePath = Find-Chrome
  if(!$chromePath){ Invoke-Log 'UYARI: Chrome bulunamadı; eklenti otomatik yüklenemedi.' }
  elseif($SkipAutoLoad){ Invoke-Log 'Otomatik yükleme atlandı.' }
  else {
    try {
      Stop-GeTubeChrome
      $session = Enable-ExtensionViaCdp
      $autoLoaded = $true
      New-DesktopShortcut -ChromePath $session.Chrome
      Invoke-Log 'Chrome açıldı. Masaüstündeki GeTube kısayolu her açılışta eklentiyi yeniden yükler.'
    } catch {
      Invoke-Log "UYARI: Otomatik yükleme başarısız: $($_.Exception.Message)"
      Invoke-Log "Elle yükleme: chrome://extensions -> Geliştirici modu -> Paketlenmemiş öğe yükle -> $ExtDir"
      if($chromePath){ New-DesktopShortcut -ChromePath $chromePath }
    }
  }

  foreach($LegacyDir in $LegacyDirs){
    if((Test-Path $LegacyDir) -and ($LegacyDir -ne $AppDir)){
      Remove-Item $LegacyDir -Recurse -Force -ErrorAction SilentlyContinue
      Invoke-Log "Eski uygulama klasörü temizlendi: $LegacyDir"
    }
  }

  return @{ AutoLoaded = $autoLoaded; ChromePath = $chromePath }
}

function Invoke-Uninstall{
  $ErrorActionPreference = 'SilentlyContinue'
  Invoke-Log 'GeTube kaldırılıyor...'

  $reg = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName"
  if (Test-Path $reg) { Remove-Item $reg -Recurse -Force; Invoke-Log "  ✓ Kayıt defteri girdisi silindi: $HostName" }

  Get-Process -Name 'NativeHost' -ErrorAction SilentlyContinue | ForEach-Object {
    try { $_.Kill(); Invoke-Log "  ✓ Çalışan süreç kapatıldı: $($_.Id)" } catch {}
  }

  Stop-GeTubeChrome

  foreach ($dir in @($AppDir) + $LegacyDirs) {
    if ($dir -and (Test-Path $dir)) {
      Remove-Item $dir -Recurse -Force
      Invoke-Log "  ✓ Klasör silindi: $dir"
    }
  }

  if (Test-Path -LiteralPath $ShortcutPath) {
    Remove-Item -LiteralPath $ShortcutPath -Force
    Invoke-Log "  ✓ Masaüstü kısayolu silindi: $ShortcutPath"
  }

  Invoke-Log ''
  Invoke-Log 'KALDIRMA TAMAMLANDI.'
  Invoke-Log 'Eklentiyi chrome://extensions üzerinden de kaldırın.'
  Invoke-Log "İndirilen dosyalar korunur: $(Join-Path $UserProfileDir 'Downloads\GeTube')"
}

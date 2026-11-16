# GeTube v2.0.0 — GUI kurulum sihirbazı (WinForms, Türkçe — Saf Rust ruvid + ruvic)
# UTF-8 BOM
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

. (Join-Path $PSScriptRoot 'install-core.ps1')

# ---------------------------------------------------------------- arayüz ----
$form = New-Object System.Windows.Forms.Form
$form.Text = 'RuviTube Kurulum'
$form.Size = New-Object System.Drawing.Size(680, 520)
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedSingle'
$form.MaximizeBox = $false
$form.BackColor = [System.Drawing.Color]::FromArgb(24, 24, 24)
$form.ForeColor = [System.Drawing.Color]::White
$form.Font = New-Object System.Drawing.Font('Segoe UI', 9)

$title = New-Object System.Windows.Forms.Label
$title.Text = 'RuviTube v2.0  ·  Saf Rust (ruvid + ruvic) Kurulumu'
$title.Font = New-Object System.Drawing.Font('Segoe UI', 15, [System.Drawing.FontStyle]::Bold)
$title.ForeColor = [System.Drawing.Color]::FromArgb(230, 230, 230)
$title.Location = New-Object System.Drawing.Point(20, 16)
$title.Size = New-Object System.Drawing.Size(560, 30)
$form.Controls.Add($title)

$subtitle = New-Object System.Windows.Forms.Label
$subtitle.Text = 'Saf Rust ruvid + ruvic + NativeHost (AV dostu, sıfır harici indirme) kurulur ve Chrome''a yüklenir.'
$subtitle.ForeColor = [System.Drawing.Color]::FromArgb(150, 150, 150)
$subtitle.Location = New-Object System.Drawing.Point(22, 48)
$subtitle.Size = New-Object System.Drawing.Size(620, 20)
$form.Controls.Add($subtitle)

$status = New-Object System.Windows.Forms.Label
$status.Text = 'Hazır. Kuruluma başlamak için "Kur" düğmesine basın.'
$status.ForeColor = [System.Drawing.Color]::FromArgb(200, 200, 200)
$status.Location = New-Object System.Drawing.Point(22, 76)
$status.Size = New-Object System.Drawing.Size(620, 22)
$form.Controls.Add($status)

$script:LogBox = New-Object System.Windows.Forms.RichTextBox
$script:LogBox.Location = New-Object System.Drawing.Point(22, 104)
$script:LogBox.Size = New-Object System.Drawing.Size(620, 300)
$script:LogBox.ReadOnly = $true
$script:LogBox.BackColor = [System.Drawing.Color]::FromArgb(16, 16, 16)
$script:LogBox.ForeColor = [System.Drawing.Color]::FromArgb(190, 190, 190)
$script:LogBox.BorderStyle = 'FixedSingle'
$script:LogBox.Font = New-Object System.Drawing.Font('Consolas', 9)
$script:LogBox.WordWrap = $false
$form.Controls.Add($script:LogBox)

$progress = New-Object System.Windows.Forms.ProgressBar
$progress.Location = New-Object System.Drawing.Point(22, 414)
$progress.Size = New-Object System.Drawing.Size(620, 8)
$progress.Style = 'Continuous'
$progress.Value = 0
$form.Controls.Add($progress)

$btnInstall = New-Object System.Windows.Forms.Button
$btnInstall.Text = 'Kur'
$btnInstall.Location = New-Object System.Drawing.Point(22, 436)
$btnInstall.Size = New-Object System.Drawing.Size(120, 34)
$btnInstall.BackColor = [System.Drawing.Color]::FromArgb(204, 0, 0)
$btnInstall.ForeColor = [System.Drawing.Color]::White
$btnInstall.FlatStyle = 'Flat'
$btnInstall.FlatAppearance.BorderSize = 0
$form.Controls.Add($btnInstall)

$btnUninstall = New-Object System.Windows.Forms.Button
$btnUninstall.Text = 'Kaldır'
$btnUninstall.Location = New-Object System.Drawing.Point(152, 436)
$btnUninstall.Size = New-Object System.Drawing.Size(120, 34)
$btnUninstall.BackColor = [System.Drawing.Color]::FromArgb(48, 48, 48)
$btnUninstall.ForeColor = [System.Drawing.Color]::White
$btnUninstall.FlatStyle = 'Flat'
$btnUninstall.FlatAppearance.BorderSize = 0
$form.Controls.Add($btnUninstall)

$btnClose = New-Object System.Windows.Forms.Button
$btnClose.Text = 'Kapat'
$btnClose.Location = New-Object System.Drawing.Point(522, 436)
$btnClose.Size = New-Object System.Drawing.Size(120, 34)
$btnClose.BackColor = [System.Drawing.Color]::FromArgb(48, 48, 48)
$btnClose.ForeColor = [System.Drawing.Color]::White
$btnClose.FlatStyle = 'Flat'
$btnClose.FlatAppearance.BorderSize = 0
$form.Controls.Add($btnClose)

$link = New-Object System.Windows.Forms.LinkLabel
$link.Text = 'Kurulum günlüğü'
$link.Location = New-Object System.Drawing.Point(288, 446)
$link.Size = New-Object System.Drawing.Size(140, 20)
$link.LinkColor = [System.Drawing.Color]::FromArgb(120, 170, 255)
$form.Controls.Add($link)

# ---------------------------------------------------------------- log ----
# Çekirdeğin Invoke-Log'u GUI kutusuna yazar; her satırda arayüz nefes alır.
function Invoke-Log {
  param([string]$Message)
  $stamp = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'
  try { Add-Content -LiteralPath $Log -Value "$stamp $Message" -Encoding UTF8 } catch {}
  if ($script:LogBox) {
    $script:LogBox.AppendText("$(Get-Date -Format 'HH:mm:ss')  $Message`r`n")
    $script:LogBox.SelectionStart = $script:LogBox.TextLength
    $script:LogBox.ScrollToCaret()
    [System.Windows.Forms.Application]::DoEvents()
  }
}

function Set-Busy([bool]$busy, [string]$text) {
  $btnInstall.Enabled = -not $busy
  $btnUninstall.Enabled = -not $busy
  $btnClose.Enabled = -not $busy
  if ($text) { $status.Text = $text; $status.ForeColor = [System.Drawing.Color]::FromArgb(200, 200, 200) }
  if ($busy) {
    $progress.Style = [System.Windows.Forms.ProgressBarStyle]::Marquee
  } else {
    $progress.Style = [System.Windows.Forms.ProgressBarStyle]::Continuous
    $progress.Value = 0
  }
  [System.Windows.Forms.Application]::DoEvents()
}

$script:LogBox.AppendText("RuviTube kurulum sihirbazı`r`n")
$script:LogBox.AppendText("Hedef klasör: $AppDir`r`n")
$script:LogBox.AppendText("Eklenti kimliği: $ExtId`r`n`r`n")

# ---------------------------------------------------------------- eylemler ----
$btnInstall.Add_Click({
  Set-Busy $true 'Kurulum çalışıyor, lütfen bekleyin...'
  try {
    $result = Invoke-Install
    if ($result.AutoLoaded) {
      $status.Text = 'Kurulum tamamlandı — eklenti Chrome RuviTube profiline yüklendi.'
      $status.ForeColor = [System.Drawing.Color]::FromArgb(90, 220, 120)
    } else {
      $status.Text = 'Kurulum tamamlandı — eklenti elle yüklenmeli (adımlar günlükte).'
      $status.ForeColor = [System.Drawing.Color]::FromArgb(230, 200, 90)
    }
    $progress.Value = 100
  } catch {
    Invoke-Log "HATA: $($_.Exception.Message)"
    $status.Text = "Kurulum başarısız: $($_.Exception.Message)"
    $status.ForeColor = [System.Drawing.Color]::FromArgb(235, 90, 90)
  } finally {
    Set-Busy $false $null
  }
})

$btnUninstall.Add_Click({
  $answer = [System.Windows.Forms.MessageBox]::Show(
    "RuviTube kaldırılsın mı?`r`n`r`nUygulama dosyaları, native host kaydı ve masaüstü kısayolu silinir.`r`nİndirilen videolar (Downloads\RuviTube) korunur.",
    'Kaldırma onayı', 'YesNo', 'Warning')
  if ($answer -ne 'Yes') { return }

  Set-Busy $true 'Kaldırılıyor...'
  try {
    Invoke-Uninstall
    $status.Text = 'Kaldırma tamamlandı.'
    $status.ForeColor = [System.Drawing.Color]::FromArgb(200, 200, 200)
  } catch {
    Invoke-Log "HATA: $($_.Exception.Message)"
    $status.Text = "Kaldırma başarısız: $($_.Exception.Message)"
    $status.ForeColor = [System.Drawing.Color]::FromArgb(235, 90, 90)
  } finally {
    Set-Busy $false $null
  }
})

$btnClose.Add_Click({ $form.Close() })

$link.Add_LinkClicked({
  if (Test-Path -LiteralPath $Log) { Start-Process notepad.exe -ArgumentList $Log }
})

$form.Add_Shown({ $form.Activate() })
[void]$form.ShowDialog()
exit 0

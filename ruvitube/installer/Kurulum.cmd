@echo off
rem GeTube — GUI kurulum sihirbazı
powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File "%~dp0Install-GUI.ps1"
if errorlevel 2 (
  echo.
  echo GUI baslatilamadi. Konsol kurulumuna geciliyor...
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install.ps1"
  pause
)

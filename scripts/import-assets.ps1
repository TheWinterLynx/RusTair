param([string]$Source = "..\altair\site\altair")
$ErrorActionPreference = "Stop"
$dest = Join-Path $PSScriptRoot "..\assets"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$files = @("Altair1.png","LEDon.png","LEDoff.png","SwitchUp.png","SwitchDown.png","SwitchCentre.png","fan.mp3","click.mp3","powerbtn.mp3","4kbas32.bin")
foreach ($f in $files) { Copy-Item -Force (Join-Path $Source $f) (Join-Path $dest $f) }
Write-Host "Copied $($files.Count) Altair assets to $dest"

param(
    [Parameter(Mandatory = $true)]
    [string]$SimhSource,

    [string]$BuildDir = "",

    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",

    [string]$CMake = ""
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir "..\..")).Path
$buildScript = Join-Path $scriptDir "build-simh-x64.ps1"
$simh = (Resolve-Path $SimhSource).Path

if ([string]::IsNullOrWhiteSpace($BuildDir)) {
    $BuildDir = Join-Path $simh "cmake\build-rustair-simh-x64"
}

$buildArgs = @{
    SimhSource = $simh
    BuildDir = $BuildDir
    Configuration = $Configuration
}
if (-not [string]::IsNullOrWhiteSpace($CMake)) {
    $buildArgs.CMake = $CMake
}

& $buildScript @buildArgs
if ($LASTEXITCODE -ne 0) {
    throw "RusTair Open-SIMH build failed with exit code $LASTEXITCODE"
}

$resolvedBuild = (Resolve-Path $BuildDir).Path
$frontPanelDll = Join-Path $resolvedBuild "rustair-frontpanel\$Configuration\simh_frontpanel.dll"
$altairExe = Join-Path $resolvedBuild "rustair-simh\$Configuration\altair.exe"
$altairZ80Exe = Join-Path $resolvedBuild "rustair-simh\$Configuration\altairz80.exe"
$bundleDir = Join-Path $repoRoot "SIMH-backend"

foreach ($artifact in @($frontPanelDll, $altairExe, $altairZ80Exe)) {
    if (-not (Test-Path $artifact -PathType Leaf)) {
        throw "Expected runtime artifact not found: $artifact"
    }
}

New-Item -ItemType Directory -Force $bundleDir | Out-Null
Copy-Item -Force $altairExe (Join-Path $bundleDir "altair.exe")
Copy-Item -Force $altairZ80Exe (Join-Path $bundleDir "altairz80.exe")
Copy-Item -Force $frontPanelDll (Join-Path $bundleDir "simh_frontpanel.dll")

Write-Host ""
Write-Host "RusTair embedded Open-SIMH bundle updated:"
Get-ChildItem $bundleDir -File | Where-Object { $_.Name -in @("altair.exe", "altairz80.exe", "simh_frontpanel.dll") } | ForEach-Object {
    $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash
    Write-Host ("  {0,-20} {1,12} bytes  SHA256 {2}" -f $_.Name, $_.Length, $hash)
}
Write-Host ""
Write-Host "The next cargo build embeds these exact three files."

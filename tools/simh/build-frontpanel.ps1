param(
    [Parameter(Mandatory = $true)]
    [string]$SimhSource,

    [string]$BuildDir = "",

    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$inject = Join-Path $scriptDir "rustair-frontpanel.cmake"
$simh = (Resolve-Path $SimhSource).Path

if ([string]::IsNullOrWhiteSpace($BuildDir)) {
    $BuildDir = Join-Path $simh "cmake\build-vstudio"
}

if (-not (Test-Path (Join-Path $simh "CMakeLists.txt"))) {
    throw "Not an Open-SIMH source tree: $simh"
}

if (-not (Test-Path $inject)) {
    throw "Missing RusTair CMake injection file: $inject"
}

# Reconfigure the existing Open-SIMH build so the injected target is created.
# Existing generator/platform/toolchain choices in CMakeCache.txt are preserved.
& cmake -S $simh -B $BuildDir "-DCMAKE_PROJECT_INCLUDE=$inject"
if ($LASTEXITCODE -ne 0) {
    throw "Open-SIMH CMake reconfigure failed with exit code $LASTEXITCODE"
}

& cmake --build $BuildDir --config $Configuration --target simh_frontpanel
if ($LASTEXITCODE -ne 0) {
    throw "simh_frontpanel build failed with exit code $LASTEXITCODE"
}

$frontPanelDir = Join-Path (Resolve-Path $BuildDir).Path "rustair-frontpanel\$Configuration"
if (-not (Test-Path $frontPanelDir)) {
    throw "Expected FrontPanel output directory was not created: $frontPanelDir"
}

Write-Host ""
Write-Host "Open-SIMH FrontPanel library built successfully."
Write-Host "Directory: $frontPanelDir"
Write-Host ""
Write-Host "For this PowerShell session run:"
Write-Host ('  $env:RUSTAIR_SIMH_FRONTPANEL_DIR="{0}"' -f $frontPanelDir)
Write-Host ('  $env:PATH="{0};$env:PATH"' -f $frontPanelDir)
Write-Host "  cargo test --features simh-ffi"

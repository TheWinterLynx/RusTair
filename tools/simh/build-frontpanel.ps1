param(
    [Parameter(Mandatory = $true)]
    [string]$SimhSource,

    [string]$BuildDir = "",

    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",

    [string]$CMake = ""
)

$ErrorActionPreference = "Stop"

function Resolve-CMakeExecutable {
    param([string]$ExplicitPath)

    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        if (Test-Path $ExplicitPath -PathType Leaf) {
            return (Resolve-Path $ExplicitPath).Path
        }

        $explicitCommand = Get-Command $ExplicitPath -ErrorAction SilentlyContinue
        if ($null -ne $explicitCommand) {
            return $explicitCommand.Source
        }

        throw "CMake executable not found: $ExplicitPath"
    }

    $pathCommand = Get-Command cmake -ErrorAction SilentlyContinue
    if ($null -ne $pathCommand) {
        return $pathCommand.Source
    }

    # Visual Studio 2022 installs its own CMake when the C++/CMake workload is
    # present, but a normal PowerShell session does not necessarily put it on
    # PATH. Check the standard editions first so users do not have to modify
    # their global environment.
    $programFiles = ${env:ProgramFiles}
    $visualStudioEditions = @("Community", "Professional", "Enterprise", "BuildTools")
    foreach ($edition in $visualStudioEditions) {
        $candidate = Join-Path $programFiles "Microsoft Visual Studio\2022\$edition\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
        if (Test-Path $candidate -PathType Leaf) {
            return $candidate
        }
    }

    # Fall back to vswhere so non-standard Visual Studio installation roots are
    # also supported.
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere -PathType Leaf) {
        $installationPaths = & $vswhere -products * -version "[17.0,18.0)" -property installationPath
        foreach ($installationPath in $installationPaths) {
            if ([string]::IsNullOrWhiteSpace($installationPath)) {
                continue
            }
            $candidate = Join-Path $installationPath "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
            if (Test-Path $candidate -PathType Leaf) {
                return $candidate
            }
        }
    }

    throw @"
CMake was not found.
Install the Visual Studio 2022 'C++ CMake tools for Windows' component, add CMake to PATH, or pass it explicitly:
  .\tools\simh\build-frontpanel.ps1 -SimhSource <path> -CMake <path-to-cmake.exe>
"@
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$inject = Join-Path $scriptDir "rustair-frontpanel.cmake"
$simh = (Resolve-Path $SimhSource).Path
$cmakeExe = Resolve-CMakeExecutable $CMake

if ([string]::IsNullOrWhiteSpace($BuildDir)) {
    $BuildDir = Join-Path $simh "cmake\build-vstudio"
}

if (-not (Test-Path (Join-Path $simh "CMakeLists.txt"))) {
    throw "Not an Open-SIMH source tree: $simh"
}

if (-not (Test-Path $inject)) {
    throw "Missing RusTair CMake injection file: $inject"
}

Write-Host "Using CMake: $cmakeExe"
Write-Host "Open-SIMH source: $simh"
Write-Host "Open-SIMH build:  $BuildDir"

# Reconfigure the existing Open-SIMH build so the injected target is created.
# Existing generator/platform/toolchain choices in CMakeCache.txt are preserved.
& $cmakeExe -S $simh -B $BuildDir "-DCMAKE_PROJECT_INCLUDE=$inject"
if ($LASTEXITCODE -ne 0) {
    throw "Open-SIMH CMake reconfigure failed with exit code $LASTEXITCODE"
}

& $cmakeExe --build $BuildDir --config $Configuration --target simh_frontpanel
if ($LASTEXITCODE -ne 0) {
    throw "simh_frontpanel build failed with exit code $LASTEXITCODE"
}

$frontPanelDir = Join-Path (Resolve-Path $BuildDir).Path "rustair-frontpanel\$Configuration"
if (-not (Test-Path $frontPanelDir)) {
    throw "Expected FrontPanel output directory was not created: $frontPanelDir"
}

# Make the freshly built import library/DLL immediately available to Cargo and
# to test binaries launched from the same PowerShell process. Environment
# variables are process-wide, so these assignments remain valid after the
# script returns to the caller.
$env:RUSTAIR_SIMH_FRONTPANEL_DIR = $frontPanelDir
$pathEntries = $env:PATH -split ';'
if ($pathEntries -notcontains $frontPanelDir) {
    $env:PATH = "$frontPanelDir;$env:PATH"
}

Write-Host ""
Write-Host "Open-SIMH FrontPanel library built successfully."
Write-Host "Directory: $frontPanelDir"
Write-Host ""
Write-Host "Environment configured for this PowerShell session:"
Write-Host ('  RUSTAIR_SIMH_FRONTPANEL_DIR={0}' -f $env:RUSTAIR_SIMH_FRONTPANEL_DIR)
Write-Host "  FrontPanel DLL directory added to PATH"
Write-Host ""
Write-Host "Next command:"
Write-Host "  cargo test --features simh-ffi"

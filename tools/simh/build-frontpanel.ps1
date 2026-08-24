param(
    [Parameter(Mandatory = $true)]
    [string]$SimhSource,

    [string]$BuildDir = "",

    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",

    [ValidateSet("x64", "Win32")]
    [string]$Platform = "x64",

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

function Get-CachedGeneratorPlatform {
    param([string]$Directory)

    $cache = Join-Path $Directory "CMakeCache.txt"
    if (-not (Test-Path $cache -PathType Leaf)) {
        return $null
    }

    $line = Get-Content $cache | Where-Object { $_ -like "CMAKE_GENERATOR_PLATFORM:INTERNAL=*" } | Select-Object -First 1
    if ($null -eq $line) {
        return $null
    }

    return ($line -split "=", 2)[1]
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$inject = Join-Path $scriptDir "rustair-frontpanel.cmake"
$simh = (Resolve-Path $SimhSource).Path
$cmakeExe = Resolve-CMakeExecutable $CMake

if ([string]::IsNullOrWhiteSpace($BuildDir)) {
    # Do not reuse the user's ordinary Open-SIMH build. That tree may be Win32
    # (as is common for the official ALTAIR Visual Studio build), while RusTair
    # is normally x86_64-pc-windows-msvc. A DLL/import library linked into Rust
    # must match the Rust process architecture even though the child SIMH
    # simulator executable may have a different bitness.
    $suffix = if ($Platform -eq "x64") { "x64" } else { "x86" }
    $BuildDir = Join-Path $simh "cmake\build-rustair-frontpanel-$suffix"
}

if (-not (Test-Path (Join-Path $simh "CMakeLists.txt"))) {
    throw "Not an Open-SIMH source tree: $simh"
}

if (-not (Test-Path $inject)) {
    throw "Missing RusTair CMake injection file: $inject"
}

$cachedPlatform = Get-CachedGeneratorPlatform $BuildDir
if ($null -ne $cachedPlatform -and -not [string]::IsNullOrWhiteSpace($cachedPlatform) -and $cachedPlatform -ne $Platform) {
    throw "CMake build directory '$BuildDir' is configured for platform '$cachedPlatform', not '$Platform'. Use a different -BuildDir or remove that build directory."
}

Write-Host "Using CMake: $cmakeExe"
Write-Host "Open-SIMH source: $simh"
Write-Host "FrontPanel platform: $Platform"
Write-Host "FrontPanel build:    $BuildDir"

# Configure an architecture-specific Open-SIMH build tree for the reusable
# FrontPanel library. This does not alter or replace the user's existing
# altair.exe/altairz80.exe build tree.
$configureArgs = @(
    "-S", $simh,
    "-B", $BuildDir,
    "-G", "Visual Studio 17 2022",
    "-A", $Platform,
    "-DCMAKE_PROJECT_INCLUDE=$inject"
)
& $cmakeExe @configureArgs
if ($LASTEXITCODE -ne 0) {
    throw "Open-SIMH CMake configure failed with exit code $LASTEXITCODE"
}

& $cmakeExe --build $BuildDir --config $Configuration --target simh_frontpanel
if ($LASTEXITCODE -ne 0) {
    throw "simh_frontpanel build failed with exit code $LASTEXITCODE"
}

$frontPanelDir = Join-Path (Resolve-Path $BuildDir).Path "rustair-frontpanel\$Configuration"
if (-not (Test-Path $frontPanelDir)) {
    throw "Expected FrontPanel output directory was not created: $frontPanelDir"
}

$importLibrary = Join-Path $frontPanelDir "simh_frontpanel.lib"
$dll = Join-Path $frontPanelDir "simh_frontpanel.dll"
if (-not (Test-Path $importLibrary -PathType Leaf)) {
    throw "FrontPanel import library was not created: $importLibrary"
}
if (-not (Test-Path $dll -PathType Leaf)) {
    throw "FrontPanel DLL was not created: $dll"
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
Write-Host "Platform:  $Platform"
Write-Host "Directory: $frontPanelDir"
Write-Host ""
Write-Host "Environment configured for this PowerShell session:"
Write-Host ('  RUSTAIR_SIMH_FRONTPANEL_DIR={0}' -f $env:RUSTAIR_SIMH_FRONTPANEL_DIR)
Write-Host "  FrontPanel DLL directory added to PATH"
Write-Host ""
Write-Host "Note: altair.exe/altairz80.exe may remain Win32; FrontPanel communicates"
Write-Host "with the simulator out-of-process. Only this DLL must match RusTair bitness."
Write-Host ""
Write-Host "Next command:"
Write-Host "  cargo test --features simh-ffi"

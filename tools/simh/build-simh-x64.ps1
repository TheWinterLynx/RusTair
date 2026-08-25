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

    $programFiles = ${env:ProgramFiles}
    foreach ($edition in @("Community", "Professional", "Enterprise", "BuildTools")) {
        $candidate = Join-Path $programFiles "Microsoft Visual Studio\2022\$edition\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
        if (Test-Path $candidate -PathType Leaf) {
            return $candidate
        }
    }

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
  .\tools\simh\build-simh-x64.ps1 -SimhSource <path> -CMake <path-to-cmake.exe>
"@
}

function Find-UniqueBuildArtifact {
    param(
        [string]$Root,
        [string]$Name,
        [string]$Configuration
    )

    $matches = Get-ChildItem -Path $Root -Recurse -File -Filter $Name |
        Where-Object { $_.FullName -match [regex]::Escape("\$Configuration\") -or $_.DirectoryName -match [regex]::Escape($Configuration) }

    if ($matches.Count -eq 0) {
        $matches = Get-ChildItem -Path $Root -Recurse -File -Filter $Name
    }

    if ($matches.Count -eq 0) {
        throw "Build artifact not found: $Name under $Root"
    }

    $preferred = $matches | Where-Object { $_.FullName -match [regex]::Escape("\$Configuration\") } | Select-Object -First 1
    if ($null -ne $preferred) {
        return $preferred.FullName
    }

    return ($matches | Select-Object -First 1).FullName
}

function Get-CMakeCacheValue {
    param(
        [string]$Directory,
        [string]$Name
    )

    $cache = Join-Path $Directory "CMakeCache.txt"
    if (-not (Test-Path $cache -PathType Leaf)) {
        return $null
    }

    $line = Get-Content $cache | Where-Object { $_ -match ('^' + [regex]::Escape($Name) + ':[^=]+=') } | Select-Object -First 1
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
    $BuildDir = Join-Path $simh "cmake\build-rustair-simh-x64"
}

if (-not (Test-Path (Join-Path $simh "CMakeLists.txt"))) {
    throw "Not an Open-SIMH source tree: $simh"
}
if (-not (Test-Path $inject -PathType Leaf)) {
    throw "Missing RusTair CMake injection file: $inject"
}

Write-Host "Using CMake: $cmakeExe"
Write-Host "Open-SIMH source: $simh"
Write-Host "RusTair SIMH platform: x64"
Write-Host "RusTair SIMH build:    $BuildDir"

$configureArgs = @(
    "-S", $simh,
    "-B", $BuildDir,
    "-G", "Visual Studio 17 2022",
    "-A", "x64",
    "-DCMAKE_PROJECT_INCLUDE=$inject"
)

# Phase 1: let Open-SIMH decide whether it needs its dependency superbuild.
# On a fresh MSVC x64 tree this commonly includes PThreads4W, which provides
# pthread.h required by sim_frontpanel.c. Building simh_frontpanel directly
# before this phase would bypass Open-SIMH's dependency machinery.
& $cmakeExe @configureArgs
if ($LASTEXITCODE -ne 0) {
    throw "Open-SIMH x64 CMake configure failed with exit code $LASTEXITCODE"
}

$dependencyBuild = Get-CMakeCacheValue -Directory $BuildDir -Name "DO_DEPENDENCY_BUILD"
if ($dependencyBuild -eq "ON") {
    Write-Host ""
    Write-Host "Open-SIMH reports missing x64 dependencies; running official dependency superbuild first."
    & $cmakeExe --build $BuildDir --config $Configuration --target simh_superbuild
    if ($LASTEXITCODE -ne 0) {
        throw "Open-SIMH dependency superbuild failed with exit code $LASTEXITCODE"
    }

    # Phase 2: configure the real simulator build after the dependency install
    # prefix has been populated. Force dependency mode off so a still-missing
    # package is reported immediately instead of silently re-entering superbuild.
    Write-Host ""
    Write-Host "Reconfiguring Open-SIMH x64 after dependency superbuild."
    $postDependencyArgs = $configureArgs + @("-DDO_DEPENDENCY_BUILD=OFF")
    & $cmakeExe @postDependencyArgs
    if ($LASTEXITCODE -ne 0) {
        throw "Open-SIMH x64 post-dependency configure failed with exit code $LASTEXITCODE"
    }
}

# Phase 3: build only the artifacts RusTair actually consumes.
foreach ($target in @("simh_frontpanel", "altair", "altairz80")) {
    Write-Host ""
    Write-Host "Building Open-SIMH target: $target"
    & $cmakeExe --build $BuildDir --config $Configuration --target $target
    if ($LASTEXITCODE -ne 0) {
        throw "Open-SIMH target '$target' failed with exit code $LASTEXITCODE"
    }
}

$frontPanelDir = Join-Path (Resolve-Path $BuildDir).Path "rustair-frontpanel\$Configuration"
$frontPanelLib = Join-Path $frontPanelDir "simh_frontpanel.lib"
$frontPanelDll = Join-Path $frontPanelDir "simh_frontpanel.dll"
if (-not (Test-Path $frontPanelLib -PathType Leaf) -or -not (Test-Path $frontPanelDll -PathType Leaf)) {
    throw "x64 FrontPanel output was not created in $frontPanelDir"
}

$altairExe = Find-UniqueBuildArtifact -Root $BuildDir -Name "altair.exe" -Configuration $Configuration
$altairZ80Exe = Find-UniqueBuildArtifact -Root $BuildDir -Name "altairz80.exe" -Configuration $Configuration

$env:RUSTAIR_SIMH_FRONTPANEL_DIR = $frontPanelDir
$env:RUSTAIR_SIMH_ALTAIR_EXE = $altairExe
$env:RUSTAIR_SIMH_ALTAIRZ80_EXE = $altairZ80Exe

foreach ($dir in @($frontPanelDir, (Split-Path -Parent $altairExe), (Split-Path -Parent $altairZ80Exe))) {
    if (($env:PATH -split ';') -notcontains $dir) {
        $env:PATH = "$dir;$env:PATH"
    }
}

Write-Host ""
Write-Host "RusTair Open-SIMH x64 stack built successfully."
Write-Host ""
Write-Host "FrontPanel DLL: $frontPanelDll"
Write-Host "Classic Altair: $altairExe"
Write-Host "AltairZ80:      $altairZ80Exe"
Write-Host ""
Write-Host "Environment configured for this PowerShell session:"
Write-Host "  RUSTAIR_SIMH_FRONTPANEL_DIR=$env:RUSTAIR_SIMH_FRONTPANEL_DIR"
Write-Host "  RUSTAIR_SIMH_ALTAIR_EXE=$env:RUSTAIR_SIMH_ALTAIR_EXE"
Write-Host "  RUSTAIR_SIMH_ALTAIRZ80_EXE=$env:RUSTAIR_SIMH_ALTAIRZ80_EXE"
Write-Host ""
Write-Host "Next command:"
Write-Host "  cargo test --features simh-ffi"

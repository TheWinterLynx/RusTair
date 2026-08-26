param(
    [Parameter(Mandatory = $true)]
    [string]$SimhSource,

    [string]$CMake = ""
)

$ErrorActionPreference = "Stop"

function Resolve-CMakeExecutable {
    param([string]$ExplicitPath)

    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        if (Test-Path $ExplicitPath -PathType Leaf) {
            return (Resolve-Path $ExplicitPath).Path
        }
        $command = Get-Command $ExplicitPath -ErrorAction SilentlyContinue
        if ($null -ne $command) {
            return $command.Source
        }
        throw "CMake executable not found: $ExplicitPath"
    }

    $command = Get-Command cmake -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Source
    }

    foreach ($edition in @("Community", "Professional", "Enterprise", "BuildTools")) {
        $candidate = Join-Path ${env:ProgramFiles} "Microsoft Visual Studio\2022\$edition\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
        if (Test-Path $candidate -PathType Leaf) {
            return $candidate
        }
    }

    throw "CMake was not found in PATH or Visual Studio 2022."
}

function Get-CacheValue {
    param([string]$BuildDir, [string]$Name)
    $cache = Join-Path $BuildDir "CMakeCache.txt"
    if (-not (Test-Path $cache -PathType Leaf)) {
        return $null
    }
    $line = Get-Content $cache | Where-Object { $_ -match ('^' + [regex]::Escape($Name) + ':[^=]+=') } | Select-Object -First 1
    if ($null -eq $line) {
        return $null
    }
    return ($line -split "=", 2)[1]
}

function Invoke-CMakeChecked {
    param([string[]]$Arguments, [string]$Label)
    $output = & $script:CMakeExe @Arguments 2>&1
    $exit = $LASTEXITCODE
    $output | Select-String -Pattern "RusTair|error|failed|fatal|warning" -CaseSensitive:$false | ForEach-Object { $_.Line }
    if ($exit -ne 0) {
        throw "$Label failed with exit code $exit"
    }
}

function Build-Variant {
    param(
        [string]$Name,
        [string]$BuildDir,
        [string]$Injection
    )

    Write-Host ""
    Write-Host "=== BUILD $Name ==="
    $configure = @(
        "-S", $script:Simh,
        "-B", $BuildDir,
        "-G", "Visual Studio 17 2022",
        "-A", "x64",
        "-DCMAKE_PROJECT_INCLUDE=$Injection"
    )
    Invoke-CMakeChecked -Arguments $configure -Label "$Name configure"

    if ((Get-CacheValue -BuildDir $BuildDir -Name "DO_DEPENDENCY_BUILD") -eq "ON") {
        Invoke-CMakeChecked -Arguments @("--build", $BuildDir, "--config", "Release", "--target", "simh_superbuild") -Label "$Name dependency build"
        Invoke-CMakeChecked -Arguments ($configure + @("-DDO_DEPENDENCY_BUILD=OFF")) -Label "$Name post-dependency configure"
    }

    foreach ($target in @("simh_frontpanel", "altair", "altairz80")) {
        Invoke-CMakeChecked -Arguments @("--build", $BuildDir, "--config", "Release", "--target", $target) -Label "$Name $target build"
    }
}

function Use-Variant {
    param([string]$BuildDir)
    $front = Join-Path $BuildDir "rustair-frontpanel\Release"
    $simulators = Join-Path $BuildDir "rustair-simh\Release"
    $env:RUSTAIR_SIMH_FRONTPANEL_DIR = $front
    $env:RUSTAIR_SIMH_ALTAIR_EXE = Join-Path $simulators "altair.exe"
    $env:RUSTAIR_SIMH_ALTAIRZ80_EXE = Join-Path $simulators "altairz80.exe"
    $env:PATH = "$front;$simulators;$env:PATH"
}

function Run-Test {
    param([string]$Label, [string]$TestName)
    Write-Host ""
    Write-Host "=== TEST $Label ==="
    Push-Location $script:RepoRoot
    try {
        $output = & cargo test --color never --features simh-ffi --test $TestName -- --ignored --nocapture 2>&1
        $exit = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }

    $output | Select-String -Pattern "FrontPanel diagnostic|M2SIO RX diagnostic|smoke test passed|Simulation stopped|Error:|FAILED|test result:" | ForEach-Object { $_.Line }
    if ($exit -eq 0) {
        Write-Host "RESULT $Label: PASS"
        return $true
    }
    Write-Host "RESULT $Label: FAIL"
    return $false
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$script:RepoRoot = (Resolve-Path (Join-Path $scriptDir "..\..")).Path
$script:Simh = (Resolve-Path $SimhSource).Path
$script:CMakeExe = Resolve-CMakeExecutable $CMake

$upstreamInjection = (Resolve-Path (Join-Path $scriptDir "rustair-upstream-frontpanel.cmake")).Path
$parserInjection = (Resolve-Path (Join-Path $scriptDir "rustair-frontpanel.cmake")).Path
$upstreamBuild = Join-Path $script:Simh "cmake\build-rustair-upstream-check-x64"
$parserBuild = Join-Path $script:Simh "cmake\build-rustair-parser-only-check-x64"

Write-Host "Open-SIMH: $script:Simh"
Write-Host "CMake:     $script:CMakeExe"
Write-Host ""
Write-Host "Variant A: upstream sim_frontpanel.c + upstream sim_timer.c"
Write-Host "Variant B: RusTair FrontPanel parser patch + upstream sim_timer.c"
Write-Host "No scheduler/RX diagnostic patches are included in either variant."

Build-Variant -Name "A UPSTREAM" -BuildDir $upstreamBuild -Injection $upstreamInjection
Use-Variant -BuildDir $upstreamBuild
$aFront = Run-Test -Label "A upstream / classic FrontPanel" -TestName "simh_frontpanel_smoke"
$aZ80 = Run-Test -Label "A upstream / AltairZ80 FrontPanel" -TestName "simh_altairz80_smoke"

Build-Variant -Name "B PARSER-ONLY" -BuildDir $parserBuild -Injection $parserInjection
Use-Variant -BuildDir $parserBuild
$bFront = Run-Test -Label "B parser-only / classic FrontPanel" -TestName "simh_frontpanel_smoke"
$bZ80 = Run-Test -Label "B parser-only / AltairZ80 FrontPanel" -TestName "simh_altairz80_smoke"
$bSerial = Run-Test -Label "B parser-only / M2SIO serial" -TestName "simh_altairz80_serial_smoke"

Write-Host ""
Write-Host "=== COMPATIBILITY SUMMARY ==="
Write-Host "A upstream classic FrontPanel : $(if ($aFront) {'PASS'} else {'FAIL'})"
Write-Host "A upstream AltairZ80         : $(if ($aZ80) {'PASS'} else {'FAIL'})"
Write-Host "B parser-only classic        : $(if ($bFront) {'PASS'} else {'FAIL'})"
Write-Host "B parser-only AltairZ80      : $(if ($bZ80) {'PASS'} else {'FAIL'})"
Write-Host "B parser-only M2SIO          : $(if ($bSerial) {'PASS'} else {'FAIL'})"
Write-Host ""
if (-not $aFront -and $bFront) {
    Write-Host "Parser conclusion: REQUIRED for this Open-SIMH revision."
} elseif ($aFront -and $aZ80) {
    Write-Host "Parser conclusion: NOT REQUIRED by these regression tests."
} else {
    Write-Host "Parser conclusion: INCONCLUSIVE; inspect the failing A/B test."
}

if ($bFront -and $bZ80 -and $bSerial) {
    Write-Host "Timer conclusion: NOT REQUIRED by the full parser-only regression set."
    Write-Host "The production-compatible simulator core can remain upstream/unmodified."
} else {
    Write-Host "Timer conclusion: NOT YET CLEARED; parser-only regression set has a failure."
}

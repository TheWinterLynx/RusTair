# RusTair embedded Open-SIMH bundle

This directory contains the Windows x64 Open-SIMH runtime embedded into RusTair with `include_bytes!`.

## Upstream

- Repository: `https://github.com/open-simh/simh`
- Upstream commit: `a1f57fa3738ed31148d31126ba1a7278ff845c6d`
- RusTair bundle revision: `a1f57fa3-rustair1`
- Build: Release / AMD64 / Visual Studio 2022

## Runtime files

- `altair.exe`
- `altairz80.exe`
- `simh_frontpanel.dll`

The executables and DLL have no non-Windows runtime DLL dependencies. They use Windows system libraries only.

## RusTair compatibility patches

RusTair builds these files from an otherwise upstream Open-SIMH tree using build-tree-only CMake substitutions. The Open-SIMH checkout itself is not modified.

1. `sim_frontpanel.c`: FrontPanel EXAMINE parsing uses the final `:` in a response (`strrchr`) so classic Altair symbolic diagnostic lines cannot hide the actual examined value.
2. `sim_timer.c`: the FrontPanel/REMOTE MASTER stop scheduler ignores the default `sim_timer_stop_time == 0.0`; this prevents a temporary negative `sim_gtime()` during startup from scheduling an unintended stop.

No RusTair patches are applied to the 8080/Z80 CPU implementation, MITS 88-2SIO, or TMXR serial implementation.

## Validation

The compatibility matrix for this revision established:

- upstream classic Altair FrontPanel: FAIL
- upstream AltairZ80 FrontPanel: PASS
- parser-only classic Altair FrontPanel: PASS
- parser-only AltairZ80 FrontPanel: PASS
- parser-only AltairZ80 M2SIO: FAIL
- parser + timer guard AltairZ80 M2SIO: PASS

The retained regression suite covers classic FrontPanel memory/register operations, AltairZ80 in 8080/Z80 modes, both M2SIO ports in both directions, and serial reconnect after power cycling.

## Updating Open-SIMH

Do not blindly rebuild and replace these files. First run:

```powershell
.\tools\simh\check-upstream-compat.ps1 -SimhSource "<path-to-open-simh>"
```

This re-tests upstream, parser-only, and parser+timer variants. Remove any RusTair compatibility patch that is no longer needed by the new upstream revision. Then build the validated production variant with:

```powershell
.\tools\simh\build-simh-x64.ps1 -SimhSource "<path-to-open-simh>"
```

Run the SIMH smoke tests, replace only the three runtime files above, update the upstream commit and bundle revision in this file and `src/backend/simh/runtime.rs`, and commit the new bundle.

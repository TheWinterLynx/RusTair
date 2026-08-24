# Intel 8080 CPU diagnostics

RusTair can run classic CP/M `.COM` CPU diagnostics without booting CP/M and without intercepting the CPU core.

Use **File → CPU diagnostics → Load .COM via Port 0…** (or Port 1 with the MITS 88-2SIO). The loader pauses any currently running guest while the native picker is open. Cancelling resumes the previous RUN state. Selecting a file performs a deterministic diagnostic boot: power on if required, STOP, RESET CPU/I/O, clear installed RAM, install a small 8080 page-zero shim, load the `.COM` at `0100h`, set `PC=0000h`, then RUN.

The shim provides only the CP/M services used by the traditional diagnostics:

- `CALL 0005h`, `C=2`: console output of the character in `E`.
- `CALL 0005h`, `C=9`: console output of the `$`-terminated string at `DE`.

Those services are themselves ordinary Intel 8080 instructions. Output polls and writes the configured emulated MITS serial board, so the test travels through the same serial hardware and Serial Router as other Altair software. No `PC=0005h` host interception is used.

The page-zero bootstrap also changes address `0000h` to `HLT` before entering the `.COM`. Diagnostics that finish through the CP/M warm-boot vector therefore halt cleanly.

## Serial ports

- MITS 88-SIO Port 0: status `00h`, data `01h`; the shim waits while TX busy bits `C0h` are set.
- MITS 88-2SIO Port 0: status/control `10h`, data `11h`; the shim waits for TX-ready bit `02h`.
- MITS 88-2SIO Port 1: status/control `12h`, data `13h`; the shim waits for TX-ready bit `02h`.

The loader never changes the user's serial cabling. It reveals whichever ASR-33, Text Terminal, External TCP or External COM endpoint is already attached to the selected port.

## Recommended diagnostics

The classic test collection is mirrored by several emulator projects and archival sites. A convenient source is the `cpu_tests` directory in `superzazu/8080`, which contains:

1. `8080PRE.COM` — preliminary instruction tests.
2. `TST8080.COM` — Microcosm Associates 8080/8085 CPU diagnostic.
3. `CPUTEST.COM` — Supersoft Associates CPU diagnostic.
4. `8080EXM.COM` — 8080 instruction exerciser with expected CRCs embedded.

Run them in that order when establishing a baseline. `8080EXM.COM` is intentionally very long at an authentic 2 MHz; use RusTair's Unlimited host execution mode when a wall-clock faithful run is not required.

## RAM

The `.COM` is loaded at `0100h`. RusTair checks that the image fits in installed RAM and reserves the upper 256 bytes for the diagnostic stack. Use 64 KiB when running the complete set so memory size is not an artificial limitation.

Each selected diagnostic starts with clean zero-filled installed RAM, so loading a smaller `.COM` after a larger one cannot observe stale bytes from the previous test.

## Why this exists

These diagnostics provide a repeatable semantic baseline for the current instruction-granular Intel 8080 core. The same `.COM` images and expected output can later be run unchanged against a T-state/cycle-accurate core, allowing the CPU implementation to evolve without silently regressing instruction behavior.

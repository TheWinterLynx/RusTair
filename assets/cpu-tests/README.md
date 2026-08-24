# Intel 8080 CPU diagnostics

RusTair embeds a permanent Intel 8080 regression suite and can also run arbitrary external CP/M `.COM` CPU diagnostics without booting CP/M and without intercepting the CPU core.

Use **File → CPU diagnostics**. The menu provides:

- **Test speed → Unlimited / Authentic 2 MHz**. This is a diagnostic-only execution override and does not change the normal emulator speed preference.
- **Serial output** selection for MITS 88-SIO Port 0 or either MITS 88-2SIO port.
- **Run full CPU diagnostic suite**.
- **Run individual test** for each embedded classic image or the RusTair control-line baseline.
- **Load external `.COM`…** for additional diagnostics.
- **Abort running diagnostic / suite** for long 2 MHz runs.

The four classic binary images live directly in this directory and are compiled into the executable with `include_bytes!`:

1. `8080PRE.COM` — preliminary instruction tests.
2. `TST8080.COM` — Microcosm Associates 8080/8085 CPU diagnostic.
3. `CPUTEST.COM` — Supersoft Associates CPU diagnostic.
4. `8080EXM.COM` — modified 8080 instruction exerciser with expected CRCs embedded.

`8080EXER.COM` is intentionally not part of the embedded menu because that original exerciser does not contain the expected CRC table needed for direct PASS/FAIL reporting; `8080EXM.COM` is the useful regression variant.

## Full suite

The full suite runs, in order:

1. RusTair 8080 control-line baseline.
2. `8080PRE.COM`.
3. `TST8080.COM`.
4. `CPUTEST.COM`.
5. `8080EXM.COM`.

The classic tests execute sequentially on the normal Altair machine. A final suite window reports PASS/FAIL for the control-line checks and exact instruction/T-state reference matching for every `.COM` test.

At authentic 2 MHz the full suite is intentionally very long because `8080EXM.COM` alone represents about 3 h 18 min of Intel 8080 time. Use **Unlimited** for development/regression runs and **Authentic 2 MHz** when wall-clock behaviour itself is under test.

The complete suite requires at least **32 KiB RAM** because `CPUTEST.COM` is 19,200 bytes and the loader also reserves high memory for the stack and mini-BDOS.

## RusTair control-line baseline

The RusTair-owned test complements the classic CP/M diagnostics, which concentrate on the instruction set. It currently freezes these behaviours before the cycle-accurate core work begins:

- delayed `EI` enable and `DI` disable behaviour;
- `HLT` acknowledgement and wake-up by an accepted interrupt;
- interrupt acknowledge using a real `RST` opcode and the resulting stack push;
- `IN` / `OUT` CPU-to-bus contract;
- current front-panel `READY/WAIT` run/stop gating;
- `HOLD/HLDA` arbitration and CPU freeze while HOLD is active.

The current CPU is instruction-granular, so the READY/WAIT check deliberately validates only the existing machine/front-panel contract. It does **not** claim true T-state `TW` insertion. The future cycle-accurate core will keep this baseline while adding pin- and T-state-level validation.

## CP/M diagnostic environment

Every classic `.COM` test is loaded at `0100h` after a deterministic diagnostic boot: power on if required, STOP, RESET CPU/I/O, clear installed RAM, install CP/M-compatible page zero, load the `.COM`, install the mini-BDOS in high memory, set `PC=0000h`, then RUN.

The shim provides only the CP/M services used by the traditional diagnostics:

- `CALL 0005h`, `C=2`: console output of the character in `E`.
- `CALL 0005h`, `C=9`: console output of the `$`-terminated string at `DE`.

Those services are themselves ordinary Intel 8080 instructions. Output polls and writes the configured emulated MITS serial board, so the test travels through the same serial hardware and Serial Router as other Altair software. No `PC=0005h` CPU interception is used.

The BDOS entry is deliberately placed in high memory and address `0005h` contains a real `JMP BDOS`, as CP/M software expects. This matters for programs such as `8080EXM.COM`, which read bytes `0006h/0007h` and derive their stack/high-memory limit from the BDOS vector.

The page-zero bootstrap changes address `0000h` to `HLT` before entering the `.COM`. Diagnostics that finish through the CP/M warm-boot vector therefore halt cleanly.

## Serial ports

- MITS 88-SIO Port 0: status `00h`, data `01h`; the shim waits while TX busy bits `C0h` are set.
- MITS 88-2SIO Port 0: status/control `10h`, data `11h`; the shim waits for TX-ready bit `02h`.
- MITS 88-2SIO Port 1: status/control `12h`, data `13h`; the shim waits for TX-ready bit `02h`.

The loader never changes the user's serial cabling. It reveals whichever ASR-33, Text Terminal, External TCP or External COM endpoint is already attached to the selected port.

## Reference instruction and T-state totals

When one of the four known diagnostics completes, RusTair compares the measured instruction count and Intel 8080 T-state total against the established reference harness totals:

| Diagnostic | Instructions | T-states |
| --- | ---: | ---: |
| `TST8080.COM` | 651 | 4,924 |
| `8080PRE.COM` | 1,061 | 7,817 |
| `CPUTEST.COM` | 33,971,311 | 255,653,383 |
| `8080EXM.COM` | 2,919,050,698 | 23,803,381,171 |

A correct run shows `REFERENCE MATCH` with a difference of zero for both metrics.

The comparison counter intentionally follows the conventional CP/M test harness rather than charging the implementation details of RusTair's richer serial shim. Counting begins at `0100h`; each call through the CP/M vector at `0005h` is normalized to the reference `OUT 1` + `RET` pair (20 T-states), and the final warm boot at `0000h` is normalized to the reference `OUT 0` (10 T-states). The guest still executes RusTair's real high-memory mini-BDOS, UART status polling and serial output. Only the reported comparison counters are normalized, so UART speed cannot alter the expected CPU-test totals.

For unknown external `.COM` diagnostics RusTair still reports measured normalized instruction/T-state counts, but does not claim a reference match when no expected totals are registered.

## RAM

The `.COM` is loaded at `0100h`. RusTair checks that the image fits below the reserved CP/M high-memory area. The loader reserves the upper 256 bytes for the mini-BDOS plus a further 256-byte stack/guard area below it.

Each selected diagnostic starts with clean zero-filled installed RAM, so loading a smaller `.COM` after a larger one cannot observe stale bytes from the previous test.

## Why this exists

These diagnostics form the frozen semantic and aggregate-timing baseline for the current instruction-granular Intel 8080 core. The same embedded images, expected output, instruction counts, T-state totals and control-line checks can later run against the T-state/cycle-accurate core, allowing the CPU implementation to evolve without silently regressing established behaviour.

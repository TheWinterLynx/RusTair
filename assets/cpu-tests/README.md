# Intel 8080 CPU diagnostics

RusTair can run the classic CP/M `.COM` Intel 8080 diagnostics without booting CP/M.

## Test files

Download the original test binaries from the Altair Clone CPU-test archive:

- `8080PRE.COM` — preliminary/basic instruction test
- `TST8080.COM` — Microcosm 8080/8085 CPU diagnostic
- `CPUTEST.COM` — Diagnostics II CPU test
- `8080EXM.COM` — 8080 instruction exerciser with expected CRC values and PASS/FAIL output

Archive: <https://altairclone.com/downloads/cpu_tests/>

The historical source page is preserved at:
<https://web.archive.org/web/20151006085348/http://www.idb.me.uk/sunhillow/8080.html>

The binaries are intentionally not vendored into RusTair. This directory documents the canonical external test set while the emulator provides a generic loader for any compatible CP/M 8080 diagnostic `.COM` file.

## Running a diagnostic

1. Configure enough RAM for the selected `.COM` file. 64 KiB is recommended for a common baseline.
2. Configure either `MITS 88-SIO` or `MITS 88-2SIO`.
3. Connect a visible endpoint (Text Terminal or ASR-33) to the serial port you intend to use.
4. Select `File -> CPU diagnostics -> Load .COM via Port 0...` (or Port 1 on the 88-2SIO).
5. Select the downloaded `.COM` file.

RusTair then:

- resets the machine;
- installs a real 8080 page-zero shim;
- installs a CP/M-compatible `CALL 0005h` vector;
- implements BDOS function 2 (character output) and function 9 (`$`-terminated string output) in 8080 machine code;
- polls the selected emulated serial card's TX-ready status and writes through its real data port;
- loads the selected `.COM` at `0100h`;
- sets a high stack inside installed RAM;
- starts execution at `0000h`;
- replaces the warm-boot entry at `0000h` with `HLT` before entering the test, so a normal CP/M warm boot at test completion stops the CPU cleanly.

No CPU opcode, program-counter, BDOS or console call is intercepted on the host side. The diagnostics execute on the normal RusTair Intel 8080 core and reach the terminal through the normal emulated 88-SIO/88-2SIO path.

## Recommended baseline order

1. `8080PRE.COM`
2. `TST8080.COM`
3. `CPUTEST.COM`
4. `8080EXM.COM`

Run these against the current instruction-level core before replacing it with the cycle-accurate implementation. Record output and, where useful, total emulated cycles. The same suite can then be used as a regression baseline for the new core.

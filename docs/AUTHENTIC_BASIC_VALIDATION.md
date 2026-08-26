# Authentic BASIC 3.2 end-to-end validation

This validation deliberately keeps the Microsoft BASIC paper-tape image outside the RusTair repository.

## Normal regression suite

`cargo test` includes deterministic tests for:

- MITS BASIC 3.2 tape layout parsing;
- program-record checksum validation;
- premature end-of-tape / missing Go Record;
- wrong serial-board bootstrap not consuming the selected UART;
- 88-2SIO Port 1 not satisfying the Port 0 bootstrap;
- the bundled Quick Load image remaining a complete non-empty 4 KiB image;
- the existing in-module bootstrap/front-panel/UART tests for both Rust engines.

## External real-tape regression

Set `RUSTAIR_BASIC32_TAP` to a legally obtained `4K BASIC Ver 3-2.tap` and run the ignored test explicitly.

PowerShell example:

```powershell
$env:RUSTAIR_BASIC32_TAP="C:\path\to\4K BASIC Ver 3-2.tap"; cargo test authentic_basic32_real_tape_matches_bundled_program_on_both_engines_and_boards -- --ignored --nocapture
```

The test does **not** call the application's Quick Load path to construct the authentic machine state. It:

1. Locates the BASIC 3.2 `AEh` leader in the external tape.
2. Validates the MITS program records (`3Ch`, count, little-endian address, data, checksum) and the final `78h` Go Record.
3. Cross-checks every RAM byte explicitly carried by those tape records against the bundled `assets/4kbas32.bin` Quick Load reference image.
4. For each combination of:
   - RusTair Fast 8080 + 88-SIO,
   - RusTair Fast 8080 + 88-2SIO,
   - RusTair Cycle Accurate 8080 + 88-SIO,
   - RusTair Cycle Accurate 8080 + 88-2SIO,
   it enters the historical bootstrap through `EXAMINE` / `DEPOSIT` / `DEPOSIT NEXT`, sets the documented sense byte, and starts the CPU.
5. Feeds the external tape byte-by-byte through `BackendSerialPort::Port0` only when guest RX is empty, so progress depends on real guest `IN` instructions rather than direct RAM writes.
6. Stops before the Go Record to verify all program-record RAM bytes against the tape.
7. Feeds the three-byte Go Record and verifies that execution leaves the temporary checksum-loader page and enters the BASIC startup path at the tape's documented `0000h` Go address.

A pass therefore demonstrates that the real tape, the historical bootstrap, both Rust CPU engines, both supported MITS serial boards and the Quick Load reference payload agree without using direct-RAM shortcuts in the authentic path.

## Historical tape format reference

The record parser follows Martin Eberhard's *Altair Paper Tape Format* description: a program load record starts with `3Ch`, followed by byte count, low/high load address, data and an 8-bit sum checksum; the Go Record is `78h` followed by the low/high execution address.

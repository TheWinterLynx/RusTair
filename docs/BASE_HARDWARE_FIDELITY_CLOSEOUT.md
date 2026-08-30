# RusTair base hardware fidelity closeout

Branch baseline: `main` at `71e5833c7d0abafbc998a4d629381dcacf615517`.

This document is the live evidence log for `agent/base-hardware-fidelity-closeout`. It deliberately separates electrical truth from presentation and compatibility behavior. A feature is not marked closed merely because software happens to run.

## Rules

1. Prefer original MITS/Intel/Motorola documentation over another emulator.
2. Another emulator may identify a question or provide a differential oracle, but is not the hardware authority.
3. Host/debugger inspection must distinguish absent hardware from guest-visible bus values.
4. Fast may reconstruct unavailable sub-instruction detail, but Cycle Accurate must not invent electrical state.
5. Presentation state (LED persistence, animation, audio) is downstream of electrical state and must never feed it back.
6. Compatibility workarounds remain explicit and optional.

## 1. S-100 open bus — PASS

### Historical evidence

The 1975 MITS *Altair 8800 Theory of Operation Manual & Schematics* documents tri-state bus drivers on the CPU board, separate data-in/data-out paths, and the ability to disable bus drivers. MITS *Computer Notes* additionally warns that when a program runs past installed memory it begins executing octal `377` (`FFh`, `RST 7`) in nonexistent memory. Taken together, this is direct evidence that an unresponded memory read is not a fabricated zero byte.

### Model contract

- Physical/debugger `peek_memory(address)` returns `None` when no RAM card is installed at that address.
- A guest memory read from an unresponded address observes `FFh`.
- Opcode and stack reads use the same guest-visible value.
- Writes to uninstalled memory are ignored and do not create storage.
- A memory card that is not selected contributes no PRDY wait states.
- An I/O read for which the installed card set has no decoder response observes `FFh`.
- A device that *does* decode the port owns the returned value; open-bus policy must not replace a legitimate zero/status value.
- Power-on and RESET-release panel projections use the same guest-visible bus resolution rather than a local zero fallback.
- Cycle Accurate exposes `FFh` on S-100 DI before the CPU samples the byte, not only as a post-hoc CPU return value.

### Regression coverage

- `src/machine/memory.rs`: physical absence vs guest-visible `FFh`, ignored writes, no wait-state contribution.
- `src/machine/io_devices.rs`: inactive serial-board address ranges and arbitrary unmapped ports resolve to `FFh`.
- `tests/open_bus_fidelity.rs`: public guest paths for memory, opcode, stack and I/O plus exact Cycle T2/T3 open-bus visibility.
- `tests/debugger_execution.rs`: debugger still sees absent RAM as `None` while watched guest reads observe `FFh` in both engines.

### Validation

The normal local `cargo test` suite passed on 2026-08-30 after the old `00h` debugger expectation was corrected. Subsequent timing work does not change the open-bus contract and keeps these regressions in the normal suite.

## 2. Front-panel LED electrical duty and optical persistence — PASS

Electrical activity and presentation are now separate layers:

- the former 32,000-T-state first-window cap is gone, so accelerated execution cannot bias duty toward the first samples of a host frame;
- `raw_panel_lamp_duty()` exposes deterministic electrical duty independently of optical persistence;
- raw counters use saturating full-window accounting and are reset only after commit/freeze;
- a >32,000-sample regression proves that late activity still contributes to the result;
- a reversed-order regression proves raw duty is sample-order invariant;
- Fast-vs-Cycle coverage compares a fixed two-NOP/eight-T-state sequence where both backends possess equivalent ADDRESS/STATUS information;
- optical persistence remains presentation-only and uses the real host frame interval rather than a fabricated fixed 16 ms interval.

The full local `cargo test` suite passed on 2026-08-30 with these regressions enabled. Physical brightness/aura calibration remains a presentation-quality task, not an electrical-fidelity blocker.

## 3. Authentic 2 MHz long-term clock — PASS

The old runtime used `min(20 ms)` and converted that clipped duration directly to a `u32` budget, permanently discarding host stalls and fractional T-states. The replacement clock is a fixed-point host-to-T-state scheduler:

- one emulated T-state equals one billion accumulator units, so nanosecond intervals retain fractional cycles without `f64` rounding loss;
- all elapsed throttled wall-clock time is retained as signed debt;
- authentic execution is bounded to 40,000 T-states per UI update while remaining debt survives subsequent updates;
- a 100 ms host stall at 2 MHz creates exactly 200,000 T-states of debt;
- actual backend T-state deltas are subtracted, so Fast whole-instruction overshoot becomes a small negative balance and is repaid instead of creating long-term drift;
- stopped time is discarded and blocked RUN states that execute zero T-states do not become catch-up bursts;
- effective clock/speed changes create a fresh debt epoch so work accrued under one rate cannot leak into another;
- Unlimited remains explicitly detached from wall-clock throttling;
- POWER, physical RUN/STOP and RESET fence the execution clock at the operator-action boundary;
- the installed `CpuBoard::clock_hz()` remains the physical timing authority; the scheduler contains no private fixed 2 MHz production constant.

Deterministic coverage includes 100 ms stall retention, fractional-cycle carry, Fast overshoot repayment, stopped-time discard, 10x scaling, rate-epoch changes and Unlimited independence. The focused clock-authority test and the full local `cargo test` suite passed on 2026-08-30.

## 4. 88-2SIO / MC6850 — IN PROGRESS

### Primary hardware evidence

The March 1977 MITS *88-2SIO Documentation* describes a board-level input wait generator in addition to the two MC6850 ACIAs. During an `IN`, SINP clocks flip-flop V and forces S-100 PRDY low; the processor remains in WAIT for 500 ns. PWAIT then clears V and releases PRDY. MITS explicitly states that this wait is used only during input to satisfy address setup time. At the stock 2 MHz MITS 8080 clock, 500 ns is exactly one additional TW.

### S-100 timing implemented in this branch

- only decoded 88-2SIO addresses `10h`–`13h` generate the input wait;
- 88-SIO accesses do not inherit it;
- unmapped I/O does not pull PRDY low;
- `OUT` does not wait;
- Fast adds the documented single external wait T-state to instruction elapsed time while remaining explicitly reconstructed at sub-instruction level;
- Cycle combines RAM-card and I/O-card PRDY contributions at the S-100 arbitration point, producing a real `T1 -> T2 -> TW -> T3` input machine cycle;
- in Cycle, READY is low at the T2 sampling point, WAIT is a real CPU output in TW, and PWAIT releases PRDY during that sole TW;
- `tests/two_sio_prdy_timing.rs` guards total T-state parity and the exact Cycle sequence.

This S-100 timing work is not sufficient to call the complete 88-2SIO electrically faithful. Full closeout still requires Motorola/MITS ACIA behavior:

- control-word clock divide and word format;
- real transmit data register + transmit shift register semantics;
- real receive shift register + receive data register semantics;
- RDRF / TDRE timing owned by the emulated card rather than ASR/Terminal/TCP/COM presentation timing;
- overrun, framing and parity status;
- CTS, DCD, RTS and BREAK behavior where exposed by the board;
- IRQ status/enable behavior and master reset;
- baud-clock/jumper timing and deterministic conformance tests for both ACIAs/ports.

## 5. MITS 88-SIO — OPEN

The 88-SIO must be audited by hardware revision rather than treated as a generic UART. Required closeout:

- establish the exact supported MITS revision(s) from primary documentation;
- verify status-bit polarity and control semantics;
- model finite receive/transmit hardware and overrun/error behavior where applicable;
- move serial timing authority from ASR/Terminal/TCP/COM endpoints into the emulated card;
- document any deliberate compatibility mode separately from historical behavior.

## Validation policy

Do not run GitHub Actions for this branch. Each checkpoint should be validated locally with focused tests first and then the normal project validation (`cargo test`; release launch/build as appropriate). Long ignored CPU diagnostics are re-run only when a change can plausibly affect CPU semantics/timing or before a release certification pass.

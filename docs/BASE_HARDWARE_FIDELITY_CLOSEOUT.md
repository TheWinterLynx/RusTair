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

## 2. Front-panel LED electrical duty and optical persistence — IMPLEMENTED, LOCAL VALIDATION REQUIRED

Implemented closeout work:

- removed the former 32,000-T-state first-window cap, so accelerated execution no longer biases duty toward the first samples of a host frame;
- added a deterministic raw electrical-duty snapshot separate from the wall-clock-persistent visible snapshot;
- raw counters use saturating full-window accounting and are reset only after commit/freeze;
- added Fast-vs-Cycle parity coverage for a fixed two-NOP/eight-T-state sequence where both backends possess equivalent ADDRESS/STATUS information;
- optical persistence remains downstream presentation and never feeds CPU/S-100 state;
- `draw_altair` now commits persistence with the real host frame interval rather than a fixed 16 ms constant.

Still required before final PASS:

- local suite validation of the new raw-duty/parity tests and real-frame integration;
- retain a deterministic long-window/order test proving no regression to truncated sampling;
- physical/calibrated LED response remains a separate presentation-quality question and must not be confused with electrical-duty correctness.

## 3. Authentic 2 MHz long-term clock — IMPLEMENTED, LOCAL VALIDATION REQUIRED

The old runtime used `min(20 ms)` and converted that clipped duration directly to a `u32` budget, permanently discarding host stalls and fractional T-states. The replacement clock is a fixed-point host-to-T-state scheduler:

- one emulated T-state equals one billion accumulator units, so nanosecond intervals retain fractional cycles without `f64` rounding loss;
- all elapsed throttled wall-clock time is retained as signed debt;
- authentic execution is bounded to 40,000 T-states per UI update, while 5x/10x preserve the previous scaled responsiveness bounds;
- a 100 ms host stall at 2 MHz creates 200,000 T-states of debt; only 40,000 are serviced at once and the remainder survives subsequent updates;
- actual backend T-state deltas are subtracted, so Fast whole-instruction overshoot becomes a small negative balance and is repaid instead of creating long-term positive drift;
- stopped time is discarded and blocked RUN states that execute zero T-states do not become catch-up bursts;
- clock/speed changes create a fresh debt epoch so debt accrued at one effective rate cannot leak into another;
- Unlimited remains explicitly detached from wall-clock throttling and keeps its bounded immediate repaint chunks;
- POWER, physical RUN/STOP and RESET fence the execution clock at the operator action boundary.

Deterministic unit coverage includes:

- 100 ms @ 2 MHz = 200,000 retained T-states;
- fractional-cycle carry;
- Fast overshoot repayment;
- stopped-time discard;
- 10x scaling;
- speed-epoch change;
- Unlimited independence.

Final PASS requires the focused scheduler tests and full local suite to be green.

## 4. 88-2SIO / MC6850 — OPEN

Current model implements the BASIC-required subset and IRQ routing but not full ACIA conformance. Required closeout from Motorola/MITS documentation:

- control-word clock divide and word format;
- transmitter and receiver register/shift-register semantics;
- RDRF / TDRE timing ownership in the board rather than endpoint UI;
- overrun, framing and parity status;
- IRQ flag/enable behavior and master reset;
- deterministic conformance tests for both ports.

## 5. MITS 88-SIO — OPEN

The 88-SIO must be audited by hardware revision rather than treated as a generic UART. Required closeout:

- establish the exact supported MITS revision(s) from primary documentation;
- verify status-bit polarity and control semantics;
- model finite receive/transmit hardware and overrun/error behavior where applicable;
- move serial timing authority from ASR/Terminal/TCP/COM endpoints into the emulated card;
- document any deliberate compatibility mode separately from historical behavior.

## Validation policy

Do not run GitHub Actions for this branch. Each checkpoint should be validated locally with focused tests first and then the normal project validation (`cargo test`; release launch/build as appropriate). Long ignored CPU diagnostics are re-run only when a change can plausibly affect CPU semantics/timing or before a release certification pass.

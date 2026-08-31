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
7. A hardware item is not `PASS` until its dedicated Markdown documentation satisfies `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`, including primary references, physical-to-code mapping, supporting snippets, Fast/Cycle claims, regressions and known gaps.

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

Dedicated implementation/evidence document: **`docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`**.

### Implemented and locally validated

- S-100 input wait generator: selected 88-2SIO `IN` produces exactly one 500 ns / one-Tw delay at 2 MHz; `OUT`, 88-SIO and unmapped I/O do not inherit it.
- Cycle exposes the real `T1 -> T2 -> Tw -> T3` sequence with READY/WAIT/PRDY behavior; Fast accounts the same +1 total T-state without claiming pin-exact sub-instruction timing.
- Two finite MC6850 channels replace the old generic unbounded serial queues for 88-2SIO.
- Control word clock divide, word format, master reset, RX/TX interrupt enables and transmitter control are modeled.
- TDR and transmit shift register are distinct; TDRE follows TDR availability instead of endpoint presentation completion.
- Timed receive shift path and finite RDR are distinct; RDRF appears only after the configured frame completes.
- Motorola-style delayed OVRN, FE and PE status behavior is modeled.
- CTS, DCD, physical RTS level, BREAK and IRQ status are modeled and exposed through the backend-neutral hardware contract.
- DCD status/IRQ clearing follows status-read then data-read sequencing.
- Card baud timing belongs to the installed board, not ASR/Terminal pacing; integer phase accumulation retains fractional effective rates.
- The 88-2SIO oscillator continues during CPU STOP, sustained RESET and HOLD/HLDA while avoiding double advancement during RUN.
- Host-visible RX state distinguishes pending receiver contents from physical receive-line occupancy, enabling real overrun instead of mandatory RDR-empty flow control.
- Authentic paper-tape loader regression consumes bytes through timed hardware and a genuine guest `IN 11h` in both Rust engines.

The full local `cargo test` suite was green on 2026-08-31 through these changes after the debugger architecture guard was made semantic rather than dependent on a local variable name.

### Implemented after the last local green — validation required

- Text Terminal, raw TCP and external COM RX delivery now wait only for the physical receive shift path to become free; an unread MC6850 RDR no longer acts as hidden flow control.
- ASR-33 paper-tape reader uses the same physical RX-line contract and no longer depends on the 8080 RUN latch.
- `ReaderControlMode` models two real installations: local/manual reader control and optional MITS 88-TYA Reader Control via 88-2SIO RTS.
- In 88-TYA mode, the local Read/Pause buttons have no electrical authority; physical MC6850 RTS HIGH runs ReaderRun+, RTS LOW stops it.
- Historical values are exposed directly: `11h` / octal `021` leaves RTS LOW; `51h` / octal `121` raises RTS HIGH.
- UI states distinguish missing RTS source, RTS LOW and a character currently occupying the RX shift path.
- `tests/asr33_reader_control_architecture.rs` guards against reintroducing CPU-RUN or RDR-empty pacing.

### Remaining blockers before `PASS`

- locally validate the new Reader Control / endpoint physical-line block and full suite;
- propagate BREAK appropriately to attached endpoint models;
- expose CTS/DCD behavior/configuration for endpoints that can provide those signals;
- expose per-port baud-generator straps instead of retaining only the current default 110/9600 installation;
- expose the physical A2-A7 base-address strap block instead of permanently fixing `10h`-`13h`;
- re-run complete serial/loader/full-suite validation after final strap/signal closeout.

`ReaderControlMode` persistence across application restarts is a configuration/UX follow-up, not an electrical-fidelity blocker; the hardware mode currently defaults to Manual on each process start.

## 5. MITS 88-SIO — OPEN

The 88-SIO must be audited by hardware revision rather than treated as a generic UART. Required closeout:

- establish the exact supported MITS revision(s) from primary documentation;
- verify status-bit polarity and control semantics;
- model finite receive/transmit hardware and overrun/error behavior where applicable;
- move serial timing authority from ASR/Terminal/TCP/COM endpoints into the emulated card;
- document any deliberate compatibility mode separately from historical behavior;
- create its dedicated hardware-fidelity Markdown document before the item can become `PASS`.

## Validation policy

Do not run GitHub Actions for this branch. Each checkpoint should be validated locally with focused tests first and then the normal project validation (`cargo test`; release launch/build as appropriate). Long ignored CPU diagnostics are re-run only when a change can plausibly affect CPU semantics/timing or before a release certification pass.

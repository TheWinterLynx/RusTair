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
7. A hardware item is not `PASS` until its dedicated Markdown documentation satisfies `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`, including primary references, physical-to-code mapping, supporting snippets, Fast/Cycle claims, regressions, known gaps and a user-observable validation procedure wherever practical.

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

Dedicated implementation/evidence documents:

- **`docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`** — card/ACIA core, timing and Reader Control;
- **`docs/88_2SIO_EXTERNAL_COM_SIGNALS.md`** — physical CTS/DCD/BREAK bridge;
- **`docs/88_2SIO_PHYSICAL_STRAPS.md`** — A2-A7 address and per-ACIA baud straps.

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
- Authentic paper-tape loader regression consumes bytes through timed hardware and a genuine guest `IN` in both Rust engines.
- Text Terminal, raw TCP, external COM and ASR reader RX delivery gate only on the physical receive shift path; unread RDR is not hidden flow control.
- ASR-33 paper-tape reader no longer depends on the 8080 RUN latch.
- `ReaderControlMode` models local/manual reader control and optional MITS 88-TYA Reader Control via the real 88-2SIO RTS pin.
- In 88-TYA mode, `11h` / octal `021` leaves RTS LOW/ReaderRun off and `51h` / octal `121` raises RTS HIGH/ReaderRun on.
- External COM can use the MITS no-modem wiring (CTS/DCD grounded) or follow real host CTS/CD pins with the required active-LOW MC6850 polarity conversion.
- MC6850 BREAK is projected to a physical COM endpoint using the host serial BREAK control rather than a fabricated data byte.
- moving/disconnecting the External COM cable cannot leave stale CTS/DCD HIGH levels on the old virtual channel.

The Reader Control / physical RX pacing block passed the full local `cargo test` suite on 2026-08-31. The subsequent CTS/DCD/BREAK External COM focused tests and full normal suite were also reported green by the user on 2026-08-31. No GitHub Actions were run.

### Implemented after the latest local green — validation required

- `TwoSioAddressBlock` models the A2-A7 jumper selection as one four-port-aligned block and rejects the `FCh-FFh` front-panel-conflicting block.
- `TwoSioBaudTap` exposes only the eight physical MITS board taps: 110/150/300/1200/1800/2400/4800/9600.
- `TwoSioStraps` gives Port 0 and Port 1 independent physical baud selections while preserving the existing default installation (base `10h`, Port 0 110, Port 1 9600).
- the production `IoDevices` decoder, PRDY wait selection, debugger data-port mapping, trace data addresses and S-100 open-bus ownership now derive from the selected A2-A7 block instead of permanently decoding `10h-13h`.
- a regression moves the board to the MITS manual example base 68 decimal / `44h` and requires `44h-47h` to decode while `10h-13h` become open bus with no 88-2SIO wait.
- a second regression gives the two ACIAs different baud straps and requires their timed receive completions to diverge accordingly.
- ASR-33 keyboard BREAK now reaches the selected 88-2SIO receive pin as a held SPACE condition through the same backend-neutral line API used by Fast and Cycle; it is not encoded as NUL. A complete held BREAK frame produces zero data plus MC6850 FE, continued BREAK may expose delayed OVRN, and releasing a short incomplete BREAK does not fabricate a character.
- moving/disconnecting/displacing the ASR-33 cable explicitly returns the old UART RX line to MARK before the router changes ownership.

### Remaining blockers before `PASS`

- locally compile/validate the physical-strap core and full suite;
- carry `TwoSioStraps` through both backend implementations and `BackendHost`;
- expose address and per-port baud jumpers in Configuration with POWER-OFF-only changes;
- persist the selected physical straps across application restarts;
- replace remaining UI/endpoint labels that assume `10h-13h` with the selected address block;
- add shared public Fast/Cycle readdressing regressions;
- add/validate user-observable strap procedures from `docs/88_2SIO_PHYSICAL_STRAPS.md`;
- add the pending regression that debugger-only 88-2SIO `IN` activity cannot leave a stale Fast +1T wait for the next guest instruction;
- close the broader interrupt-routing/jumper question (direct interrupt vs no interrupt / 88-VI integration) at the machine-card boundary;
- re-run complete serial/loader/full-suite validation after final strap/signal closeout, including `tests/serial_receive_break_fidelity.rs`.

## 5. MITS 88-SIO — IMPLEMENTED / REVALIDATION PENDING

Dedicated evidence:

- `docs/88_SIO_HARDWARE_FIDELITY.md` — COM2502/card core, revision semantics, timing and endpoint contract;
- `docs/88_SIO_INTERRUPT_ROUTING.md` — Rev0/Rev1 interrupt sources, D0/D1 enables and PINT/raw-VI routing;
- `docs/88_SIO_ABC_ELECTRICAL_INTERFACES.md` — A/RS-232, B/TTL and C/current-loop connector conversion.

Implemented digital claim:

- finite COM2502 receive/transmit state and error behavior;
- board-owned baud/format timing and continued card operation while CPU execution is parked;
- Rev0 external RIN/ROT ready flip-flops kept distinct from RDA/TBMT;
- DATA IN/OUT handshake side effects;
- Rev1 internal-ready polling semantics;
- configurable even/odd I/O address pair, baud, word format and physical interrupt routing;
- raw VI lines kept distinct from direct PINT;
- real asynchronous RSI/TSO frame-bit projection for accepted baud-matched frames;
- A/B/C typed electrical conversion without importing 88-2SIO modem semantics;
- Fast/Cycle parity at the physical six-signal/electrical boundary;
- endpoint cable truth: direct ASR-33 only on C, direct External COM only on A, virtual Text Terminal/TCP peers explicitly adapt to the selected family, and no endpoint fabricates RIN/ROT;
- ASR-33 BREAK is a held physical SPACE condition, not `00h`: a complete BREAK frame produces zero data plus COM2502 framing error, while releasing an incomplete BREAK does not synthesize a character;
- leaving LINE or moving/disconnecting/displacing the ASR cable restores MARK on its previous receive line before routing changes;
- stale fixed `00h/01h` cable labeling removed.

The focused endpoint test, physical-boundary test and complete normal `cargo test` suite were reported green by the user on **2026-09-01** before the receive-BREAK correction. Because that correction changes the UART receive path, the PASS label is deliberately suspended until the new `tests/serial_receive_break_fidelity.rs` regressions and the normal full local suite are green. No GitHub Actions are required or have been run.

Known non-claims remain unchanged: analog voltage/current tolerances, noise/slew/cable effects, independently clocked remote receive sampling that automatically creates baud-mismatch framing faults, and a complete 88-VI controller beyond the raw VI wires.

## Validation policy

Do not run GitHub Actions for this branch. Each checkpoint should be validated locally with focused tests first and then the normal project validation (`cargo test`; release launch/build as appropriate). Long ignored CPU diagnostics are re-run only when a change can plausibly affect CPU semantics/timing or before a release certification pass. User-observable validation procedures complement deterministic tests and are required in hardware documentation wherever the behavior can be meaningfully exercised from the normal application.

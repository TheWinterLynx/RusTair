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

## 1. S-100 open bus — CLOSED IN CODE, LOCAL VALIDATION REQUIRED

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

### Regression coverage

- `src/machine/memory.rs`: physical absence vs guest-visible `FFh`, ignored writes, no wait-state contribution.
- `src/machine/io_devices.rs`: inactive serial-board address ranges and arbitrary unmapped ports resolve to `FFh`.
- `tests/open_bus_fidelity.rs`: public Fast/S-100 guest paths for memory, opcode, stack and I/O.

### Remaining edge before final PASS

Power-on/front-panel helper paths that inspect RAM through `peek()` must also resolve `None` through the same bus policy rather than using a local zero fallback. This will be closed before the open-bus item is marked final PASS.

## 2. Front-panel LED electrical duty and optical persistence — OPEN

Required closeout:

- remove first-window sampling bias under accelerated execution;
- expose deterministic raw electrical duty separately from optical persistence;
- prove sample-order invariance and long-window accounting;
- compare fixed-program raw duty between Fast reconstruction and Cycle Accurate electrical samples;
- retain wall-clock optical persistence as presentation only.

## 3. Authentic 2 MHz long-term clock — OPEN

Current runtime caps delayed-frame elapsed time and therefore can permanently discard emulated time after a host stall. Required closeout:

- retain elapsed-time debt rather than discarding it;
- bound per-UI-update execution for responsiveness while carrying remaining debt forward;
- preserve fractional T-state accounting;
- keep Unlimited explicitly outside wall-clock throttling;
- add deterministic scheduler tests, including a 100 ms host stall at 2 MHz producing 200,000 T-states of total debt.

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
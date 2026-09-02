# RusTair base hardware fidelity closeout

Status: **CLOSED — all base-hardware items in this ledger are PASS.**

Closeout branch: `agent/base-hardware-fidelity-closeout`.

Final local validation: **2026-09-02**, focused hardware checkpoints plus complete normal `cargo test` suite green.

No GitHub Actions were required for this closeout.

> Scope clarification (2026-09-02): item 3 below certifies the **long-term installed CPU-board clock rate/scheduler authority**. It was never a certification that every Intel 8080 package pin transitions on its exact PHI1/PHI2 edge. That separate physical CPU claim is now tracked explicitly in `docs/CPU_8080_HARDWARE_FIDELITY.md` and must reach PASS before RusTair calls Cycle "1:1 pin accurate" at the digital package boundary.

## Rules used for PASS

1. Prefer original MITS/Intel/Motorola documentation over emulator precedent.
2. Host/debugger inspection distinguishes absent hardware from guest-visible bus values.
3. Fast may reconstruct instruction-level effects, but does not claim unavailable exact sub-instruction pins.
4. Cycle Accurate must expose real electrical/timing state where the exact core owns it.
5. Presentation state never feeds back into machine electrical truth.
6. Compatibility workarounds remain explicit and optional.
7. Dedicated hardware documentation records references, physical-to-code mapping, regressions, Fast/Cycle claims, non-claims and user-observable validation.

## 1. S-100 open bus — PASS

The machine distinguishes physical absence from guest-visible bus value:

- debugger/physical peek of uninstalled memory returns absence;
- guest reads from unresponded memory or I/O observe `FFh`;
- writes to uninstalled memory do not create storage;
- unselected memory/cards do not contribute wait states;
- Cycle exposes `FFh` on S-100 DI before CPU sampling.

Key regressions: `tests/open_bus_fidelity.rs`, debugger execution coverage, memory and I/O unit tests.

## 2. Front-panel LED electrical duty — PASS

Electrical activity and optical persistence are separate layers. Raw lamp duty accounts the complete sample window without the former 32,000-T-state truncation, is order-invariant, and is shared consistently across Fast/Cycle where both engines possess equivalent information.

Optical brightness/aura remain presentation concerns, not electrical state.

## 3. Authentic long-term CPU clock — PASS

The authentic scheduler retains elapsed-time debt and fractional T-states instead of clipping host stalls. It uses the installed CPU-board clock as authority, repays Fast whole-instruction overshoot, discards stopped time, fences rate epochs correctly and avoids catch-up bursts from blocked execution.

Focused clock-authority and full-suite validation were green before serial closeout and remained protected by the final suite.

This PASS concerns **clock-rate accounting over time**. Exact Intel 8080 PHI1/PHI2 package-edge behavior is intentionally outside this item and is governed by the dedicated CPU physical-fidelity closeout.

## 4. MITS 88-2SIO / MC6850 — PASS

Canonical parent: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Focused evidence:

- `docs/88_2SIO_PHYSICAL_STRAPS.md`
- `docs/88_2SIO_INTERRUPT_ROUTING.md`
- `docs/88_2SIO_EXTERNAL_COM_SIGNALS.md`
- `docs/88_2SIO_BREAK_FIDELITY.md`
- `docs/88_2SIO_SIGNAL_INTERFACES.md`

Closed digital claim includes:

- two finite MC6850 channels with TDR/TSR and receiver/RDR separation;
- documented control/status/error/IRQ behavior including delayed OVRN;
- exact selected-input one-Tw PRDY delay in Cycle and equivalent +1T accounting in Fast;
- independent card oscillator while CPU execution is parked;
- configurable A2-A7 four-address block and independent per-port baud taps;
- selected block controls decode, open bus, wait, debugger mapping, trace addresses and authentic bootstrap operands together;
- debugger-only 88-2SIO reads cannot leak Fast wait debt into guest execution;
- independent DI/EI wiring to disconnected, PINT or raw VI0..VI7;
- CTS/DCD/RTS/BREAK modem-line semantics and External COM active-LOW conversion/cleanup;
- native host COM transmit BREAK rather than a fake byte;
- ASR-33 receive BREAK as held SPACE with FE/overrun behavior and no fabricated short-BREAK NUL;
- independent per-port RS-232 / TTL / TTY 20 mA hardwiring;
- direct ASR requires TTY 20 mA, direct External COM requires RS-232, while Text Terminal/TCP are explicit virtual selected-family peers;
- POWER-OFF-only physical rewiring, persistence, dynamic labels and automatic incompatible-cable disconnect;
- Fast and Cycle preserve the same installed card configuration.

Final signal-interface and BREAK regressions plus the complete normal `cargo test` suite were reported green locally on **2026-09-02**.

Explicit non-claims: exact analog voltage/current magnitude, line impedance/noise, full Teletype selector mechanics and an installed 88-VI controller beyond raw VI request lines.

## 5. MITS 88-SIO / COM2502 — PASS

Canonical evidence:

- `docs/88_SIO_HARDWARE_FIDELITY.md`
- `docs/88_SIO_INTERRUPT_ROUTING.md`
- `docs/88_SIO_ABC_ELECTRICAL_INTERFACES.md`

Closed digital claim includes:

- finite COM2502 RX/TX state and errors;
- board-owned baud/format timing while CPU execution is parked;
- Rev0 external RIN/ROT ready flip-flops kept separate from RDA/TBMT;
- Rev1 internal-ready polling semantics;
- DATA IN/OUT handshake side effects;
- configurable address pair, baud, format and physical interrupt routing;
- PINT and raw VI routing kept distinct;
- asynchronous RSI/TSO serial line projection;
- A/RS-232, B/TTL and C/current-loop electrical families without importing MC6850 semantics;
- direct ASR only on C/current-loop and direct External COM only on A/RS-232, with virtual Terminal/TCP selected-family peers;
- ASR-33 BREAK as physical SPACE rather than `00h`, including complete-frame FE and short-BREAK abort;
- cable moves/disconnects restore MARK before routing ownership changes;
- no stale fixed `00h/01h` endpoint labels.

The receive-BREAK focused regressions and complete normal suite were reported green locally on **2026-09-02**, restoring the dedicated 88-SIO PASS after that receive-path change.

## Final validation record

The last closeout sequence included the focused 88-SIO/88-2SIO configuration, endpoint, signal-interface and BREAK regressions followed by the complete normal `cargo test` suite. The user reported the suite green on **2026-09-02**.

Long ignored CPU diagnostics were intentionally not repeated for the final serial-only changes because CPU semantics/timing were not modified. They remain appropriate for a dedicated release/CPU certification pass.

## Result

The base hardware closeout represented by this document has no remaining blocker **within its stated scope**. It must not be cited as proof of PHI-edge/package-pin accuracy. Future work should be opened as a new scoped hardware/peripheral item rather than leaving completed entries in an indefinite `IN PROGRESS` state.

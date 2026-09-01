# MITS 88-2SIO / Motorola MC6850 hardware fidelity

Status: **PASS — digital card, timing, wiring, endpoint and Fast/Cycle claims locally validated.**

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

Related focused evidence:

- `docs/88_2SIO_PHYSICAL_STRAPS.md`
- `docs/88_2SIO_INTERRUPT_ROUTING.md`
- `docs/88_2SIO_EXTERNAL_COM_SIGNALS.md`
- `docs/88_2SIO_BREAK_FIDELITY.md`
- `docs/88_2SIO_SIGNAL_INTERFACES.md`

## 1. Fidelity claim

RusTair models the digital/electrical behavior of the MITS 88-2SIO at the S-100, MC6850 and serial-connector boundaries without treating host terminal convenience as hardware truth.

The PASS claim covers:

- two finite Motorola MC6850 ACIAs;
- control/status/data semantics, master reset, word formats and /1, /16, /64 clock selection;
- separate TDR/TSR and receive-shift/RDR stages;
- TDRE, RDRF, FE, PE, delayed OVRN and IRQ behavior;
- literal CTS, DCD, RTS and BREAK pin/control state;
- one documented 500 ns / one-Tw S-100 wait on selected `IN`, with no corresponding `OUT` wait;
- independent per-port board baud taps;
- configurable A2-A7 four-address decode block;
- independent per-port RS-232 / TTL / TTY 20 mA external signal hardwiring;
- DI/EI interrupt wiring to disconnected, PINT or VI0..VI7 raw vector lines;
- independent serial-card clocking while the 8080 is STOPped, RESET-held or in HOLD/HLDA;
- backend-neutral physical serial line state shared by Fast and Cycle;
- External COM CTS/DCD and native transmit BREAK bridging;
- ASR-33 receive BREAK as held SPACE rather than ASCII NUL;
- optional MITS 88-TYA reader control from the physical RTS level;
- endpoint routing that refuses incompatible direct electrical connections instead of inserting hidden converters.

Analog voltage magnitude, threshold tolerance, line impedance, current-loop compliance, propagation delay, noise and mechanical Teletype selector behavior are explicit non-claims.

## 2. Primary evidence

Primary hardware references:

- MITS, *Altair 88-2-SIO Documentation*, reprinted March 1977: `https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf`
- Motorola Semiconductor Products Inc., *MC6800 Microcomputer System Design Data*, 1976, MC6850 section: `https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf`

The MITS documentation supplies the board-level address decode, baud taps, input-wait generator, DI/EI interrupt options and per-port serial interface wiring. Motorola supplies the ACIA register, status, modem-pin, error, interrupt and BREAK semantics.

## 3. Address and wait timing

A2-A7 select one aligned block of four I/O ports. A0/A1 then select Port 0/Port 1 and control/status versus data. `TwoSioAddressBlock` therefore accepts aligned bases through `F8h`; `FCh-FFh` is rejected because FFh belongs to the Altair front-panel sense-switch input.

The common RusTair installation remains `10h-13h`, but it is not hard-coded hardware truth. Readdressing the card moves together:

- IN/OUT decode;
- open-bus ownership;
- debugger data-port mapping;
- I/O trace addresses;
- PRDY wait selection;
- authentic bootstrap immediate port operands;
- endpoint labels.

The MITS input wait generator adds one 500 ns wait at the stock 2 MHz CPU clock. Cycle exposes the real `T1 -> T2 -> Tw -> T3` path with READY/WAIT behavior. Fast adds the same +1 total T-state without claiming pin-exact sub-instruction timing. Debugger-only `IN` inspection does not leave stale Fast wait debt for the next guest instruction.

## 4. MC6850 ownership

`src/mc6850.rs` owns ACIA register and status truth. `src/machine/two_sio.rs` adds the physical serial clock/shift path for one channel. `src/machine/io_devices.rs` owns the two installed channels, board decode, PRDY contribution and interrupt routing.

Important invariants:

- TDR emptiness, not host endpoint consumption, determines TDRE;
- a received frame must complete before RDRF rises;
- unread RDR does not freeze the physical receive line, so a later frame may generate real overrun;
- Motorola delayed OVRN visibility is preserved;
- CTS HIGH inhibits transmitter-ready behavior according to the ACIA model;
- DCD status/IRQ clearing follows the status-read then data-read sequence;
- BREAK is an electrical SPACE condition, not a byte value.

## 5. Board clock and baud straps

Each ACIA has an independent physical tap selected from the documented set:

`110, 150, 300, 1200, 1800, 2400, 4800, 9600`

The selected tap feeds the MC6850 clock input; CR1:CR0 still selects /1, /16 or /64. Endpoint pacing never overwrites the card clock.

`TwoSioPort` uses integer phase accumulation, avoiding floating-point drift. The card clock also advances while CPU execution is parked in STOP, sustained RESET or HOLD/HLDA. During RUN, CPU T-states remain the authority so wall-clock chassis service does not double-count serial time.

## 6. Physical signal interfaces and endpoints

Each port is independently hardwired as one of:

- RS-232;
- TTL;
- TTY 20 mA current loop.

The direct endpoint contract is explicit:

| Endpoint | Direct physical requirement |
| --- | --- |
| Built-in ASR-33 | TTY 20 mA current loop |
| External COM | RS-232 |
| Text Terminal | explicit virtual selected-family peer |
| External TCP | explicit virtual selected-family peer |

ASR-33 or External COM cannot be connected to an incompatible direct port. Rewiring a port is POWER-OFF-only, disconnects an incompatible existing direct cable, and never installs an invisible level converter. Persisted cable assignments pass through the same compatibility check.

If an ASR-33 cable is moved or made incompatible while BREAK is active, RusTair first returns the old receive line to MARK and only then changes routing.

## 7. BREAK behavior

### MC6850 transmit BREAK

CR6:CR5=`11` drives continuous spacing on Tx Data. TDR/TSR progression continues, but any character frame overlapped by BREAK is not delivered as a valid completed byte. External COM uses the host serial API's native BREAK control. Text Terminal/TCP do not receive fabricated NUL bytes.

### ASR-33 receive BREAK

The Model 33 BREAK key is a physical receive-line condition. While held in LINE mode it drives the selected UART receive line to SPACE. A complete BREAK frame reaches the MC6850 as zero data with framing error; continued BREAK can naturally create overrun. Releasing an incomplete synthetic BREAK frame does not fabricate a NUL character.

Fast and Cycle share the same backend-neutral `serial_set_receive_break` path.

## 8. CTS/DCD, RTS and Reader Control

External COM may use the MITS no-modem wiring with CTS/DCD grounded, or follow real host CTS/CD pins. Host assertion semantics are explicitly converted to the active-LOW MC6850 pin levels. Moving/disconnecting COM restores the old channel to the grounded input state.

The MC6850 physical RTS level is exposed independently from BREAK. Optional MITS 88-TYA reader control uses that literal level:

- `11h` / octal `021`: RTS LOW, ReaderRun off;
- `51h` / octal `121`: RTS HIGH, ReaderRun on when 88-TYA wiring is selected.

The ASR reader mechanism is not tied to the 8080 RUN latch.

## 9. Interrupt routing

The two ACIA IRQ outputs are separate from where their board wires go. `TwoSioInterruptWiring` independently maps Port 0 / DI and Port 1 / EI to:

- disconnected;
- PINT;
- VI0..VI7.

An ACIA may therefore have IRQ/status bit 7 active while processor PINT remains inactive. VI0..VI7 are exposed as raw chassis request lines only; the 88-2SIO never fabricates a CPU restart opcode for a future 88-VI board.

## 10. Fast versus Cycle

Both engines consume the same installed `TwoSioStraps`, interrupt wiring and physical line state.

Cycle claims exact CPU/bus T-state placement where the exact core has that information, including the 88-2SIO input Tw. Fast claims equivalent architectural/card results and total timing, but does not pretend to expose exact intra-instruction pins.

Switching engines while POWER is OFF reapplies the installed serial board, straps and interrupt wiring, preserving the same physical machine configuration.

## 11. Persistence and UI

Current persisted 88-2SIO hardware keys include:

```text
machine.two_sio_base
machine.two_sio_port0_baud
machine.two_sio_port1_baud
machine.two_sio_port0_interface
machine.two_sio_port1_interface
machine.two_sio_port0_irq
machine.two_sio_port1_irq
```

Configuration exposes physical address, baud, signal-interface and DI/EI controls as POWER-OFF-only operations. Endpoint labels use the selected address/interface rather than fixed `10h-13h` assumptions.

## 12. Physical-to-code map

- `src/mc6850.rs` — ACIA register/status/error semantics.
- `src/config/two_sio.rs` — address block, baud taps, per-port signal family and interrupt wiring types.
- `src/machine/two_sio.rs` — one timed MC6850 serial channel.
- `src/machine/io_devices.rs` — two-channel board decode, waits, line routing and interrupt projection.
- `src/backend/mod.rs`, `native.rs`, `cycle_host.rs` — shared backend contract.
- `src/io/serial_router.rs` — endpoint electrical compatibility.
- `src/app/serial_hardware.rs` — physical cable/backend bridge.
- `src/app/mod.rs`, `runtime.rs`, `persistence.rs` — reconfiguration, UI and persistence.
- `src/app/external_com.rs`, `src/io/com_serial.rs` — host COM signal bridge.
- `src/app/asr33_controller.rs`, `src/peripherals/asr33/keyboard.rs` — Model 33 BREAK control.

## 13. Regression evidence

The normal suite includes focused coverage for:

- `tests/two_sio_prdy_timing.rs`
- `tests/two_sio_idle_chassis_clock.rs`
- `tests/two_sio_modem_pins.rs`
- `tests/two_sio_external_com_signals.rs`
- `tests/two_sio_break_fidelity.rs`
- `tests/serial_receive_break_fidelity.rs`
- `tests/two_sio_debugger_wait_isolation.rs`
- `tests/two_sio_interrupt_ui.rs`
- `tests/two_sio_strap_ui.rs`
- `tests/two_sio_signal_interfaces.rs`
- authentic serial/loader regressions and unit tests in `mc6850`, `two_sio`, `io_devices`, configuration and persistence.

## 14. Validation history

Focused 88-2SIO hardware checkpoints were repeatedly reported green locally through 2026-08-31 and 2026-09-01. After adding ASR receive BREAK and the final per-port electrical-interface model, the focused regressions and the complete normal `cargo test` suite were reported green on **2026-09-02**.

No GitHub Actions were required for this closeout.

## 15. Known limits / non-claims

PASS is for the documented digital hardware boundary. It does not claim:

- exact analog RS-232 voltage magnitude/loading;
- exact TTL thresholds or propagation delay;
- exact current-loop source/sink current or compliance;
- cable capacitance, contact bounce, noise or marginal-signal corruption;
- full ASR-33 selector-magnet running-open mechanics;
- an installed 88-VI controller beyond the raw VI request lines.

These limits are explicit and are not replaced by compatibility hacks.

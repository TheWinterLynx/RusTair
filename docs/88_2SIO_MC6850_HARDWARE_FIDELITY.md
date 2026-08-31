# MITS 88-2SIO / Motorola MC6850 hardware fidelity

Status: **IN PROGRESS — core ACIA, S-100 wait timing, independent card clock, modem pins, physical RX pacing and optional 88-TYA Reader Control are implemented; physical strap configuration and remaining endpoint signal propagation are still open.**

This document describes the physical MITS 88-2SIO, the Motorola MC6850 behavior required to emulate it, how RusTair maps that hardware into Rust state, what Fast and Cycle Accurate can truthfully claim, and which pieces still prevent a final `PASS`.

The project-wide documentation rules are in `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope and fidelity claim

The target is the digital/electrical behavior visible to the Altair CPU and to attached serial equipment:

- S-100 address decoding and I/O response;
- board-generated PRDY/WAIT behavior;
- both MC6850 channels;
- control/status/data registers;
- TDR/TSR and receiver/RDR separation;
- RDRF, TDRE, FE, OVRN, PE and IRQ semantics;
- CTS, DCD, RTS and BREAK control/status behavior;
- independent serial clocks and baud-generator taps;
- continued card operation while the 8080 is STOPped, RESET-held or in HOLD/HLDA;
- guest-visible interrupt projection onto the canonical Altair PINT path;
- physical endpoint boundary: bytes only reach the host after a complete emulated serial frame;
- host-to-ACIA receive pacing based on the physical receive shift path rather than RDR emptiness;
- optional MITS 88-TYA Reader Control where physical MC6850 RTS HIGH drives ReaderRun+.

Out of scope for the digital fidelity claim are analog voltage rise/fall times, exact RS-232 driver slew, cable capacitance, relay bounce and acoustic/mechanical presentation. Those may be modeled later for presentation, but they must not change register/bus truth.

## 2. Primary physical hardware

The 88-2SIO is a dual serial I/O board built around **two Motorola MC6850 ACIAs**. Each ACIA exposes two CPU-visible registers and its own serial/modem-control pins. MITS adds S-100 decoding, board clock generation/baud selection and an input wait generator around those chips.

### 2.1 Standard address map

For the common MITS base address `10h` (octal `020`):

| Address | A1 | A0 | Function |
| ---: | ---: | ---: | --- |
| `10h` | 0 | 0 | Port 0 status read / control write |
| `11h` | 0 | 1 | Port 0 data read/write |
| `12h` | 1 | 0 | Port 1 status read / control write |
| `13h` | 1 | 1 | Port 1 data read/write |

The physical board uses address straps/jumpers for A2–A7, so `10h`–`13h` is a configuration, not an intrinsic fixed address. RusTair currently models that standard block as fixed; configurable address straps remain an open item.

### 2.2 S-100 input wait generator

The MITS board documentation describes a card-level wait generator used on **input only**. SINP selects/clocks the board logic, PRDY is forced low, the 8080 enters WAIT for approximately **500 ns**, and PWAIT releases the board ready contribution.

At the stock 2 MHz Altair CPU clock:

`1 T-state = 500 ns`

Therefore a selected 88-2SIO `IN` adds exactly **one Tw**. `OUT` does not receive this board wait.

Guest timing consequence for an 8080 `IN port` instruction:

- ordinary/unmapped input path: 10 T-states;
- selected 88-2SIO input: **11 T-states**.

Cycle Accurate must expose the real machine-cycle progression:

`T1 -> T2 (READY low) -> Tw (WAIT high) -> T3`

Fast can reproduce the total +1T but cannot claim exact external pin state inside the instruction.

## 3. MC6850 register model

### 3.1 Control register

RusTair models the Motorola control layout:

| Bits | Function |
| --- | --- |
| CR1:CR0 | clock divide `/1`, `/16`, `/64`, or master reset (`11`) |
| CR4:CR2 | word length, parity and stop-bit selection |
| CR6:CR5 | RTS/TX interrupt/BREAK transmitter control |
| CR7 | receive interrupt enable |

Word formats represented by CR4:CR2:

| CR4:CR2 | Data | Parity | Stop |
| --- | ---: | --- | ---: |
| `000` | 7 | even | 2 |
| `001` | 7 | odd | 2 |
| `010` | 7 | even | 1 |
| `011` | 7 | odd | 1 |
| `100` | 8 | none | 2 |
| `101` | 8 | none | 1 |
| `110` | 8 | even | 1 |
| `111` | 8 | odd | 1 |

Transmitter-control interpretation used by RusTair follows the Motorola/MITS electrical levels:

| CR6:CR5 | RTS physical level | TX IRQ | BREAK |
| --- | --- | --- | --- |
| `00` | LOW | disabled | no |
| `01` | LOW | enabled | no |
| `10` | **HIGH** | disabled | no |
| `11` | LOW | disabled | **yes** |

The wording intentionally uses **physical HIGH/LOW**, not “asserted/deasserted”, because MITS' 88-TYA ReaderRun circuit uses the physical RTS level directly.

### 3.2 Status register

| Bit | Symbol | RusTair meaning |
| ---: | --- | --- |
| 0 | RDRF | Receive Data Register contains current data; forced empty by DCD high |
| 1 | TDRE | Transmit Data Register is empty and may accept new data; inhibited by CTS high |
| 2 | DCD | Data Carrier Detect input/status latch |
| 3 | CTS | Clear To Send input level |
| 4 | FE | framing error belonging to current RDR character |
| 5 | OVRN | receiver overrun, with Motorola's delayed visibility semantics |
| 6 | PE | parity error belonging to current RDR character |
| 7 | IRQ | enabled ACIA interrupt condition is active |

## 4. Physical-to-RusTair ownership map

| Physical element | RusTair owner | File |
| --- | --- | --- |
| MC6850 digital registers/error state | `Mc6850` | `src/mc6850.rs` |
| one physical ACIA channel + baud clock | `TwoSioPort` | `src/machine/two_sio.rs` |
| two installed ACIAs and board decode | `IoDevices::two_sio` | `src/machine/io_devices.rs` |
| S-100 PRDY contribution | `IoDevices::ready_for_input_t_state` | `src/machine/io_devices.rs` |
| canonical interrupt/PINT projection | `IoDevices::interrupt_request` + `AltairBus::refresh_interrupt_request_line` | `src/machine/io_devices.rs`, `src/machine/mod.rs` |
| Fast total wait accounting | `AltairBus::fast_account_io_input_wait` | `src/machine/io_devices.rs` |
| exact Cycle READY sampling | shared S-100 READY arbitration | `src/backend/cycle.rs` / machine bus |
| idle card clock while CPU parked | backend idle chassis clock service | `src/backend/native.rs`, `src/backend/cycle_host.rs` |
| backend-neutral modem pin API | `SerialModemLines` | `src/backend/mod.rs` |
| physical RX line/RDR distinction exposed to endpoints | `serial_rx_line_idle` + `serial_rx_empty` | backend contract + `src/app/serial_hardware.rs` |
| ASR-33 Reader Control wiring selection | `ReaderControlMode` | `src/app/asr33_state.rs` |
| RTS -> ReaderRun+ resolution | `RusTairApp::asr_reader_motor_running` | `src/app/serial_hardware.rs` |
| ASR reader transport | `update_paper_tape_reader` | `src/app/ui/asr33_window.rs` |

The important architectural rule is that **the terminal/ASR/TCP/COM endpoint does not own MC6850 status bits or baud timing**. Endpoints can present bytes to a physical line or consume bytes that have completed transmission, but RDRF, TDRE, IRQ and error bits remain card state.

## 5. Supporting code snippets

The snippets below are intentionally short invariants. The source files remain authoritative if implementation details evolve.

### 5.1 Word format belongs to the MC6850

`src/mc6850.rs`:

```rust
pub(crate) fn word_format(&self) -> WordFormat {
    match (self.control >> 2) & 7 {
        0 => WordFormat { data_bits: 7, parity: Parity::Even, stop_bits: 2 },
        1 => WordFormat { data_bits: 7, parity: Parity::Odd, stop_bits: 2 },
        2 => WordFormat { data_bits: 7, parity: Parity::Even, stop_bits: 1 },
        3 => WordFormat { data_bits: 7, parity: Parity::Odd, stop_bits: 1 },
        4 => WordFormat { data_bits: 8, parity: Parity::None, stop_bits: 2 },
        5 => WordFormat { data_bits: 8, parity: Parity::None, stop_bits: 1 },
        6 => WordFormat { data_bits: 8, parity: Parity::Even, stop_bits: 1 },
        _ => WordFormat { data_bits: 8, parity: Parity::Odd, stop_bits: 1 },
    }
}
```

### 5.2 TDRE follows TDR, not the host terminal

`src/mc6850.rs`:

```rust
fn tdre(&self) -> bool {
    !self.cts_high && self.tdr.is_none()
}

pub(crate) fn transfer_tdr_to_shift_if_idle(&mut self) -> bool {
    if self.cts_high || self.tx_shift.is_some() { return false; }
    let Some(value) = self.tdr.take() else { return false; };
    self.tx_shift = Some(value);
    true
}
```

Once TDR transfers into the transmit shift register, TDRE may rise even though the character is still physically shifting and has not yet reached ASR/Terminal/TCP/COM.

### 5.3 Receiver and RDR are distinct physical stages

`src/machine/two_sio.rs`:

```rust
rx_shift: Option<(u8, bool, bool)>,
rx_bits_remaining: u8,

pub(super) fn receive_line_idle(&self) -> bool {
    self.rx_shift.is_none()
}
```

and completion occurs only after the configured frame:

```rust
if self.rx_bits_remaining == 1 {
    self.rx_bits_remaining = 0;
    self.rx_shift = None;
    self.acia.receive_character(value, framing_error, parity_error);
}
```

Thus `RDRF=0` while a character is still traversing the receive shift path, and the receive line can later become free even while RDR remains full. This distinction is required to permit real overrun instead of host-side flow control.

### 5.4 Motorola-style delayed overrun visibility

`src/mc6850.rs`:

```rust
pub(crate) fn receive_character(&mut self, value: u8, framing_error: bool, parity_error: bool) {
    if self.rdr_full || self.overrun_visible {
        self.overrun_pending = true;
        return;
    }
    ...
    self.rdr_full = true;
}
```

The valid first character remains in RDR. The later character is lost. OVRN becomes visible after the valid earlier RDR character is read, matching Motorola's documented delayed status behavior.

### 5.5 Physical RTS level and BREAK are separate control outputs

`src/mc6850.rs`:

```rust
pub(crate) fn rts_high(&self) -> bool {
    self.control & 0x60 == 0x40
}

pub(crate) fn break_active(&self) -> bool {
    self.control & 0x60 == 0x60
}
```

This gives the historical 88-TYA values their literal electrical meaning:

- `021` octal = `11h`: RTS LOW, reader control off;
- `121` octal = `51h`: RTS HIGH, ReaderRun enabled when that cable option is installed.

The ASR endpoint now exposes this as an explicit physical wiring choice rather than forcing all tape readers to obey RTS.

### 5.6 Explicit manual versus 88-TYA reader wiring

`src/app/asr33_state.rs`:

```rust
pub(super) enum ReaderControlMode {
    Manual,
    Mits88TyaRts,
}

pub(super) const fn effective_running(
    self,
    manual_running: bool,
    rts_high: Option<bool>,
) -> bool {
    match self {
        Self::Manual => manual_running,
        Self::Mits88TyaRts => matches!(rts_high, Some(true)),
    }
}
```

The default remains `Manual`. This is intentional: an Altair with a normal/manual tape-reader workflow is a valid physical installation, and the authentic bootstrap writes `11h`/`021` octal, which leaves RTS LOW. Selecting `Mits88TyaRts` instead models the optional program-controlled 88-TYA wiring.

`src/app/serial_hardware.rs` resolves the actual motor command from the MC6850 pin:

```rust
fn asr_reader_motor_running(&mut self) -> bool {
    let rts_high = self.asr_serial_rts_high();
    self.asr33
        .reader_control
        .effective_running(self.asr33.reader_running, rts_high)
}
```

No UI shadow register is involved.

### 5.7 Reader transport is independent from CPU RUN and RDR occupancy

`src/app/ui/asr33_window.rs`:

```rust
if self.tty.mode != TtyMode::Line
    || !self.asr_connection().is_connected()
    || !self.machine.powered()
    || !self.asr_serial_rx_line_idle()
{
    return;
}
```

There is deliberately no `machine.running()` requirement. A physical reader motor and the 88-2SIO oscillator do not stop just because the 8080 is STOPped. There is also deliberately no `asr_serial_rx_empty()` requirement: an unread RDR byte must not pause the next physical serial frame.

### 5.8 Host endpoints gate on line occupancy, not RDRF

Terminal/TCP/COM input paths now call the shared physical-line query:

```rust
if !self.terminal_serial_rx_line_idle() { ... }
```

or:

```rust
if self.serial_rx_line_idle_at(connection) {
    self.serial_receive_at(connection, byte);
}
```

This permits a second physical frame to arrive while RDR still contains an unread prior character, which is necessary for the MC6850 to set OVRN naturally.

### 5.9 Board-level input wait

`src/machine/io_devices.rs`:

```rust
pub(super) fn input_wait_states(&self, port: u8) -> u8 {
    if self.serial_board == SerialBoard::TwoSio88
        && Self::two_sio_decodes_port(port) { 1 } else { 0 }
}

pub(super) fn ready_for_input_t_state(
    &self,
    port: u8,
    input_read: bool,
    phase: MemoryReadyPhase,
) -> bool {
    if !input_read || self.input_wait_states(port) == 0 { return true; }
    !matches!(phase, MemoryReadyPhase::T1 | MemoryReadyPhase::T2)
}
```

The card releases its wait in Tw; output and unmapped I/O remain unaffected.

### 5.10 Board clock is independent from endpoint presentation

`src/machine/two_sio.rs` accumulates exact integer phase:

```rust
let numerator_per_t_state = u64::from(self.baud_tap.baud()) * 16;
let threshold = u64::from(cpu_clock_hz) * u64::from(divider);
let total = self.bit_phase_numerator.saturating_add(
    t_states.saturating_mul(numerator_per_t_state)
);
let boundaries = total / threshold;
self.bit_phase_numerator = total % threshold;
```

No floating-point baud accumulator is required, and fractional effective rates remain deterministic.

## 6. Baud generator / straps

The MITS board provides eight labelled taps per ACIA:

- 110
- 150
- 300
- 1200
- 1800
- 2400
- 4800
- 9600 baud

RusTair retains the complete physical tap set in `TwoSioBaudTap`. The current default installation is:

- Port 0: 110 baud, intended for ASR-33;
- Port 1: 9600 baud, intended for text terminal.

The MITS tap is treated as a 16x source and the MC6850 CR1:CR0 divider applies `/1`, `/16` or `/64`. This allows the model to represent rates such as 27.5 baud without floating-point drift.

**Open blocker:** these taps are not yet exposed as physical configuration straps in the user configuration. Until that is complete the digital clock engine is implemented, but the installed-board configuration is not fully user-selectable like the physical card.

## 7. Serial timing examples

### 7.1 Historical bootstrap control `11h`

`11h` means:

- `/16` clock;
- 8 data bits;
- no parity;
- 2 stop bits;
- RTS LOW;
- TX interrupt disabled;
- RX interrupt disabled.

Frame length:

`1 start + 8 data + 2 stop = 11 bits`

At 110 baud:

`11 / 110 = 0.100 s = 100 ms`

At a 2 MHz chassis reference that corresponds to **200,000 CPU-clock quanta** of serial elapsed time. This is why the authentic loader regression must not expect an injected tape byte to appear in RDR immediately.

### 7.2 9600 baud 8N1

A 10-bit 8N1 frame takes approximately 1.0417 ms. More importantly, TDRE can return much earlier than frame completion: when the next transmitter bit-clock transfers TDR into TSR, TDR is empty although the character continues shifting.

## 8. Interrupt behavior

The MC6850 IRQ bit and the card's S-100 interrupt contribution are owned by ACIA state, not endpoint activity.

Implemented sources include:

- RDRF when RX interrupts are enabled;
- overrun when RX interrupts are enabled;
- DCD transition/status latch when RX interrupts are enabled;
- TDRE when TX-empty interrupts are enabled.

DCD clearing uses the documented status-read followed by data-read sequence. `set_serial_modem_inputs()` immediately refreshes the canonical interrupt line so an external pin transition does not wait for a later unrelated I/O call.

The current machine still uses the broader RusTair direct interrupt-vector policy (`RST 7` / `FFh`) until the planned vectored-interrupt hardware work replaces that system-level simplification. That limitation is outside the internal MC6850 status/IRQ-generation claim but remains relevant to whole-machine fidelity.

## 9. Fast versus Cycle Accurate

### Fast

Fast is instruction-oriented. It guarantees guest-visible register semantics and correct total elapsed T-state accounting, including the 88-2SIO's +1T input wait. It does **not** claim pin-exact timing inside an individual instruction.

For serial time, Fast advances the board using the actual T-state delta consumed by executed instructions. When the CPU is physically parked, the idle chassis clock service supplies the missing elapsed board time.

### Cycle Accurate

Cycle Accurate samples real CPU T-states and combines card READY/PRDY with other S-100 ready sources. A selected 88-2SIO input therefore produces a real Tw and WAIT state rather than a post-hoc cycle-count correction.

The serial board advances once per real Cycle T-state while the CPU is running. When STOP/RESET/HOLD prevents CPU T-states from advancing, the separate chassis-clock path keeps the 88-2SIO oscillator alive.

### Shared invariant

Neither backend may double-count serial elapsed time: CPU T-states are authority while they exist; wall/chassis elapsed time fills only intervals where the CPU core is electrically parked. Both expose the same MC6850 modem-pin and RX-line contracts to the ASR/terminal host layer.

## 10. STOP, RESET and HOLD/HLDA

The serial board has its own clock hardware and does not stop merely because the CPU stops executing instructions.

Both backends therefore track T-states already covered by CPU execution between panel/runtime commits. If the CPU is parked, elapsed chassis time is converted to board quanta and only the uncovered interval is applied.

The regression explicitly covers:

- STOP;
- sustained front-panel RESET;
- HOLD/HLDA;
- normal RUN without double advancement.

This matters for both RX and TX. A character already travelling through the ACIA must continue to completion while the operator stops the CPU. It also matters for 88-TYA Reader Control: once RTS is HIGH, the reader is not implicitly frozen by the CPU RUN/STOP latch.

## 11. Modem/control pins

RusTair exposes a backend-neutral physical structure:

```rust
pub struct SerialModemLines {
    pub rts_high: bool,
    pub break_active: bool,
    pub cts_high: bool,
    pub dcd_high: bool,
}
```

The 88-SIO returns no fabricated `MC6850` modem-pin structure; these signals exist only when the 88-2SIO is installed.

CTS high:

- status bit 3 becomes high;
- TDRE is inhibited;
- TDR cannot transfer into TSR.

DCD high:

- status bit 2 becomes high;
- RDRF is suppressed;
- with RX interrupt enabled, IRQ is generated/latches according to the MC6850 sequence.

BREAK is represented as a distinct MC6850 output state. Propagating that continuous spacing condition into every possible attached endpoint remains an open integration item.

## 12. 88-TYA / ASR-33 Reader Control relationship

The MITS 88-TYA Call/Control unit provides a program-controlled paper-tape reader input. The 88-TYA manual specifically describes control from the 88-2SIO RTS output.

The important historical distinction is between two valid installations:

1. **Manual reader control** — the operator starts/stops the reader using the ASR-33 controls. The computer need not raise RTS.
2. **88-TYA Reader Control via RTS** — physical RTS HIGH energizes ReaderRun+ and RTS LOW stops the reader.

The MITS manual identifies the corresponding 88-2SIO initialization values:

- octal `021` (`11h`) keeps reader control off while preserving 8 data bits, 2 stop bits and divide-16;
- octal `121` (`51h`) raises RTS and turns the reader on while preserving the remaining configuration bits.

RusTair models these as **physical wiring choices**, not compatibility modes. `Manual` is the default so the existing authentic loader workflow can continue to model an operator-started reader after the bootstrap writes `11h`. Selecting `Mits88TyaRts` disables the local Read/Pause authority and makes the actual MC6850 RTS pin authoritative.

The UI exposes the current electrical condition explicitly:

- `WAIT RTS SOURCE` — no installed/connected MC6850 RTS pin is available;
- `WAIT RTS LOW` — 88-TYA wiring is selected and the guest is holding RTS LOW;
- `RX FRAME` — a serial character is currently occupying the receive shift path;
- `READING` — the reader is commanded on and can present the next character.

A particularly important fidelity property is that **CPU RUN is not part of this state machine**. The reader may continue while the CPU is STOPped; if the CPU fails to consume RDR in time, the correct hardware consequence is MC6850 overrun.

Implementation status: **implemented in code; local validation of this new endpoint/UI block is still required before it is marked validated.**

## 13. RX host-boundary correction

A critical model distinction is explicit:

- `serial_rx_empty()` represents pending receiver content/state;
- `serial_rx_line_idle()` represents whether the external serial line / receiver shift path can start another character.

The old host pacing rule “only send another byte when RDR is empty” was too generous: real serial equipment can deliver another character while RDR is still occupied, causing overrun if software fails to read fast enough.

The following endpoint paths now use physical line occupancy rather than RDR emptiness:

- internal ASR-33 paper-tape reader;
- Text Terminal host input;
- raw TCP serial input;
- external COM input.

This does **not** mean two characters can occupy the receive shift register simultaneously. `serial_rx_line_idle()` still prevents overlapping frames in the single physical receiver path. It only removes the non-historical pause caused by an unread RDR.

The COM bridge deserves one additional distinction: the host OS/UART has already applied the real external baud/framing before RusTair sees a byte, so RusTair does not add a second host-link delay. It still respects the emulated MC6850 receive shift-path occupancy before accepting the next completed host byte.

## 14. Regression evidence

### `tests/two_sio_prdy_timing.rs`

Protects:

- +1T on selected 88-2SIO `IN` in both engines;
- no extra wait on `OUT`;
- no wait on unmapped input;
- exact Cycle `T1 -> T2 -> Tw -> T3` progression and PRDY release.

### `src/mc6850.rs` unit tests

Protect:

- control word decoding;
- TDR/TSR separation;
- TDRE timing;
- finite RDR;
- delayed OVRN visibility;
- FE/PE association with current RDR character;
- CTS gating;
- DCD interrupt/latch clearing;
- RTS/BREAK electrical control combinations;
- master reset behavior.

### `src/machine/two_sio.rs` unit tests

Protect:

- exact baud-clock thresholding;
- 110-baud and 9600-baud frame timing;
- endpoint presentation independence from TDRE;
- no hidden unlimited pre-ACIA RX FIFO;
- receiver line occupancy versus RDR state;
- a second frame can produce real OVRN while the previous RDR byte remains unread;
- modem pin propagation.

### `tests/two_sio_idle_chassis_clock.rs`

Protects independent card operation during STOP, RESET and HOLD/HLDA, plus no double advancement during RUN.

### `tests/two_sio_modem_pins.rs`

Protects the shared Fast/Cycle pin contract, exact `11h`/`51h`/`71h` RTS/BREAK behavior, CTS/DCD status projection and DCD clear sequence.

### `tests/asr33_reader_control_architecture.rs`

Protects the new host/peripheral boundary:

- reader motor command is resolved from the selected wiring and real MC6850 RTS pin;
- the reader update path contains no CPU `RUN` dependency;
- the reader advances on physical RX-line availability, not RDR emptiness;
- manual Read/Pause controls cannot override 88-TYA RTS mode;
- Text Terminal, TCP and COM input also gate on line occupancy rather than RDRF/RDR emptiness;
- repaint scheduling keeps the independently clocked reader alive while the CPU is stopped.

### Authentic loader regression

`app::authentic_loader::tests::bootstrap_consumes_reader_bytes_with_real_guest_in_on_both_rust_engines` verifies that paper-tape bytes traverse timed 88-2SIO receive hardware and are ultimately consumed by a genuine guest `IN 11h` rather than a loader-side memory shortcut.

## 15. Validation history

- 2026-08-31: `two_sio_prdy_timing` and full local suite green after card-level input wait implementation.
- 2026-08-31: authentic loader regression and full local suite green after timed RX/RDR separation.
- 2026-08-31: idle chassis clock tests and full local suite green for STOP/RESET/HOLD behavior.
- 2026-08-31: modem-pin/line-idle work reached a full local green suite after the unrelated debugger architecture guard was made semantic rather than dependent on a local variable name.
- 2026-08-31: 88-TYA Reader Control + endpoint physical-line migration implemented; **local validation pending**.

GitHub Actions were not used for these checkpoints.

## 16. Current gaps before 88-2SIO `PASS`

Digital/electrical blockers still open:

1. Validate the new 88-TYA Reader Control / endpoint line-idle block locally in both engines and against the full suite.
2. Propagate BREAK appropriately to attached endpoint models instead of exposing it only as a pin/state observation.
3. Expose CTS/DCD behavior/configuration sensibly for endpoint types that can provide physical modem inputs.
4. Expose physical per-port baud-generator straps in configuration.
5. Expose the board base-address strap block instead of permanently fixing `10h`–`13h`.
6. Re-run complete serial/loader regressions after the final strap/signal closeout.

Configuration/UX gap that does not change the current electrical claim:

- `ReaderControlMode` currently defaults to Manual each process start; persistence of this UI wiring choice can be added to saved settings without changing the underlying hardware behavior.

Potentially acceptable non-blocking analog omissions:

- exact 1488/1489 analog voltage thresholds/slew;
- cable capacitance;
- relay contact bounce;
- oscillator ppm tolerance;
- analog noise probability.

Those are not required for the current digital hardware fidelity claim unless a later project goal explicitly expands into analog fault simulation.

## 17. Primary references

### MITS 88-2SIO

**MITS, _Altair 88-2-SIO Documentation_, reprinted March 1977.** Primary board manual: schematics, address selection, MC6850 control/status description, baud selection and PRDY/PWAIT input-wait theory.

Archive: https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

Important sections used by this implementation:

- board addressing / A0-A7 selection;
- control-register bit tables;
- status-register bit definitions;
- baud-rate selector information;
- input-cycle theory describing SINP, flip-flop V, PRDY, PWAIT and the 500 ns wait state.

### Motorola MC6850

**Motorola Semiconductor Products Inc., _MC6800 Microcomputer System Design Data_, 1976 — MC6850 Asynchronous Communications Interface Adapter section.** Primary semiconductor authority for ACIA register and error semantics.

Archive: https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf

Relevant material includes the MC6850 section beginning around printed page 49 and its control/status descriptions, including RDRF, TDRE, DCD, CTS, FE, OVRN, PE, IRQ, double buffering and `/1` `/16` `/64` clocking.

Additional primary Motorola reference:

_M6800 Systems Reference and Data Sheets_, May 1975:
https://vtda.org/docs/computing/Motorola/M6800SystemsReferenceDataSheets_May75.pdf

This source is especially useful for the documented delayed receiver-overrun sequence.

### MITS 88-TYA / ASR-33 Reader Control

**MITS, _88-TYA Call/Control Unit Theory and Assembly Manual_.** Primary authority for the program-controlled paper-tape reader and its 88-2SIO RTS connection.

A readable scan is indexed at:
https://www.manualslib.com/manual/4116676/Mits-88-Tya.html

Reader Control / “Paper Tape Reader Control With 88-2SIO” is on scan page 9. It identifies RTS/bit 6 as the reader-control signal and documents octal `021` versus `121` initialization behavior.

Contemporary corroboration: **MITS Computer Notes, 1976**, description of the 88-TYA Call-Control Kit and its circuit for program control of the reader:
https://altairclone.com/downloads/computer_notes/1976_01_10.pdf

### Intel 8080 timing context

Intel 8080-family documentation defines READY/WAIT and the Tw extension used to interpret the board's 500 ns PRDY delay at the stock 2 MHz CPU clock. One later consolidated primary Intel manual is:

_Intel MCS-80/85 User's Manual_, Intel, 1983:
https://www.bitsavers.org/components/intel/MCS80/MCS80_85_Users_Manual_Jan83.pdf

RusTair's board-specific wait duration is taken from MITS; Intel is the authority for how READY produces Tw on the processor.

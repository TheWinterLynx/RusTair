# 88-2SIO external COM electrical signals: CTS, DCD and BREAK

Status: **IMPLEMENTED — local compile/full-suite validation pending for this block.**

Parent hardware document: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Project documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Purpose

This document covers the electrical boundary between a RusTair **MITS 88-2SIO / Motorola MC6850** channel and the **External COM** endpoint backed by the host operating system's serial-port API.

The important distinction is that a COM endpoint is not merely a byte pipe. A physical asynchronous serial connection may also expose control/status signals and a continuous BREAK condition. For the fidelity target used by RusTair, these signals must retain their electrical meaning:

- MC6850 `CTS` is an input pin;
- MC6850 `DCD` is an input pin;
- MC6850 `RTS` is an output pin, already modeled elsewhere;
- MC6850 BREAK is a continuous **spacing level on Tx Data**, not a byte value;
- host RS-232 APIs commonly report logical signal **assertion**, while the MC6850 documentation describes literal TTL input levels;
- the MC6850 CTS and DCD inputs are active LOW in their normal asserted conditions.

No host-side convenience flag may directly edit RDRF, TDRE or IRQ. The COM layer only drives physical inputs or receives physical outputs; the MC6850 remains the authority for register/status consequences.

## 2. Primary hardware evidence

### 2.1 MITS 88-2SIO installation requirement

Primary source:

**MITS, _Altair 88-2-SIO Documentation_, reprinted March 1977**, notice on manual page 1-1.

Archive:

https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

The notice states that if **Data Carrier Detect** and **Clear to Send** are not connected, they must be **jumpered to Ground**.

This has an important emulation consequence: an unconnected/no-modem endpoint must not leave CTS/DCD floating or default them HIGH. RusTair therefore defines the default physical COM wiring as:

```text
MC6850 CTS = LOW
MC6850 DCD = LOW
```

This is modeled as `ComModemInputMode::Grounded` and is the default configuration.

### 2.2 Motorola MC6850 CTS semantics

Primary source:

**Motorola Semiconductor Products Inc., _MC6800 Microcomputer System Design Data_, 1976, MC6850 section**, particularly the status-register description around printed pages 4-533 through 4-535.

Archive:

https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf

Motorola specifies:

- status bit 3 reflects the CTS input;
- **CTS LOW means Clear-To-Send is present**;
- CTS HIGH sets the CTS status bit and inhibits TDRE;
- master reset does not alter the external CTS status.

Therefore the useful/active modem condition is LOW at the MC6850 TTL input.

### 2.3 Motorola MC6850 DCD semantics

The same Motorola source specifies:

- DCD HIGH inhibits/initializes the receiver and causes RDRF to indicate empty;
- a **LOW-to-HIGH** DCD transition can generate a receive interrupt when CR7 is enabled;
- DCD status remains latched according to the documented status-read/data-read sequence.

Again, normal carrier-present operation corresponds to a LOW physical input.

### 2.4 Motorola BREAK semantics

Motorola defines transmitter control CR6:CR5=`11` as:

- RTS LOW;
- transmit interrupt disabled;
- **transmit a Break level (space) on the Transmit Data Output**.

BREAK is therefore not character `00h`, ASCII NUL, or any other ordinary byte. It is an out-of-band electrical line condition that persists while the control state remains selected.

## 3. Host serial API versus MC6850 pin polarity

RusTair uses Rust `serialport` 4.9 for the physical/virtual COM transport.

Implementation API reference:

https://docs.rs/serialport/4.9.0/serialport/trait.SerialPort.html

The host API methods:

```rust
read_clear_to_send() -> Result<bool>
read_carrier_detect() -> Result<bool>
```

return whether the corresponding modem signal is **asserted**.

That is not the same vocabulary as the MC6850 status description. At the emulator cable boundary, asserted host CTS/CD must be translated into the active-LOW TTL input expected by the ACIA:

| Host API result | Meaning | MC6850 physical pin |
| --- | --- | --- |
| `CTS asserted = true` | modem says Clear To Send | `CTS LOW` |
| `CTS asserted = false` | no Clear To Send | `CTS HIGH` |
| `CD asserted = true` | carrier present | `DCD LOW` |
| `CD asserted = false` | carrier absent | `DCD HIGH` |

RusTair makes the inversion explicit:

`src/app/external_com.rs`:

```rust
const fn mc6850_active_low_pin_high(host_asserted: bool) -> bool {
    !host_asserted
}
```

This conversion is deliberately located at the cable/app boundary. `ComSerialTransport` reports host assertion semantics; `Mc6850` consumes literal TTL HIGH/LOW semantics.

## 4. Physical wiring modes

`src/config/external_com.rs` defines:

```rust
pub enum ComModemInputMode {
    Grounded,
    HostPins,
}
```

### 4.1 Grounded — MITS no-modem jumpers

This is the default and directly represents the MITS installation notice.

The emulated channel receives:

```text
CTS LOW
DCD LOW
```

regardless of what the host USB/COM adapter reports.

Use this for:

- a three-wire/non-modem serial connection;
- terminal equipment where CTS/DCD are not physically wired;
- reproducing the common MITS jumper configuration.

### 4.2 Follow host CTS / Carrier Detect

In this mode the host serial adapter's modem pins are treated as physically connected to the selected 88-2SIO channel.

The worker polls:

```rust
port.read_clear_to_send()
port.read_carrier_detect()
```

and forwards changes to the emulator thread as logical host assertions. The app then inverts them into MC6850 pin levels before calling the backend hardware contract.

## 5. Ownership map

| Physical function | RusTair owner | Source |
| --- | --- | --- |
| MC6850 CTS/DCD electrical/status semantics | `Mc6850` | `src/mc6850.rs` |
| board/channel pin exposure | `TwoSioPort` | `src/machine/two_sio.rs` |
| backend-neutral modem input drive | `MachineBackend::serial_set_modem_inputs` | `src/backend/mod.rs` + Fast/Cycle adapters |
| app cable helper | `serial_set_modem_inputs_at` | `src/app/serial_hardware.rs` |
| host RS-232 pin polling | `ComSerialTransport` worker | `src/io/com_serial.rs` |
| host assertion -> MC6850 TTL polarity conversion | External COM controller | `src/app/external_com.rs` |
| modem wiring selection | `ComModemInputMode` | `src/config/external_com.rs` |
| MC6850 BREAK observation | `serial_break_active_at` | `src/app/serial_hardware.rs` |
| OS serial BREAK output | `WorkerCommand::SetBreak` | `src/io/com_serial.rs` |

## 6. CTS/DCD code path

The COM worker observes host modem pins without mutating emulator registers:

```rust
if let (Ok(cts_asserted), Ok(dcd_asserted)) =
    (port.read_clear_to_send(), port.read_carrier_detect())
{
    ...
    WorkerEvent::ModemPins { cts_asserted, dcd_asserted }
}
```

The transport exposes the latest host assertion state:

```rust
pub(crate) fn modem_pins_asserted(&self) -> Option<(bool, bool)> {
    self.modem_pins_asserted
}
```

The app resolves the installed cable:

```rust
let (cts_high, dcd_high) = match config.modem_inputs {
    ComModemInputMode::Grounded => (false, false),
    ComModemInputMode::HostPins => self
        .external_com
        .port
        .modem_pins_asserted()
        .map(|(cts_asserted, dcd_asserted)| {
            (
                mc6850_active_low_pin_high(cts_asserted),
                mc6850_active_low_pin_high(dcd_asserted),
            )
        })
        .unwrap_or((false, false)),
};

self.serial_set_modem_inputs_at(connection, cts_high, dcd_high);
```

From that point onward the ordinary MC6850 implementation determines TDRE, DCD status, RDRF and IRQ. The COM endpoint does not set those bits itself.

## 7. Cable removal and stale-input prevention

Physical cable changes require explicit cleanup.

A previous implementation risk in any modem-pin bridge is:

1. external COM drives DCD HIGH;
2. endpoint is moved/disconnected;
3. no device remains to drive the old virtual channel;
4. stale HIGH persists forever in the emulated ACIA.

RusTair tracks the previous COM connection and grounds the old channel when the virtual cable moves:

```rust
if previous_connection != connection {
    if previous_connection.is_connected() {
        let _ = self.serial_set_modem_inputs_at(
            previous_connection,
            false,
            false,
        );
    }
    self.external_com.last_connection = connection;
}
```

This again follows the MITS no-modem grounding rule rather than choosing an arbitrary idle value.

Power-off/disconnected handling also clears the host BREAK request and restores grounded modem inputs where a channel remains selected.

## 8. BREAK code path

### 8.1 MC6850 side

The ACIA's CR6:CR5 transmitter-control bits already expose:

```rust
break_active: bool
```

through the shared `SerialModemLines` backend structure.

### 8.2 App/cable side

The External COM controller reads the actual selected 88-2SIO signal:

```rust
let break_active = self
    .serial_break_active_at(connection)
    .unwrap_or(false);

self.external_com.port.set_break_active(break_active);
```

### 8.3 Host OS side

The worker receives an out-of-band command:

```rust
WorkerCommand::SetBreak(bool)
```

and maps it to the operating-system serial control:

```rust
let result = if active {
    port.set_break()
} else {
    port.clear_break()
};
```

No character is inserted into the serial data stream.

That distinction is essential: a remote UART sees a continuous spacing condition, not a normal 8-bit zero character.

## 9. Fast versus Cycle Accurate

There is no intentional Fast/Cycle divergence in the CTS/DCD/BREAK API.

Both engines expose the same:

- `serial_modem_lines()` output contract;
- `serial_set_modem_inputs()` input contract;
- MC6850 status behavior;
- PINT refresh after external modem-input changes.

The difference between Fast and Cycle remains CPU/bus timing granularity, not modem-pin semantics.

An external COM pin transition is an asynchronous hardware event relative to the 8080 instruction stream. The machine layer refreshes the canonical interrupt projection immediately after modem inputs change; it does not wait for a later unrelated guest I/O operation.

## 10. Relationship to host hardware flow control

`ExternalComConfig::flow_control` configures the **host OS serial port** and may use RTS/CTS in the host driver.

`ComModemInputMode` separately defines whether host CTS/CD are electrically connected to the **emulated MC6850 inputs**.

These are different layers:

- host flow control controls how the operating system transports bytes on the real COM device;
- emulated CTS/DCD wiring determines what the Altair's MC6850 sees in its status register and interrupt logic.

RusTair keeps the options separate to avoid implying that enabling host RTS/CTS flow control automatically rewires the historical 88-2SIO modem inputs.

## 11. UI / teaching visibility

The External COM configuration identifies the CTS/DCD wiring explicitly:

- `Grounded — MITS no-modem jumpers`
- `Follow host CTS / Carrier Detect`

The transport-state view also shows:

- host CTS assertion;
- host CD assertion;
- current MC6850 BREAK state.

The labels deliberately distinguish host **assertion** from MC6850 physical HIGH/LOW semantics.

## 12. Regression evidence

### Existing hardware regressions

`tests/two_sio_modem_pins.rs` already protects the shared Fast/Cycle MC6850 pin behavior, including:

- CTS status projection;
- DCD status and IRQ/latch behavior;
- `11h`, `51h`, `71h` transmitter-control behavior;
- BREAK state exposure.

### New configuration unit test

`src/config/external_com.rs` verifies that the default physical modem wiring is `Grounded`.

### Active-LOW conversion unit test

`src/app/external_com.rs` verifies:

```text
host asserted     -> MC6850 pin HIGH = false
host deasserted   -> MC6850 pin HIGH = true
```

### Architecture guard

`tests/two_sio_external_com_signals.rs` protects:

- the MITS grounded default;
- host CTS/CD polling through real serial-port methods;
- OS-native `set_break` / `clear_break` usage;
- explicit active-LOW conversion;
- physical input drive through `serial_set_modem_inputs_at`;
- cleanup of stale modem levels when the COM cable moves;
- prohibition on representing BREAK as a magic data byte.

## 13. Current validation state

Previous 88-TYA Reader Control / physical RX pacing block:

- **locally validated green with the full normal `cargo test` suite on 2026-08-31**;
- obsolete `terminal_serial_rx_empty` helper removed with the suite remaining green;
- no GitHub Actions were run.

This CTS/DCD/BREAK-to-COM block was implemented after that green checkpoint and is **not yet locally compiled/validated** at the time this document was written.

## 14. Remaining limitations / next work

This checkpoint closes the physical COM bridge only after local validation. The following broader 88-2SIO work remains:

1. Determine and model the precise internal transmitter behavior while MC6850 BREAK is continuously selected, especially the relationship between TDR/TSR progress and post-BREAK data, using primary Motorola evidence rather than emulator precedent.
2. Represent BREAK meaningfully on internal ASR/text-terminal endpoints; raw TCP has no native electrical BREAK concept and must not fake one as a byte.
3. Expose physical per-port MITS baud-generator straps.
4. Expose the A2-A7 board base-address straps.
5. Complete final serial/loader/full-suite validation before marking the 88-2SIO `PASS`.

## 15. References

### Primary: MITS 88-2SIO

MITS, _Altair 88-2-SIO Documentation_, reprinted March 1977.

https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

Used here for:

- manual page 1-1 requirement to ground unconnected DCD and CTS;
- board/MC6850 interface context;
- physical hardware configuration philosophy.

### Primary: Motorola MC6850

Motorola Semiconductor Products Inc., _MC6800 Microcomputer System Design Data_, 1976, MC6850 section.

https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf

Used here for:

- CTS LOW = Clear-To-Send present;
- CTS HIGH inhibits TDRE;
- DCD HIGH receiver/status behavior;
- DCD LOW-to-HIGH interrupt condition;
- CR6:CR5 BREAK = continuous spacing level on Tx Data;
- master reset retaining external CTS/DCD conditions.

Additional readable MC6850 scan:

https://www.cpcwiki.eu/imgs/3/3f/MC6850.pdf

### Implementation API reference: Rust serialport 4.9

https://docs.rs/serialport/4.9.0/serialport/trait.SerialPort.html

This is **not historical hardware authority**. It is referenced only to document the host API methods used to obtain CTS/CD assertion state and to set/clear an operating-system serial BREAK condition.

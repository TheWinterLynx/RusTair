# MITS 88-SIO A/B/C electrical interface fidelity

Status: **IMPLEMENTED AT THE CARD/CHASSIS BOUNDARY — endpoint wiring validation pending.**

Parent hardware document: `docs/88_SIO_HARDWARE_FIDELITY.md`.

Project documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope

The original MITS 88-SIO has one COM2502-family UART and one common set of board-side serial/handshake signals. The suffixes A, B and C do **not** identify different UART register sets or software protocols. They identify the electrical interface fitted between the common TTL-domain board logic and the external wafer connector.

RusTair therefore keeps two separate layers:

1. COM2502 + 88-SIO board logic: `RSI`, `RIN`, `ROT`, `TSO`, `BIN`, `BOT`.
2. Selected line interface: A/RS-232, B/TTL or C/TTY current loop.

No MC6850 `RTS`, `CTS`, `DCD` or BREAK semantics are imported from the later 88-2SIO.

## 2. Primary MITS evidence

Primary source:

**MITS, _Serial I/O Board Documentation_, 1975**, especially the section **Serial I/O Interface Operation** and the preceding status/handshake description.

Archive:

https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIO%20Documentation.pdf

The manual names the common logical signals:

- receive serial input `RSI`;
- input-device-ready pulse `RIN`;
- output-device-ready pulse `ROT`;
- transmit serial output `TSO`;
- input busy `BIN`;
- output busy `BOT`.

At the external interface these appear as the corresponding `SRSI`, `SRIN`, `SROT`, `STSO`, `SBIN` and `SBOT` connections.

The same manual states that DATA IN resets the input-ready flip-flop and DATA OUT resets the output-ready flip-flop. Thus `RIN`/`ROT` are independent external-device events, not aliases for COM2502 RDA/TBMT.

## 3. 88-SIO A — RS-232 level interface

MITS describes the A interface as its standard RS-232 interface.

For signals driven **from the 88-SIO** (`TSO`, `BIN`, `BOT`):

| Board TTL level | External A-interface level |
| --- | --- |
| HIGH | negative RS-232 level, nominally about -12 V |
| LOW | positive RS-232 level, approximately +3 V or greater |

For signals driven **into the 88-SIO** (`SRSI`, `SRIN`, `SROT`), the conversion is the inverse mapping back to TTL:

| External A-interface level | Board TTL level |
| --- | --- |
| positive RS-232 | LOW |
| negative RS-232 | HIGH |

RusTair represents the external states as typed values `Rs232Positive` and `Rs232Negative`; they are not collapsed into the same boolean type used for TTL.

## 4. 88-SIO B — TTL level interface

MITS describes B as the standard TTL-level interface and explicitly uses non-inverting buffers.

Therefore:

| Board logic | External B interface |
| --- | --- |
| LOW | TTL LOW |
| HIGH | TTL HIGH |

The mapping is non-inverting in both directions for `RSI/RIN/ROT` and `TSO/BIN/BOT`.

RusTair represents these as `TtlLow` and `TtlHigh`.

## 5. 88-SIO C — TTY/current-loop interface

MITS describes C as the TTY-level/current-loop interface.

For board outputs (`TSO`, `BIN`, `BOT`):

- board TTL HIGH drives the interface transistor into conduction and supplies loop current to the external device;
- board TTL LOW turns that transistor off, presenting a high-impedance/open current-loop output.

For board inputs (`SRSI`, `SRIN`, `SROT`), the receiver stages restore the same logical state at `RSI`, `RIN` and `ROT`; the logical relationship is therefore non-inverting even though the physical circuit contains transistor inversions internally.

RusTair represents the two external states as `CurrentLoopConducting` and `CurrentLoopOpen`. It does not claim analog current magnitude, loop voltage, cable resistance or transistor slew fidelity.

## 6. Six-signal logical boundary

`src/machine/sio.rs` exposes a stable board-side observation containing:

```text
RSI
RIN-ready-latched
ROT-ready-latched
TSO
BIN
BOT
```

`RIN` and `ROT` are pulses, so a snapshot cannot truthfully report an historical edge as a continuously asserted signal. Instead the observable state records the ready flip-flop that the corresponding pulse sets. The explicit pulse methods remain the event boundary.

`RSI` and `TSO` are not inferred from completed host bytes. They are generated from the actual in-flight asynchronous frame:

```text
start LOW
then data bits LSB first
then configured parity if present
then configured stop bit(s) HIGH
idle HIGH / MARK
```

The frame phase advances from the same chassis T-state clock already used by the COM2502 model, so Fast and Cycle observe the same serial-line state for the same elapsed hardware time.

## 7. Electrical boundary types

`src/config/sio_electrical.rs` defines the public connector vocabulary:

```rust
pub enum SioElectricalLevel {
    Rs232Positive,
    Rs232Negative,
    TtlLow,
    TtlHigh,
    CurrentLoopOpen,
    CurrentLoopConducting,
}
```

and:

```rust
pub struct SioConnectorOutputs {
    pub stso: SioElectricalLevel,
    pub sbin: SioElectricalLevel,
    pub sbot: SioElectricalLevel,
}
```

Using distinct variants is intentional. A TTL HIGH cannot accidentally be passed to a configured RS-232 A input and accepted as though the cable were electrically compatible. `sio_decode_connector_input` returns `None` for a level from the wrong interface family.

## 8. Fast versus Cycle ownership

Both engines contain the same `AltairBus`/88-SIO card state. The electrical interface is therefore not implemented independently in each CPU engine.

`AltairBus` exposes:

- `sio_physical_wiring()` — revision plus IN/OUT interrupt destinations;
- `sio_logical_lines()` — the six common logical line states;
- `sio_connector_outputs()` — STSO/SBIN/SBOT after A/B/C conversion;
- `sio_decode_connector_input()` — connector input conversion back to board logic.

`tests/sio88_physical_boundary.rs` verifies that the Fast and Cycle chassis project identical logical and electrical states for the same installed card configuration.

## 9. Deliberately not fabricated

This implementation does **not** assume that receiving a host byte automatically pulses `RIN`, nor that COM2502 TBMT or host consumption automatically pulses `ROT`.

Those are separate wires/events on the original board. A specific virtual endpoint may drive them only when its modeled cable/device wiring justifies doing so. This prevents a byte-oriented terminal abstraction from silently changing Rev0 hardware behavior.

Likewise, no 88-2SIO/MC6850 modem signal is reused as a substitute.

## 10. Remaining endpoint closeout

The card/chassis electrical boundary is now representable and shared. The remaining work before the A/B/C block can be declared fully PASS is to audit each user-selectable endpoint connection and specify which physical 88-SIO signals that cable/device actually drives:

- ASR-33 / TTY current loop;
- Text Terminal virtual endpoint;
- external serial/TCP endpoint;
- physical host COM endpoint where applicable.

Until that cable-level audit is complete, RusTair must prefer a disconnected/un-driven handshake line over an invented ready pulse.

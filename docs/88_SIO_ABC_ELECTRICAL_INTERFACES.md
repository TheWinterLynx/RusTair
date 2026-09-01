# MITS 88-SIO A/B/C electrical interface fidelity

Status: **PASS — endpoint wiring audit and final local validation green on 2026-09-01.**

Parent hardware document: `docs/88_SIO_HARDWARE_FIDELITY.md`.

Project documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope

The original MITS 88-SIO has one COM2502-family UART and one common set of board-side serial/handshake signals. A, B and C do **not** identify different UART register sets or software protocols. They identify the electrical interface fitted between common TTL-domain board logic and the external connector.

RusTair therefore keeps two layers:

1. COM2502 + board logic: `RSI`, `RIN`, `ROT`, `TSO`, `BIN`, `BOT`;
2. A/RS-232, B/TTL or C/TTY current-loop adaptation.

No MC6850 `RTS`, `CTS`, `DCD` or BREAK semantics are imported from the later 88-2SIO.

## 2. Primary MITS evidence

Primary source:

**MITS, _Serial I/O Board Documentation_, 1975**, especially *Serial I/O Interface Operation* and the status/handshake description.

`https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIO%20Documentation.pdf`

The manual names the common signals:

- `RSI` receive serial input;
- `RIN` input-device-ready pulse;
- `ROT` output-device-ready pulse;
- `TSO` transmit serial output;
- `BIN` input busy;
- `BOT` output busy.

At the external interface the corresponding connections are `SRSI`, `SRIN`, `SROT`, `STSO`, `SBIN`, `SBOT`.

DATA IN resets the input-ready flip-flop and DATA OUT resets the output-ready flip-flop. Thus RIN/ROT are independent external-device events, not aliases for COM2502 RDA/TBMT.

## 3. 88-SIO A — RS-232

For board outputs (`TSO`, `BIN`, `BOT`):

| Board TTL | External A |
| --- | --- |
| HIGH | negative RS-232 |
| LOW | positive RS-232 |

For inputs, the inverse electrical mapping restores the board TTL level:

| External A | Board TTL |
| --- | --- |
| positive RS-232 | LOW |
| negative RS-232 | HIGH |

RusTair uses typed `Rs232Positive` / `Rs232Negative` states rather than pretending they are TTL booleans.

## 4. 88-SIO B — TTL

MITS uses non-inverting TTL buffers:

| Board logic | External B |
| --- | --- |
| LOW | TTL LOW |
| HIGH | TTL HIGH |

The relationship is non-inverting in both directions.

## 5. 88-SIO C — TTY/current loop

For board outputs (`TSO`, `BIN`, `BOT`) in the documented circuit:

- board TTL HIGH drives the output transistor into the conducting/current state;
- board TTL LOW turns it off and presents an open/high-impedance output.

The input circuitry restores the common logical polarity at `RSI/RIN/ROT`.

RusTair represents connector state as `CurrentLoopConducting` / `CurrentLoopOpen`. It does not claim analog loop current, voltage, cable resistance or transistor slew fidelity.

## 6. Six-signal logical boundary

`AltairBus::sio_logical_lines()` and backend `SioLogicalLines` expose:

```text
RSI
RIN-ready-latched
ROT-ready-latched
TSO
BIN
BOT
```

RIN/ROT are pulse events, so their stable observable representation is the ready flip-flop they set. Explicit pulse methods represent the event itself.

RSI/TSO expose asynchronous frame phase (start, LSB-first data, parity if configured, stop, idle MARK). Receive-side RSI represents an accepted baud-matched frame; RusTair does not currently simulate an independently clocked external transmitter against the COM2502 sampling clock.

## 7. Electrical boundary types

`src/config/sio_electrical.rs` defines:

```rust
pub enum SioElectricalLevel {
    Rs232Positive,
    Rs232Negative,
    TtlLow,
    TtlHigh,
    CurrentLoopOpen,
    CurrentLoopConducting,
}

pub struct SioConnectorOutputs {
    pub stso: SioElectricalLevel,
    pub sbin: SioElectricalLevel,
    pub sbot: SioElectricalLevel,
}
```

A level from the wrong family is rejected by `sio_decode_connector_input()` instead of silently coerced.

## 8. Fast versus Cycle

Both engines expose the same card state through the backend-neutral 88-SIO API. The A/B/C conversion is not independently reimplemented per CPU engine.

Regression coverage compares Fast and Cycle for:

- revision/interrupt wiring;
- six logical signals;
- STSO/SBIN/SBOT electrical output family;
- rejection of wrong-family connector inputs.

## 9. Endpoint wiring audit

The cable contract is explicit:

| Endpoint | 88-SIO compatibility | Nature |
| --- | --- | --- |
| ASR-33 | C only | direct current-loop endpoint |
| Text Terminal | A/B/C | explicit virtual peer matching selected family |
| External TCP | A/B/C | explicit virtual peer matching selected family |
| External COM | A only | direct host RS-232 endpoint |

This means:

- an ASR-33 cable is automatically disconnected if the physical card changes from C to A/B;
- an External COM cable is automatically disconnected if the physical card changes from A to B/C;
- reconnecting either incompatible physical endpoint is rejected rather than inserting an invisible converter;
- Text Terminal and TCP may remain connected when A/B/C changes because they are explicitly virtual peers, not claims about one fixed physical device interface.

None of these endpoint byte paths fabricates `RIN` or `ROT`. Those wires remain un-driven unless a modeled peripheral supplies explicit ready events based on documented hardware.

## 10. Address/cable UI boundary

The 88-SIO address is jumper-selectable. The serial-router UI no longer claims the stale fixed `00h/01h` pair for every 88-SIO connection.

Where the complete card config is available, status text can identify the selected interface and exact configured status/data addresses. Generic selectors use a neutral `configured I/O` label rather than lying about an address they do not own.

## 11. Regression coverage

- `tests/sio88_physical_boundary.rs` — Fast/Cycle six-line and A/B/C parity.
- `tests/sio88_endpoint_wiring.rs` — endpoint compatibility, no automatic RIN/ROT, no old fixed-address cable label.
- `src/io/serial_router.rs` unit tests — direct endpoint compatibility matrix.
- machine unit tests — connector conversion and Rev0 ready-latch behavior.

## 12. Deliberate non-claims

This block does not claim:

- analog RS-232 voltage margins;
- exact TTL thresholds;
- current-loop current magnitude or cable effects;
- propagation delay/noise/contact bounce;
- automatic framing faults caused by remote-clock mismatch;
- undocumented device-ready contacts on generic terminal/TCP/COM endpoints.

Unsupported behavior remains explicit rather than being replaced by compatibility magic.

No implementation blocker remains in the stated A/B/C digital electrical/cable claim. The focused endpoint/physical-boundary tests and complete local `cargo test` suite were reported green on 2026-09-01. GitHub Actions were not run.

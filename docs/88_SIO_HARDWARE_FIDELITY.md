# MITS 88-SIO hardware fidelity

Status: **PASS — receive-BREAK correction plus focused and full local validation green on 2026-09-02.**

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

Related documents:

- `docs/88_SIO_INTERRUPT_ROUTING.md`
- `docs/88_SIO_ABC_ELECTRICAL_INTERFACES.md`
- `docs/BASE_HARDWARE_FIDELITY_CLOSEOUT.md`

## 1. Scope

This item models the original single-channel MITS 88-SIO as a physical card rather than as a generic host serial queue. The fidelity claim covers:

- COM2502-style finite receive/transmit state;
- Rev0 versus Rev1 status semantics;
- configurable I/O address, baud preset and word format;
- board-owned serial timing;
- original Rev0 external RIN/ROT ready flip-flops and BIN/BOT outputs;
- D0/D1 interrupt enables and physical PINT/VI routing;
- A/B/C electrical-interface families;
- a backend-neutral six-signal digital boundary shared by Fast and Cycle;
- explicit endpoint/cable compatibility without hidden level converters;
- physical receive BREAK as a held SPACE condition rather than a fabricated NUL byte.

Analog voltage/current tolerances, cable capacitance, transistor slew, noise and independent remote-clock drift are outside this digital claim.

## 2. Primary MITS evidence

Primary source:

**MITS, _Serial I/O Board Documentation_, 1975**

`https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIO%20Documentation.pdf`

Additional primary material in the same archive includes the Rev0 errata and Rev1 schematic.

The manual establishes the common logical signals:

- `RSI` — Receive Serial Input;
- `RIN` — Input device Ready pulse;
- `ROT` — Output device Ready pulse;
- `TSO` — Transmit Serial Output;
- `BIN` — input busy output;
- `BOT` — output busy output.

It also distinguishes Rev0 external device-ready state from COM2502 RDA/TBMT and documents A/B/C as electrical-interface variants rather than different UART register sets.

## 3. Physical configuration

`SioHardwareConfig` is the atomic dormant/installed card configuration:

```rust
pub struct SioHardwareConfig {
    pub revision: SioRevision,
    pub interface: SioInterface,
    pub address: SioAddressPair,
    pub baud: SioBaudRate,
    pub format: SioWordFormat,
    pub interrupt_wiring: SioInterruptWiring,
}
```

Changing these fields represents moving board hardware/jumpers and is POWER-OFF-only in the normal application UI.

The I/O decoder follows the configured even status/control address and adjacent odd data address. The old fixed `00h/01h` assumption is no longer the production authority; endpoint labels no longer claim that pair when the card has been readdressed.

## 4. Rev0 status register

Rev0 is intentionally not reduced to only the COM2502 flags:

| Bit | Meaning | Polarity |
| ---: | --- | --- |
| D7 | external Output Device Ready FF | active LOW |
| D6 | unused/not claimed | — |
| D5 | COM2502 RDA | active HIGH |
| D4 | overrun error | active HIGH |
| D3 | framing error | active HIGH |
| D2 | parity error | active HIGH |
| D1 | COM2502 TBMT | active HIGH |
| D0 | external Input Device Ready FF | active LOW |

With both external ready flip-flops reset and an empty transmitter holding register, the modeled clear/idle Rev0 status is `83h`.

A serial character completing reception sets RDA/D5 but **does not** set the external input-device-ready flip-flop. Likewise COM2502 TBMT does not set the external output-device-ready flip-flop.

## 5. Rev0 external ready handshake

The original external handshake is explicit:

```text
RIN pulse -> input-device-ready FF set -> D0 becomes active LOW -> BIN high
DATA IN  -> RDAR + input-ready FF reset

ROT pulse -> output-device-ready FF set -> D7 becomes active LOW -> BOT high
DATA OUT  -> output-ready FF reset + UART transmit-data strobe
```

RusTair exposes explicit RIN/ROT pulse operations through `AltairBus`, `MachineBackend` and `BackendHost`.

No byte-oriented endpoint automatically calls them. This is deliberate: a terminal producing a byte is not evidence that the separate MITS ready-contact wire pulsed.

## 6. Rev1/internal-ready behavior

The supported Rev1/internal-ready behavior uses the later D0/D7 convention:

- D0 active LOW reflects COM2502 receive ready;
- D7 active LOW reflects transmitter-buffer availability;
- D4:D2 remain the error bits.

This keeps Rev1 software polling separate from the original Rev0 external-ready protocol.

## 7. Finite COM2502 model

`src/machine/sio.rs` separates UART storage and shift activity:

### Receive

- receive shift register / frame in progress;
- finite receiver holding register (`rx_data` / RDA);
- a second completed frame while RDA is still set records overrun and overwrites the old unread character with the newly completed one;
- framing/parity error state is associated with the completed receive frame;
- DATA IN resets RDA, not all error state by fiat;
- a held external BREAK forces RSI to SPACE/LOW, completes as zero data with framing error after one configured frame time, and can continue into natural overrun while held;
- releasing BREAK before a complete frame aborts that incomplete BREAK frame and does not fabricate an ASCII NUL character.

### Transmit

- transmitter holding register;
- transmitter shift register;
- TBMT follows holding-register availability rather than host presentation completion;
- when the shift register is idle, a written byte is promoted immediately, so TBMT may return ready while the character is still physically shifting;
- a second byte may occupy the holding register while the first is in flight.

The completed endpoint byte queue is downstream of the UART shift process and is not the hardware-ready source.

## 8. Board-owned timing

The 88-SIO owns its baud/format clock. `SioPort::advance_t_states()` advances frame phase from elapsed chassis T-states using the selected card baud.

This has two important consequences:

1. serial hardware timing is not derived from UI animation or terminal presentation pacing;
2. Fast and Cycle observe the same card state after the same elapsed physical card time.

The serial-card oscillator also has an idle-chassis path so hardware can continue advancing while the CPU is stopped rather than freezing merely because no instruction executes.

## 9. RSI and TSO digital line state

RusTair exposes instantaneous board-side `RSI` and `TSO` logic levels for an asynchronous frame:

```text
start LOW
then data bits LSB first
then configured parity if present
then configured stop bit(s) HIGH
idle HIGH / MARK
```

This is a real bit-phase projection of the modeled in-flight frame, not a boolean such as “RX queue non-empty”. A held receive BREAK overrides RSI to continuous LOW/SPACE for the duration of the physical condition.

### Important receive-side boundary

The normal endpoint byte API accepts a character and the 88-SIO then represents the corresponding **baud-matched accepted frame** on RSI using the card timing. RusTair does not yet run an independently clocked remote transmitter bitstream against the COM2502 sampling clock.

BREAK is modeled separately because it is a persistent physical line condition, not a byte. Therefore the current fidelity claim includes held BREAK/SPACE and its framing-error consequence without claiming arbitrary analog or independently clocked remote waveform sampling.

The current fidelity claim does **not** include automatically generating framing/parity corruption from endpoint baud mismatch, phase drift, electrical noise or marginal sampling. Those remain explicit out-of-scope fault-injection/analog work and must not be inferred from the presence of `rsi_high`.

## 10. A/B/C electrical interfaces

The common board logic is converted at the connector boundary:

### A — RS-232

Board HIGH outputs (`TSO/BIN/BOT`) become negative RS-232 levels; board LOW becomes positive. Inputs are converted back with the inverse electrical mapping.

### B — TTL

Non-inverting TTL LOW/HIGH mapping in both directions.

### C — TTY/current loop

For the documented output circuit, board HIGH drives the output transistor/conducting state; board LOW produces open/high-impedance state. Receiver circuitry restores the common logical polarity on inputs.

Typed external states are used (`Rs232Positive`, `Rs232Negative`, `TtlLow`, `TtlHigh`, `CurrentLoopOpen`, `CurrentLoopConducting`) so incompatible electrical families cannot accidentally be accepted as the same bool.

See `docs/88_SIO_ABC_ELECTRICAL_INTERFACES.md` for the detailed mapping.

## 11. Endpoint/cable contract

The endpoint audit deliberately distinguishes physical devices from virtual peers:

| Endpoint | 88-SIO direct/virtual compatibility | Ready-wire behavior |
| --- | --- | --- |
| Built-in ASR-33 | direct C / current-loop only | serial data plus physical held BREAK; does not invent RIN/ROT |
| Text Terminal | virtual peer instantiated in selected A/B/C family | data only; does not invent RIN/ROT |
| External TCP | virtual peer instantiated in selected A/B/C family | data only; does not invent RIN/ROT |
| External COM | direct A / RS-232 only | data only for 88-SIO; does not invent RIN/ROT |

Changing the physical 88-SIO interface automatically disconnects an attached physical endpoint that is no longer electrically compatible. Attempting to reconnect it is rejected with an explicit message rather than installing a hidden adapter.

The built-in ASR-33 BREAK key is not passed through `key_to_byte()`. In LINE mode it drives the selected UART receive line to BREAK/SPACE while held and returns it to MARK when released. Leaving LINE, disconnecting/moving the ASR cable, or displacing the ASR with another endpoint restores MARK on the old physical port before routing changes.

The virtual Text Terminal/TCP peers are not claims about a historical physical terminal model; they are explicit host-side peers whose connector side follows the selected digital electrical family. They still do not gain separate MITS ready contacts.

External COM framing/baud remains independently configurable. RusTair does not silently synchronize a real host COM endpoint to the 88-SIO board; a mismatch represents a misconfigured peer, although receive-bit sampling faults from that mismatch are outside the current endpoint model as described above.

## 12. Interrupt routing

OUT to the status/control address stores D0 input-enable and D1 output-enable. The source conditions are revision-sensitive:

- Rev0: external input/output ready flip-flops;
- Rev1: internal COM2502 ready state.

Each resulting IN/OUT request can be wired to:

- `Disconnected`;
- direct `Pint`;
- raw `Vi0..Vi7`.

VI lines remain raw chassis requests. The 88-SIO does not fabricate an interrupt vector for them. See `docs/88_SIO_INTERRUPT_ROUTING.md`.

## 13. Fast versus Cycle ownership

There is one physical 88-SIO model per backend chassis, not two behaviorally independent UART implementations.

The backend-neutral API exposes:

- `SioLogicalLines` — RSI, latched RIN/ROT ready state, TSO, BIN, BOT;
- `sio_connector_outputs()` — STSO/SBIN/SBOT after A/B/C conversion;
- `sio_decode_connector_input()` — selected-family connector input conversion;
- explicit RIN/ROT pulse operations;
- `serial_set_receive_break()` — a backend-neutral physical receive BREAK/SPACE line operation used by the ASR-33 rather than a special byte.

Regression tests require Fast and Cycle to project identical 88-SIO logical/electrical state for the same card configuration.

Cycle additionally owns exact CPU T-state visibility; Fast remains instruction-level and must not claim exact CPU pin timing that it does not possess.

## 14. Physical-to-code mapping

- `src/config/sio.rs` — revision, address, baud, framing, interface and interrupt wiring.
- `src/config/sio_electrical.rs` — typed connector-level vocabulary.
- `src/machine/sio.rs` — finite COM2502, frame timing, receive BREAK, Rev0 ready flip-flops, RSI/TSO.
- `src/machine/sio_interface.rs` — A/B/C electrical conversion.
- `src/machine/io_devices.rs` — I/O decode, card ownership, explicit ready pulses and receive-BREAK dispatch.
- `src/machine/serial.rs` — six-line/connector boundary on `AltairBus`.
- `src/machine/mod.rs` — D0/D1 interrupt enables and PINT/VI projection.
- `src/backend/mod.rs`, `native.rs`, `cycle_host.rs` — backend-neutral physical API.
- `src/io/serial_router.rs` — endpoint electrical compatibility.
- `src/app/serial_hardware.rs`, `src/app/mod.rs`, `src/app/asr33_controller.rs` — POWER-OFF reconfiguration, cable disconnect/reject policy and physical ASR BREAK handling.

## 15. Regression coverage

Important coverage includes:

- Rev0/Rev1 ready-bit positions and polarity;
- Rev0 RIN/ROT independence from COM2502 RDA/TBMT;
- DATA IN/OUT reset side effects;
- finite receive overrun and transmit double buffering;
- serial start/data/parity/stop bit progression;
- held receive BREAK as SPACE/LOW with zero+FE after a complete frame;
- short BREAK release without fabricated NUL;
- A/B/C typed level conversion;
- Fast/Cycle physical-boundary parity;
- Rev0 PINT/VI routing and non-fabricated interrupts;
- endpoint compatibility matrix;
- no endpoint calls hidden RIN/ROT pulse APIs;
- ASR BREAK release before cable rerouting;
- cable labels cannot regress to hard-coded `00h/01h`.

Relevant integration files include:

- `tests/sio88_hardware_fidelity.rs`
- `tests/sio88_physical_boundary.rs`
- `tests/sio88_interrupt_configuration.rs`
- `tests/sio88_configuration_ui.rs`
- `tests/sio88_endpoint_wiring.rs`
- `tests/serial_receive_break_fidelity.rs`

## 16. User-observable validation

1. POWER OFF and select MITS 88-SIO.
2. Move its I/O address away from `00h/01h`; the configuration remains authoritative and connection status must not claim the old fixed pair.
3. Select interface C and connect the ASR-33: direct connection is allowed.
4. POWER OFF and change to A or B: the ASR-33 cable must be disconnected rather than silently converted.
5. On A, External COM may be connected directly. Change to B or C while powered off: that COM cable must be disconnected.
6. Text Terminal or External TCP may remain attached as explicit virtual A/B/C peers.
7. On Rev0, receiving data may affect RDA/D5 but must not fabricate the external RIN-ready/D0 condition.
8. Explicit RIN/ROT test events must affect their ready latches, and DATA IN/OUT must clear them.
9. With the ASR in LINE mode on interface C, holding BREAK must drive a physical receive BREAK rather than inject one immediate NUL byte; releasing it returns the line to MARK.
10. Switching Fast/Cycle while powered off must preserve the same physical SIO configuration.

## 17. Known limits / non-claims

The 88-SIO base-card item intentionally does not claim:

- analog RS-232 voltage margins or rise/fall time;
- current-loop current magnitude, cable resistance or opto/transistor analog behavior;
- electromagnetic noise or contact bounce;
- independently clocked remote receive-bit sampling and automatic baud-mismatch framing faults;
- a complete 88-Vector Interrupt controller beyond raw VI request wires;
- undocumented RIN/ROT contacts for endpoints whose historical wiring does not establish them.

These limits do not require compatibility hacks: unsupported physical behavior remains un-driven or explicitly outside the claim.

No known implementation blocker remains inside the stated digital 88-SIO claim. The receive-BREAK regression, focused 88-SIO/88-2SIO serial tests, physical-boundary tests and complete local `cargo test` suite were reported green on 2026-09-02. GitHub Actions were not run.

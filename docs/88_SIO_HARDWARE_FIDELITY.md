# MITS 88-SIO hardware fidelity

Status: **IN PROGRESS — primary-source audit complete for the COM2502 core/status revisions; finite UART core implemented and entering production integration.**

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope and fidelity claim

This document covers the original single-channel MITS 88-SIO serial board. It is intentionally separate from `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`: the two boards use different UART hardware, different status words and different physical configuration mechanisms.

The target fidelity boundary includes:

- the COM2502-family UART used by the 88-SIO;
- original Rev 0 and later Rev 1 status-word behavior;
- finite receiver/transmitter holding and shift registers;
- RDA, TBMT, overrun, framing and parity status;
- board-owned serial clock/baud timing;
- 5/6/7/8 data-bit, parity and stop-bit hardwiring;
- selectable even control/status address plus following odd data address;
- 88-SIO A/B/C electrical interface variants;
- input/output interrupt enable flip-flops and their physical IN/OUT/BH routing;
- direct PINT versus VI0..VI7 boundary;
- continued board operation when the 8080 is STOPped, RESET-held or in HOLD/HLDA;
- truthful Fast versus Cycle behavior;
- user-observable validation from the normal application.

Analog line voltage/slew, cable capacitance and current-loop analog faults are outside the present digital claim unless they affect a documented digital status input.

## 2. Primary hardware

The 88-SIO is a single serial I/O board built around a COM2502-family universal asynchronous receiver/transmitter. Unlike the later 88-2SIO it does **not** contain an MC6850 and therefore must not inherit MC6850 register or modem-pin semantics.

MITS offered three line-interface assemblies around the same board/UART logic:

| Variant | Physical interface |
| --- | --- |
| 88-SIO A | EIA / RS-232 levels |
| 88-SIO B | TTL levels |
| 88-SIO C | TTY / current-loop-oriented interface |

RusTair represents this separately from the UART/status revision through `SioInterface`.

## 3. Address selection

The board uses seven address jumpers and occupies two consecutive I/O addresses:

- even address = control OUT / status IN;
- following odd address = data IN/OUT.

The MITS chart permits every even control address from `00h` through `FEh`; the data channel is therefore the following odd address from `01h` through `FFh`.

This means `FEh/FFh` is physically selectable even though `FFh` collides with the Altair front-panel sense-switch input. RusTair must not claim that a historically possible jumper setting is impossible merely because it is inconvenient. The UI may warn about the collision, but the hardware configuration model permits it.

`src/config/sio.rs` models the pair as:

```rust
pub struct SioAddressPair { base: u8 }

pub const fn try_new(base: u8) -> Option<Self> {
    if base & 1 == 0 { Some(Self { base }) } else { None }
}
```

## 4. Baud and hardwired word format

MITS documents a selectable serial rate through **25,000 baud**. The COM2502 receives a clock at sixteen times the serial bit rate.

The UART format is selected by physical logic/jumpers rather than a runtime control register. The audited configuration therefore keeps these as board hardware:

- data bits: 5, 6, 7 or 8;
- parity: none, even or odd;
- stop bits: 1 or 2;
- nominal baud: 0 through 25,000.

The default RusTair installation remains the historical teletype-oriented configuration used by the project: 110 baud, 8 data bits, no parity, 2 stop bits.

## 5. Rev 0 versus Rev 1 status words

This is a critical compatibility distinction, not a cosmetic label.

### 5.1 Rev 0 / original status word

The original board exposes the COM2502 ready outputs directly:

| Status bit | Meaning | Polarity |
| ---: | --- | --- |
| D5 | RDA / receiver data available | HIGH = ready |
| D4 | receiver overrun | HIGH = error |
| D3 | framing error | HIGH = error |
| D2 | parity error | HIGH = error |
| D1 | TBMT / transmitter buffer empty | HIGH = ready |

Period MITS software documentation describes polling D5 for input and D1 for output on this revision.

### 5.2 Rev 1 / modified status word

The later board modification moves the two software-ready indications and inverts them:

| Status bit | Meaning | Polarity |
| ---: | --- | --- |
| D7 | transmitter **not ready** | LOW = ready |
| D4 | receiver overrun | HIGH = error |
| D3 | framing error | HIGH = error |
| D2 | parity error | HIGH = error |
| D0 | receiver **not ready** | LOW = ready |

Thus common Rev 1 polling code waits for D0 to clear before input and D7 to clear before output.

The revision is represented explicitly:

```rust
pub enum SioRevision {
    Rev0,
    Rev1,
}
```

Default is Rev 1 because that preserves the later/common MITS software environment RusTair previously approximated. Rev 0 remains a real selectable hardware revision rather than a software hack.

## 6. Pre-audit RusTair behavior and defects

Before this closeout the 88-SIO path reused `SerialPort`, a generic byte queue whose state was owned partly by host endpoints.

The status value was synthesized as:

```rust
(if rx_empty { 0x01 } else { 0 }) | (if tx_busy { 0xc0 } else { 0 })
```

This happened to approximate two visible Rev 1 tests but was not the real board:

1. D0 approximately represented active-low receive ready.
2. D7 approximately represented active-low transmit ready.
3. **D6 was falsely duplicated with D7.**
4. D4/D3/D2 error outputs did not exist.
5. Rev 0 D5/D1 active-high ready status was impossible.
6. RX was an unbounded `VecDeque`, not a finite COM2502 receiver.
7. TX readiness followed host endpoint drain rather than COM2502 TBMT.
8. a completed unread receive character prevented the next host frame from starting, hiding real overrun.
9. board serial time was not advanced by a hardware-owned clock.
10. the address pair was fixed at `00h/01h` even though the physical card has address jumpers.

These are hardware-fidelity blockers, not presentation differences.

## 7. COM2502 receive pipeline

The receiver is double buffered:

```text
serial input
    -> receiver shift register
    -> receiver data-bits holding register
    -> CPU data bus
```

`RDA` describes the holding register, not line occupancy.

A crucial COM2502 overrun behavior differs from the MC6850 used by the 88-2SIO. When a new frame completes while RDA is already HIGH, the UART:

1. records overrun;
2. transfers the **new** shift-register character into the holding register;
3. leaves RDA HIGH.

The old unread character is therefore overwritten. RusTair must not reuse MC6850's “retain old byte / lose new byte” behavior here.

`src/machine/sio.rs` implements:

```rust
self.overrun = self.rx_full;
self.rx_data = value & mask;
self.rx_full = true;
```

A CPU data-channel read pulses the board's RDAR path and resets RDA. It does not fabricate an unrelated clearing rule for the error flags.

## 8. COM2502 transmit pipeline

The transmitter is also double buffered:

```text
CPU data bus
    -> transmitter holding register
    -> transmitter shift register
    -> serial output
```

TDS loads the holding register and drives TBMT LOW. If the shift register is idle, the UART transfers the holding byte into the shift register immediately; TBMT therefore returns HIGH even though that character is still physically being transmitted.

A second byte may occupy the holding register while the first frame is in progress. At the first frame boundary it transfers immediately, giving back-to-back serial characters without a fabricated idle gap.

This is fundamentally different from the old rule “TX busy until the host terminal has displayed/acknowledged the byte.”

## 9. Current RusTair implementation

### 9.1 Physical configuration

`src/config/sio.rs` now defines:

- `SioRevision`;
- `SioInterface`;
- `SioAddressPair`;
- `SioBaudRate`;
- `SioDataBits`;
- `SioParity`;
- `SioStopBits`;
- `SioWordFormat`;
- `SioHardwareConfig`.

### 9.2 Finite UART core

`src/machine/sio.rs` owns:

- receiver holding register and RDA;
- receiver shift path;
- ROR/RFE/RPE;
- transmitter holding register and TBMT;
- transmitter shift register;
- completed downstream wire-byte queue;
- exact integer serial phase accumulation.

The board clock advances by chassis T-states:

```rust
let total = self.bit_phase_numerator
    .saturating_add(t_states.saturating_mul(u64::from(baud)));
let boundaries = total / u64::from(cpu_clock_hz);
self.bit_phase_numerator = total % u64::from(cpu_clock_hz);
```

The physical COM2502 16x clock is represented by its resulting bit-boundary rate; no host endpoint owns TBMT/RDA timing.

### 9.3 Staged integration

The COM2502 module is compiled through `src/machine/serial.rs` while the production 88-SIO path is migrated from the old generic `SerialPort` to `SioPort`. This staging is deliberate: chip/core failures can be validated independently from the larger S-100 decoder/backend/UI change.

## 10. Interrupt control and physical routing

The MITS control output uses D0 and D1 as independent interrupt enable flip-flops:

| D1 | D0 | Enabled source(s) |
| ---: | ---: | --- |
| 0 | 0 | none |
| 0 | 1 | input |
| 1 | 0 | output |
| 1 | 1 | input + output |

The PCB exposes input, output and combined interrupt request points, plus VI0..VI7 connections for the MITS vectored-interrupt system. MITS also documents direct connection to the processor interrupt line, which results in the usual 8080 restart at octal 70 (`RST 7`, opcode `FFh`).

This routing is not yet closed in RusTair. The same rule used for the audited 88-2SIO applies: a raw board interrupt and a CPU PINT request are not synonyms. VIx routing must terminate at a raw vector-line boundary until a real 88-VI component consumes it.

## 11. Fast versus Cycle requirements

### Fast

Fast may account board activity at instruction boundaries using elapsed T-state deltas, but it must expose the same RDA/TBMT/error values and the same total serial elapsed time as Cycle.

### Cycle Accurate

Cycle advances the board from real elapsed chassis/CPU T-states. No 88-SIO-specific wait state is currently claimed by the primary evidence used in this audit; one must not be copied from the later 88-2SIO PRDY circuit.

### CPU parked

The 88-SIO clock is independent hardware. A frame already in flight must continue while the 8080 is STOPped, RESET-held or in HOLD/HLDA. RusTair's shared idle-chassis serial clock mechanism will be used once the production SIO path is connected.

## 12. Regression evidence already added

`src/config/sio.rs` protects:

- Rev 1 as migration/default revision;
- even status/data address-pair selection through `FEh/FFh`;
- 25,000-baud documented ceiling;
- default 110-baud 8N2 TTY configuration.

`src/machine/sio.rs` protects:

- exact Rev 0 D5/D1 active-high ready positions;
- exact Rev 1 D0/D7 active-low ready positions;
- absence of the old fabricated D6 ready bit;
- COM2502 overrun overwriting old unread data with the newly completed character;
- RDAR clearing RDA without inventing an error-clear side effect;
- double-buffered TX/TBMT returning ready before the character finishes;
- next-byte promotion at the exact previous-frame boundary;
- receive shift continuing while the previous holding-register byte remains unread.

These tests require local validation before this implementation phase is considered green.

## 13. How the user will validate it

The full manual procedure becomes available after configuration/UI integration. Required user-visible checks are already defined:

### 13.1 Rev 0 versus Rev 1

1. POWER OFF.
2. Install/select MITS 88-SIO.
3. Select Rev 0.
4. POWER ON and inspect the status port while RX is empty/TX ready: D1 must be HIGH; D5 LOW until a character is received.
5. Present one receive character. After its complete frame, D5 must become HIGH.
6. POWER OFF and select Rev 1.
7. Repeat: empty RX must expose D0 HIGH; a completed received character must clear D0. TX ready must be represented by D7 LOW.
8. D6 must never mirror TX-ready merely because old RusTair once did so.

### 13.2 Finite receiver and overrun

1. Configure a known baud/format.
2. Let one complete character reach RDA but do not read it.
3. Allow the next physical frame to complete.
4. Overrun D4 must set.
5. Reading DATA must return the **second/newer** character, demonstrating COM2502 overwrite semantics rather than MC6850 semantics.

### 13.3 Double-buffered transmitter

1. Write one byte while transmitter idle.
2. TBMT must return to its ready state after the holding byte transfers into the shift register, before the full serial frame finishes.
3. Write a second byte during the first frame. TBMT must become not-ready while that second byte occupies the holding register.
4. At the first frame boundary the second byte must promote immediately and TBMT must become ready again.

### 13.4 Board clock while CPU parked

1. Begin an RX or TX frame.
2. STOP the CPU before the frame completes.
3. Leave the chassis powered for longer than the remaining frame duration.
4. The 88-SIO frame must complete even though the CPU executed no instructions.
5. Repeat under sustained RESET and HOLD/HLDA in the diagnostic views.

## 14. Remaining blockers before PASS

1. Replace the production generic `SerialPort` 88-SIO path with `SioPort`.
2. Move status/data decoding to `SioAddressPair`.
3. Advance the 88-SIO baud clock during RUN and the shared idle-chassis path.
4. Replace endpoint-owned RX/TX-ready timing with UART-owned RDA/TBMT state.
5. Carry `SioHardwareConfig` through `MachineConfig`, Fast/Cycle backends and engine recreation.
6. Persist revision, address, baud, format and A/B/C interface.
7. Add POWER-OFF-only Configuration UI and dynamic endpoint labels.
8. Model 88-SIO input/output interrupt routing to disconnected/PINT/VIx without fabricating 88-VI vectors.
9. Validate endpoint behavior for A/B/C interfaces without inventing MC6850 modem pins.
10. Run focused Fast/Cycle tests, full local suite and the manual procedures above.
11. Update this document and the Point 1 ledger to `PASS` only after that green checkpoint.

## 15. Primary references

### MITS 88-SIO documentation

MITS, *Altair 88-SIO Serial I/O Interface* documentation and schematics, including Rev 0/Rev 1 material.

Archive index:

https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/

The archive includes the main 88-SIO documentation, Rev 0 errata, Rev 1 schematic and COM2502 data sheet.

The MITS 88-ACR manual also reproduces the standard SIO-B documentation and is useful for the address/baud/format/interface descriptions:

https://deramp.com/downloads/altair/hardware/cassette_interface/Altair%2088-ACR%20Cassette%20Interface.pdf

### COM2502

COM2502 / TR1602-family UART data sheet contained in the MITS archive above. Primary authority for:

- RDA/RDAR;
- TBMT/TDS;
- double-buffered receiver/transmitter paths;
- ROR, RFE and RPE;
- receiver-overrun flow showing transfer of the new character into the holding register.

### Period MITS software/status revision evidence

MITS *Computer Notes*, 1975 issues, documents the later status polling convention used by MITS software. The October 1975 material explicitly describes Rev 1-style receive ready on D0 active LOW and transmit ready on D7 active LOW.

Archive example:

https://altairclone.com/downloads/computer_notes/1975_01_05.pdf

Period programming literature also records the transition from original D5/D1 active-HIGH ready bits to modified D0/D7 active-LOW polling. These period sources are used to distinguish physical Rev 0/Rev 1 behavior rather than treating one convention as a compatibility hack.

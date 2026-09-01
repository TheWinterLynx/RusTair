# MITS 88-SIO interrupt routing fidelity

Status: **IMPLEMENTED — ready for final local validation.**

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope

This document covers interrupt generation and physical routing on the original single-channel MITS 88-SIO. RusTair keeps three layers separate:

1. the revision-dependent source condition;
2. the D0/D1 software interrupt-enable flip-flops;
3. the physical IN/OUT routing to direct PINT, an 88-VI input, or no connection.

The COM2502/card core is documented in `docs/88_SIO_HARDWARE_FIDELITY.md`; A/B/C connector behavior is documented in `docs/88_SIO_ABC_ELECTRICAL_INTERFACES.md`.

## 2. Primary MITS evidence

Primary sources:

- MITS, *Serial I/O Board Documentation* (1975):
  `https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIO%20Documentation.pdf`
- MITS, *88-SIOB Rev 1 Schematic*:
  `https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIOB%20Rev%201%20Schematic.pdf`
- MITS, *88-SIO Rev 0 Errata*:
  `https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIO%20Rev%200%20Errata.pdf`

The original manual is authoritative for the important Rev0 distinction: COM2502 RDA/TBMT and the external device-ready flip-flops are separate signals.

## 3. Software interrupt enables

OUT to the even status/control address uses:

| Control bit | Meaning |
| ---: | --- |
| D0 | input interrupt enable |
| D1 | output interrupt enable |

RusTair stores these in `AltairBus::sio_interrupt_control`. They are runtime card state written by guest software, not configuration-menu options.

## 4. Rev0 request sources

The original Rev0 status register contains both UART and external-handshake state:

| Bit | Rev0 meaning | Polarity |
| ---: | --- | --- |
| D7 | external Output Device Ready flip-flop | active LOW |
| D5 | COM2502 RDA | active HIGH |
| D4 | overrun | active HIGH |
| D3 | framing error | active HIGH |
| D2 | parity error | active HIGH |
| D1 | COM2502 TBMT | active HIGH |
| D0 | external Input Device Ready flip-flop | active LOW |

Therefore Rev0 interrupt sources are **not** RDA/TBMT:

```text
external RIN pulse -> input-ready FF -> D0 active LOW
                                    -> D0 software enable -> IN request

external ROT pulse -> output-ready FF -> D7 active LOW
                                     -> D1 software enable -> OUT request
```

DATA IN resets the input-ready flip-flop as well as COM2502 RDA. DATA OUT resets the output-ready flip-flop independently of COM2502 TBMT.

RusTair models these flip-flops explicitly. A received character does not synthesize RIN, and transmitter readiness does not synthesize ROT.

## 5. Rev1/internal-ready request sources

For the later Rev1/internal-ready behavior represented by RusTair, D0/D7 instead expose internal UART readiness:

```text
COM2502 RDA  -> D0 active LOW -> D0 software enable -> IN request
COM2502 TBMT -> D7 active LOW -> D1 software enable -> OUT request
```

The routing stage downstream of those revision-specific status bits is shared.

## 6. Physical IN / OUT routing

RusTair represents the resulting board wiring as:

```rust
pub enum SioInterruptTarget {
    Disconnected,
    Pint,
    Vi0, Vi1, Vi2, Vi3, Vi4, Vi5, Vi6, Vi7,
}

pub struct SioInterruptWiring {
    pub input: SioInterruptTarget,
    pub output: SioInterruptTarget,
}
```

Wiring both sources to the same destination represents the electrically relevant result of a combined BH arrangement. Keeping them independent also permits IN and OUT to use different VI priorities.

## 7. PINT is not VIx

`PINT` is the direct CPU interrupt request path. In RusTair's direct Altair interrupt mechanism, interrupt acknowledge supplies `FFh` (`RST 7`).

`VI0..VI7` are raw request wires for a separate 88-Vector Interrupt system. The 88-SIO itself cannot convert `VI3` into `RST 3` or any other CPU opcode. RusTair therefore exposes a raw VI mask and does not fabricate CPU vectors.

## 8. Physical-to-code mapping

- `src/machine/sio.rs`
  - owns Rev0 ready flip-flops and COM2502 ready/error state;
  - DATA IN/OUT perform their independent Rev0 handshake resets.
- `src/machine/io_devices.rs`
  - resolves revision-sensitive ready sources;
  - exposes explicit RIN/ROT pulse entry points.
- `src/machine/mod.rs`
  - applies D0/D1 software enables;
  - routes IN/OUT to PINT or raw VI lines.
- `src/config/sio.rs`
  - stores physical `SioInterruptWiring` inside `SioHardwareConfig`.
- `src/backend/*`
  - carries the same card state through Fast and Cycle.

## 9. Fast versus Cycle

Both engines use the same 88-SIO physical card state and routing configuration.

Fast may service a newly asserted PINT at an instruction boundary because its CPU execution engine is instruction-level. Cycle sees the same chassis interrupt line at exact CPU T-state boundaries. This execution granularity difference does not change the source flip-flops, software enables or physical routing.

RIN/ROT may be pulsed while the CPU is stopped; they belong to the external card/device boundary rather than to guest instruction execution.

## 10. Regression coverage

Coverage includes:

- Rev0 RIN -> input-ready latch -> enabled PINT path;
- DATA IN clears the Rev0 input request;
- Rev0 ROT -> output-ready latch -> raw VI path;
- DATA OUT clears the Rev0 output request;
- COM2502 RDA/TBMT alone cannot fabricate Rev0 RIN/ROT interrupt state;
- Rev1 uses internal RDA/TBMT readiness;
- VI requests remain raw and never masquerade as direct PINT;
- Fast and Cycle expose the same physical wiring and ready state.

Relevant tests include `tests/sio88_hardware_fidelity.rs`, `tests/sio88_interrupt_configuration.rs`, `tests/sio88_physical_boundary.rs` and machine unit tests.

## 11. User validation

### Rev0 external-ready path

1. POWER OFF and select MITS 88-SIO Rev0.
2. Route input to PINT and output to a VI level.
3. POWER ON and have guest software enable D0/D1 as desired.
4. A normal received byte may set D5/RDA but must not by itself assert the external-ready interrupt source.
5. An explicit RIN event must set the input-ready latch / active-low D0 source.
6. DATA IN must clear both RDA and that input-ready latch.
7. An explicit ROT event must set the output-ready latch / active-low D7 source.
8. DATA OUT must clear that output-ready latch independently of TBMT.

### Direct PINT versus VI

1. Route an active source to VI3: the raw VI3 request may assert, but the CPU must not take a direct restart merely because VI3 is high.
2. POWER OFF and route the same source to PINT.
3. Repeat with CPU interrupts enabled: the direct PINT path may now be accepted through the existing Altair interrupt-acknowledge mechanism.

A regression exists if VI selection fabricates a CPU vector or if Rev0 RDA/TBMT silently substitutes for RIN/ROT.

## 12. Known limits

- RusTair models the digital interrupt/handshake network, not analog pulse width, propagation delay or electrical noise.
- A complete 88-VI board is outside this 88-SIO item; VI lines end at the raw chassis boundary.
- Endpoint cables do not invent RIN/ROT. If a future modeled peripheral has documented ready contacts, those pulses must be added explicitly at that peripheral boundary.

No remaining implementation blocker is known inside the 88-SIO interrupt-routing claim. The item remains labelled “ready for final local validation” until the final post-endpoint `cargo test` checkpoint is green.

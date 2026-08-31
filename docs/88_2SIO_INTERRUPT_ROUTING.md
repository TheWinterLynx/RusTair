# MITS 88-2SIO interrupt routing

Status: **IN PROGRESS — physical routing types implemented and documented; machine/backend/persistence/UI application still pending.**

Parent hardware document: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope

This document covers the wiring between the two MC6850 IRQ outputs on a MITS 88-2SIO and the Altair interrupt system.

It does **not** redefine MC6850 interrupt conditions. RDRF/DCD/TX interrupt enable and status-bit behavior remain owned by `src/mc6850.rs`. This block answers the next physical question: once one ACIA is requesting service, where is that electrical request wire actually connected?

The 88-2SIO manual permits three installation classes:

1. no interrupt connection;
2. single-level interrupt through the Altair `PINT` line;
3. one of eight vector-interrupt levels through a separate MITS 88-Vector Interrupt system.

Port 0 and Port 1 are independent sources. MITS names their board pads `DI` and `EI` respectively.

## 2. Primary MITS evidence

Primary source:

**MITS, _Altair 88-2-SIO Documentation_, reprinted March 1977.**

Archive:

https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

### 2.1 Assembly Manual page 2-19 — three interrupt choices

The interrupt section states that the board can provide:

- interrupt at eight levels via the 88-Vector Interrupt;
- one level via the interrupt request line provided on the 88-2SIO;
- no interrupt at all.

It explicitly says any one of those three options may be implemented.

For the single-level system it instructs the builder to choose `DI`, `EI`, or the desired request leads and connect them to the pad marked `PINT`. It identifies:

- `DI` = Port 0;
- `EI` = Port 1.

The wording “chosen interrupt request line or lines” means wiring both ports to PINT is physically valid, but wiring only one port is equally valid.

### 2.2 Processor consequence of PINT

The same section warns that the processor can handle only one direct interrupt signal and that if the 88-2SIO single-level PINT wiring is used, another board cannot also be hard-wired directly to the processor interrupt input.

RusTair therefore must not treat “ACIA IRQ active” and “CPU PINT active” as synonyms. The former is chip state; the latter depends on board wiring.

### 2.3 88-Vector Interrupt is a separate system component

The PCB/schematic exposes `VI0` through `VI7` alongside `PINT`. Wiring DI/EI to `VIx` does not mean the 88-2SIO itself invents an 8080 restart opcode. The separate 88-VI hardware owns vector arbitration/opcode presentation during interrupt acknowledge.

RusTair will model the 88-2SIO boundary faithfully even before an 88-VI card exists in the chassis model:

- the 2SIO can expose which VI level is electrically requested;
- only an installed 88-VI model may turn those requests into the processor interrupt/vector behavior.

This prevents a future 88-VI implementation from having to undo a fake direct-RST shortcut inside the serial card.

## 3. Current pre-closeout behavior and why it is insufficient

Before this audit, `IoDevices::interrupt_request()` for the 88-2SIO effectively used:

```rust
self.two_sio.iter().any(TwoSioPort::interrupt_request)
```

and `AltairBus::refresh_interrupt_request_line()` projected that aggregate directly to the shared S-100 interrupt state.

That corresponds to exactly one possible physical installation: **both DI and EI hard-wired to PINT**.

It cannot reproduce:

- no interrupt wiring;
- Port 0 only to PINT;
- Port 1 only to PINT;
- DI/EI sent to VI0..VI7 instead of PINT;
- different VI levels for the two ports.

The current aggregate behavior is retained temporarily until the new routing type is connected through the machine/backend/configuration layers. This document therefore remains IN PROGRESS.

## 4. RusTair physical topology types

`src/config/two_sio.rs` now defines the destination of one physical IRQ wire:

```rust
pub enum TwoSioInterruptTarget {
    Disconnected,
    Pint,
    Vi0,
    Vi1,
    Vi2,
    Vi3,
    Vi4,
    Vi5,
    Vi6,
    Vi7,
}
```

The model deliberately uses explicit `Vi0`..`Vi7` variants rather than an arbitrary integer. Only the eight physical vector lines printed/exposed by the MITS system are representable.

Two independent wires are then represented by:

```rust
pub struct TwoSioInterruptWiring {
    pub port0: TwoSioInterruptTarget,
    pub port1: TwoSioInterruptTarget,
}
```

The mapping is literal:

| RusTair field | MITS board signal | Source |
| --- | --- | --- |
| `port0` | `DI` | Port 0 MC6850 IRQ |
| `port1` | `EI` | Port 1 MC6850 IRQ |

## 5. Why interrupt wiring is not inside `TwoSioStraps`

`TwoSioStraps` remains the address/baud block because MITS describes A2-A7 and baud selection as the board's hardware-select options in the Theory of Operation.

The interrupt assembly procedure is a separate signal-interconnect operation: DI/EI are wired to PINT or VIx pads. RusTair keeps that distinction visible instead of turning every soldered wire into one generic “strap settings” bag.

This also lets documentation and UI explain what physically changes:

- address jumpers decide which I/O block decodes;
- baud wiring decides the ACIA clock source;
- interrupt wiring decides where an already-generated ACIA IRQ travels.

## 6. Default and backwards compatibility

`TwoSioInterruptTarget::default()` is `Pint`, and `TwoSioInterruptWiring::default()` therefore represents:

```text
DI -> PINT
EI -> PINT
```

This is **not** claimed as the unique MITS factory/default installation. It is a migration default chosen to preserve RusTair's pre-audit behavior while the physical choice becomes explicit.

Once persistence/UI are connected, users can reproduce the other valid installations instead of being silently locked to both-PINT.

## 7. Vector boundary

Each target exposes two mutually exclusive interpretations:

```rust
pub const fn drives_pint(self) -> bool
pub const fn vector_level(self) -> Option<u8>
```

Required invariant:

- `Pint` drives PINT and has no VI level;
- `Vi0`..`Vi7` expose exactly one VI level and do not drive PINT;
- `Disconnected` drives neither.

An ACIA routed to `VI3` must therefore be able to assert the board's VI3 output while the processor PINT line stays unchanged unless an 88-VI board is installed and arbitrates that request.

## 8. Fast versus Cycle Accurate

The routing itself is static chassis wiring and is backend-independent.

### Fast

Fast may service a direct PINT interrupt at an instruction boundary, but only if the selected ACIA IRQ is physically wired to PINT.

A VIx-routed request must not be converted directly into the existing Fast `RST 7`/`FFh` path.

### Cycle Accurate

Cycle must sample the same routed PINT level on the shared interrupt control line. VIx requests remain separate board/chassis signals until an 88-VI component consumes them.

The two engines must therefore agree on routing even though their CPU timing models differ.

## 9. Phase-1 regression evidence

`src/config/two_sio.rs` contains:

### `interrupt_wiring_models_disconnected_pint_and_all_eight_vi_levels`

Protects:

- exactly the ten legal destinations: disconnected, PINT, VI0..VI7;
- PINT is not also a vector level;
- VI0..VI7 map to levels 0..7 exactly;
- vector targets do not silently drive PINT.

### `interrupt_wiring_is_independent_for_di_and_ei`

Protects the physical independence of Port 0/DI and Port 1/EI.

### `interrupt_wiring_default_preserves_previous_pint_projection`

Protects the migration default of both ports wired to PINT until the user selects another installation.

## 10. Next implementation phase

Before this block can become PASS, RusTair still has to connect the topology to actual machine behavior:

1. store `TwoSioInterruptWiring` as machine configuration independent of A2-A7/baud straps;
2. expose it through Fast and Cycle backends;
3. route only PINT-selected ACIA IRQs to the processor interrupt line;
4. expose active VI0..VI7 requests as a separate chassis signal/boundary for a future 88-VI board;
5. persist both DI and EI destinations;
6. expose POWER-OFF-only UI controls, because these are physical jumper/solder changes;
7. add focused regressions for None / Port0 PINT / Port1 PINT / both PINT / VI routing;
8. document user-visible manual validation;
9. run the full local suite.

## 11. User validation procedure once phase 2 is wired

The intended manual test is:

1. POWER OFF and select MITS 88-2SIO.
2. Set DI/Port 0 to **Disconnected**, EI/Port 1 to **Disconnected**.
3. POWER ON, enable an MC6850 receive interrupt and create RDRF on Port 0. Status bit 7 must show the ACIA IRQ condition, but CPU/PINT must remain inactive.
4. POWER OFF and change only DI to **PINT**.
5. Repeat the same Port 0 condition. CPU/PINT must now assert.
6. Clear Port 0 and generate an IRQ on Port 1. With EI disconnected, CPU/PINT must remain inactive.
7. POWER OFF and set EI to **PINT**. The same Port 1 IRQ must now reach CPU/PINT.
8. POWER OFF and route DI to **VI3**. Recreate the Port 0 IRQ. The 88-2SIO must report VI3 active while direct PINT remains inactive.
9. Fast and Cycle must show the same routing result.

Until an 88-VI board is modeled, step 8 stops at the chassis VI3 boundary; RusTair must not fabricate a CPU vector beyond that boundary.

## 12. Primary references

### MITS 88-2SIO

MITS, _Altair 88-2-SIO Documentation_, reprinted March 1977.

https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

Relevant material:

- Assembly Manual page 2-19, **INTERRUPT**: three supported routing classes; DI=Port 0, EI=Port 1; selected request line or lines wired to PINT;
- schematic around printed page 1-11: DI/EI, VI0..VI7 and PINT board signals;
- MC6850 control/status pages 1-6 through 1-8: chip-level interrupt enable and IRQ status behavior.

### Motorola MC6850

Motorola Semiconductor Products Inc., _MC6800 Microcomputer System Design Data_, 1976, MC6850 section.

https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf

Used only for the conditions under which each ACIA raises IRQ. The MITS manual remains authoritative for how that IRQ output is wired into the Altair system.

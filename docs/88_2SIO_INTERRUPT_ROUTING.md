# MITS 88-2SIO interrupt routing

Status: **IN PROGRESS — physical routing types and machine-level PINT/VI routing implemented; backend/persistence/UI application still pending.**

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

RusTair therefore does not treat “ACIA IRQ active” and “CPU PINT active” as synonyms. The former is chip state; the latter depends on board wiring.

### 2.3 88-Vector Interrupt is a separate system component

The PCB/schematic exposes `VI0` through `VI7` alongside `PINT`. Wiring DI/EI to `VIx` does not mean the 88-2SIO itself invents an 8080 restart opcode. The separate 88-VI hardware owns vector arbitration/opcode presentation during interrupt acknowledge.

RusTair models the 88-2SIO boundary even before an 88-VI card exists in the chassis model:

- the 2SIO exposes which VI levels are electrically requested;
- only an installed 88-VI model may turn those requests into processor interrupt/vector behavior.

This prevents a future 88-VI implementation from having to undo a fake direct-RST shortcut inside the serial card.

## 3. Pre-audit behavior and correction

Before this audit, `IoDevices::interrupt_request()` for the 88-2SIO effectively used:

```rust
self.two_sio.iter().any(TwoSioPort::interrupt_request)
```

and `AltairBus::refresh_interrupt_request_line()` projected that aggregate directly to the shared S-100 interrupt state.

That corresponded to exactly one possible physical installation: **both DI and EI hard-wired to PINT**.

The machine layer now separates three stages:

```text
MC6850 IRQ state
      ↓
DI / EI board output
      ↓
physical TwoSioInterruptTarget
      ├─ Disconnected → no system interrupt line
      ├─ PINT         → shared processor interrupt request
      └─ VI0..VI7     → separate vector-request line mask
```

Therefore an MC6850 may have status bit 7/IRQ active while processor PINT remains inactive.

## 4. RusTair physical topology types

`src/config/two_sio.rs` defines the destination of one physical IRQ wire:

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

Two independent wires are represented by:

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

Existing tests that expected either ACIA to reach PINT therefore remain valid under the default wiring.

## 7. Machine implementation

`src/machine/io_devices.rs` owns both raw ACIA IRQ state and the physical wiring.

The direct PINT request is now selected per source:

```rust
fn two_sio_pint_request(&self) -> bool {
    [0usize, 1].into_iter().any(|index| {
        self.two_sio_interrupt_wiring
            .target(index)
            .map_or(false, TwoSioInterruptTarget::drives_pint)
            && self.two_sio_irq(index)
    })
}
```

`IoDevices::interrupt_request()` uses that routed value for the 88-2SIO rather than OR-ing the two ACIAs unconditionally.

The vector side is independently projected as an eight-bit physical line mask:

```rust
pub(super) fn vector_interrupt_requests(&self) -> u8
```

Bit `n` means `VIn` is being driven by an active 88-2SIO ACIA IRQ. A VI bit does not assert PINT by itself.

`AltairBus::two_sio_vector_interrupt_requests()` exposes this board/chassis boundary for a future 88-VI component. It intentionally does not return an 8080 opcode.

## 8. Vector boundary

Each target exposes two mutually exclusive interpretations:

```rust
pub const fn drives_pint(self) -> bool
pub const fn vector_level(self) -> Option<u8>
```

Required invariant:

- `Pint` drives PINT and has no VI level;
- `Vi0`..`Vi7` expose exactly one VI level and do not drive PINT;
- `Disconnected` drives neither.

An ACIA routed to `VI3` can therefore assert the board's VI3 output while the processor PINT line stays unchanged. This remains true until an 88-VI board is modeled and explicitly consumes/arbitrates that line.

## 9. Fast versus Cycle Accurate

The routing itself is static chassis wiring and is backend-independent.

### Fast

Fast may service a direct PINT interrupt at an instruction boundary, but only if the selected ACIA IRQ is physically wired to PINT.

A VIx-routed request is excluded from the existing direct `FFh` interrupt opcode path.

### Cycle Accurate

Cycle samples the same routed PINT level on the shared interrupt control line because `AltairBus::refresh_interrupt_request_line()` now sees the routed result. VIx requests remain separate board/chassis signals until an 88-VI component consumes them.

Backend configuration APIs still need to expose the physical wiring before this sub-block can be considered complete from the application surface.

## 10. Regression evidence

### `src/config/two_sio.rs`

`interrupt_wiring_models_disconnected_pint_and_all_eight_vi_levels`

- exactly ten legal destinations: disconnected, PINT, VI0..VI7;
- PINT is not also a vector level;
- VI0..VI7 map to levels 0..7 exactly;
- vector targets do not silently drive PINT.

`interrupt_wiring_is_independent_for_di_and_ei`

- protects the physical independence of Port 0/DI and Port 1/EI.

`interrupt_wiring_default_preserves_previous_pint_projection`

- protects the migration default of both ports wired to PINT.

### `src/machine/io_devices.rs`

`two_sio_irq_is_routed_after_the_acia_not_fabricated_as_pint`

- creates a real MC6850 RX IRQ first;
- proves its chip IRQ/status remains active with DI disconnected;
- proves disconnected DI produces neither PINT nor VI;
- reroutes the same live IRQ to VI3 and proves PINT remains low while VI3 rises;
- reroutes it to PINT and proves the processor request rises.

`di_and_ei_route_independently_and_vi_levels_are_combined_as_lines`

- routes Port 0/DI to VI3 and Port 1/EI to PINT;
- proves Port 0 cannot accidentally drive PINT;
- proves Port 1 independently can;
- reroutes both to different VI levels and verifies the physical VI mask contains both lines without asserting PINT.

Existing DCD/RX/TX tests continue to protect the migration default where both DI and EI are PINT-connected.

## 11. Remaining implementation before PASS

The electrical machine layer is implemented. Remaining application/configuration work is deliberately above that layer:

1. store `TwoSioInterruptWiring` in `MachineConfig` independently of A2-A7/baud straps;
2. expose configure/query through Fast and Cycle `MachineBackend` implementations;
3. reapply wiring when an engine is recreated;
4. persist both DI and EI destinations;
5. expose POWER-OFF-only UI controls;
6. add backend/persistence/UI regressions;
7. run the manual validation below and the full local suite;
8. update the parent 88-2SIO closeout document and mark this block PASS.

The 88-VI card itself is **not** a prerequisite for this board-level routing block. The honest 88-2SIO boundary is the VI0..VI7 line mask; CPU vector generation belongs to the future 88-VI implementation.

## 12. User validation procedure once application wiring is exposed

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

## 13. Validation history

- Address/baud straps and the readdressed authentic loader are separately PASS as of 2026-08-31.
- Interrupt routing topology types were added after that green checkpoint.
- Machine-level routed PINT plus separate VI0..VI7 mask were then implemented in commit `4f7464568954ee2917e0c6487e31a2746335c2a9`.
- These new interrupt-routing commits require local focused/full validation before backend/persistence/UI work is stacked on top.
- GitHub Actions were not run.

## 14. Primary references

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

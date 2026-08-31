# MITS 88-2SIO interrupt routing

Status: **IMPLEMENTED — machine routing was locally green; backend/config/persistence/UI phase awaits the next local validation checkpoint.**

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

`MachineConfig` therefore owns two independent physical values:

```rust
pub two_sio_straps: TwoSioStraps,
pub two_sio_interrupt_wiring: TwoSioInterruptWiring,
```

Changing DI/EI never silently changes A2-A7 or either baud tap.

## 6. Default and backwards compatibility

`TwoSioInterruptTarget::default()` is `Pint`, and `TwoSioInterruptWiring::default()` therefore represents:

```text
DI -> PINT
EI -> PINT
```

This is **not** claimed as the unique MITS factory/default installation. It is a migration default chosen to preserve RusTair's pre-audit behavior while the physical choice becomes explicit.

Configuration files written before interrupt wiring became explicit contain no DI/EI keys. They therefore load with both ports at PINT and retain the behavior they had before this fidelity closeout.

An unknown edited value does not become a guessed vector. The parser keeps the safe migration default for that field.

## 7. Machine implementation

`src/machine/io_devices.rs` owns both raw ACIA IRQ state and the physical wiring.

The direct PINT request is selected per source:

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

## 9. Backend contract and Fast/Cycle behavior

The routing itself is static chassis wiring and is backend-independent. `MachineBackend` exposes:

```rust
fn configure_two_sio_interrupt_wiring(
    &mut self,
    wiring: TwoSioInterruptWiring,
) -> BackendResult<()>;
fn two_sio_interrupt_wiring(&mut self) -> BackendResult<TwoSioInterruptWiring>;
fn two_sio_vector_interrupt_requests(&mut self) -> BackendResult<u8>;
```

`BackendHost` exposes the same operations to the application without leaking either internal machine implementation.

### Fast

Fast stores the physical wiring in the same `AltairBus`/`IoDevices` hardware model. Direct PINT service still occurs only at the Fast CPU's interrupt boundary, but an IRQ reaches that path only if its DI/EI wire targets PINT.

A VIx-routed request is excluded from the direct `FFh` interrupt opcode path. Querying the VI mask returns only the raw board/chassis line state.

### Cycle Accurate

Cycle uses the same physical bus state. A wiring change is routed through `CycleHostBackend`, which owns the processor reset boundary rather than teaching the CPU-independent `AltairChassis` about an 8080 implementation.

The exact core samples the routed PINT level on the shared interrupt control line. VIx requests remain separate board/chassis signals until an 88-VI component consumes them.

### Engine replacement

Fast and Cycle are two engines around one configured physical machine. When the application recreates an engine with POWER OFF, it reapplies in order:

```rust
self.machine.configure_serial_board(...);
self.machine.configure_two_sio_straps(...);
self.machine.configure_two_sio_interrupt_wiring(...);
```

Switching engines therefore cannot silently restore DI/EI to PINT or lose a selected VI level.

## 10. Persistence

Persistent configuration version 3 adds two independent keys:

```text
machine.two_sio_port0_irq=pint
machine.two_sio_port1_irq=pint
```

Legal values are:

```text
disconnected
pint
vi0
vi1
vi2
vi3
vi4
vi5
vi6
vi7
```

`none` is accepted as a read-only compatibility alias for `disconnected`; saves normalize it to `disconnected`.

Old files lacking these keys retain `pint/pint`. Unknown values are ignored per field rather than being interpreted as a made-up restart vector.

Persistence reapplies the wiring to the backend after selecting the board and applying its address/baud straps.

## 11. User interface and observability

The normal user surface is:

**Configuration -> Serial board -> Physical 88-2SIO interrupt wiring**

It presents two independent selectors:

- `DI / Port 0 IRQ`
- `EI / Port 1 IRQ`

Each selector contains exactly:

- No interrupt connection
- PINT — single-level processor interrupt
- 88-VI level 0 (VI0) through level 7 (VI7)

The controls are disabled while POWER is ON. The UI explicitly says that DI/EI are physical request wires, not runtime software settings.

The same section also exposes the live raw vector boundary:

```text
Active raw 88-2SIO vector outputs: none
```

or, while requests are present:

```text
Active raw 88-2SIO vector outputs: VI3
Active raw 88-2SIO vector outputs: VI2, VI6
```

This is intentionally a **line-state display**, not a CPU vector display. The accompanying text states that selecting VIx never fabricates a CPU `RST` opcode inside the 88-2SIO.

## 12. Regression evidence

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

`interrupt_target_persistence_keys_round_trip_and_reject_unknown_values`

- round-trips all ten stable configuration names;
- protects the `none` read alias;
- rejects invented values such as `irq7`.

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

### Backend/config/persistence

`both_backends_preserve_interrupt_wiring_and_expose_vi_boundary`

- configures both Fast and Cycle with DI -> VI3 and EI disconnected;
- generates a real MC6850 receive IRQ through the debugger hardware boundary;
- requires both engines to expose the same VI3 raw line.

`two_sio_interrupt_wiring_is_machine_configuration_not_address_state`

- changes DI/EI independently;
- proves address/baud straps are not mutated as a side effect.

`persistent_text_round_trip_preserves_all_tunable_groups`

- now includes DI -> VI3 / EI -> disconnected in the full configuration round trip.

`old_or_invalid_interrupt_wiring_keeps_safe_migration_default`

- verifies old files load as PINT/PINT;
- verifies an invented `rst7` value cannot silently become a vector;
- verifies a valid VI target on the other port still loads independently.

### `tests/two_sio_interrupt_ui.rs`

Source-level architecture guards require:

- engine recreation to reapply the physical wiring;
- the application apply boundary to reject POWER-ON changes;
- independent DI and EI selectors using the complete physical target enum;
- explicit user text that VIx does not fabricate an 8080 RST;
- the live raw VI line display;
- both persistence keys and migration test to remain present.

## 13. How the user can validate it

These steps are for a normal RusTair user. Automated tests are not a substitute for this section.

### 13.1 Confirm the physical choices and POWER-OFF interlock

1. POWER OFF the Altair.
2. Open **Configuration -> Serial board** and select **MITS 88-2SIO**.
3. Find **Physical 88-2SIO interrupt wiring**.
4. Confirm there are separate selectors for **DI / Port 0 IRQ** and **EI / Port 1 IRQ**.
5. Confirm each offers disconnected, PINT and VI0..VI7.
6. POWER ON.
7. Reopen the same menu. Both selectors must be disabled and the UI must say POWER OFF is required.

A live DI/EI rewiring while POWER is ON is a regression.

### 13.2 ACIA IRQ versus disconnected system wiring

1. POWER OFF.
2. Set both DI and EI to **No interrupt connection**.
3. POWER ON.
4. Program Port 0 MC6850 receive interrupts on and create a receive condition that sets RDRF. The debugger/I/O Inspector status register must show the ACIA IRQ condition (status bit 7), because the chip itself is still requesting service.
5. Open **T-STATE TEACHER** or another bus/control view that exposes the processor interrupt request. PINT/CPU interrupt must remain inactive.
6. In the Serial board menu, **Active raw 88-2SIO vector outputs** must say `none`.

The important observation is `MC6850 IRQ = 1` while both downstream interrupt systems remain inactive.

### 13.3 Route DI to PINT

1. POWER OFF.
2. Change only **DI / Port 0 IRQ** to **PINT — single-level processor interrupt**. Leave EI disconnected.
3. POWER ON and recreate the same Port 0 receive IRQ.
4. The processor/PINT request must now assert.
5. Clear/read the MC6850 condition as appropriate; PINT must release with the underlying ACIA IRQ.
6. Generate an IRQ only on Port 1. Because EI is still disconnected, it must not reach PINT.

This proves DI and EI are independent physical wires.

### 13.4 Route DI to VI3 without fabricating a CPU vector

1. POWER OFF.
2. Set DI / Port 0 IRQ to **88-VI level 3 (VI3)** and leave EI disconnected.
3. POWER ON and recreate the Port 0 receive IRQ.
4. MC6850 status bit 7 must indicate IRQ.
5. The direct processor/PINT request must remain inactive.
6. Open **Configuration -> Serial board** while the request remains active. The live line must read:

   ```text
   Active raw 88-2SIO vector outputs: VI3
   ```

7. There must be no claim that the CPU received RST 3, RST 7, or any other restart opcode. RusTair currently has no installed 88-VI board to perform that conversion.

This is the most direct validation of the board boundary.

### 13.5 Two vector lines simultaneously

1. POWER OFF.
2. Route DI to VI2 and EI to VI6.
3. POWER ON and create active IRQ conditions on both ACIAs.
4. The menu must report:

   ```text
   Active raw 88-2SIO vector outputs: VI2, VI6
   ```

5. Direct PINT must remain inactive.

If the two requests collapse into one arbitrary CPU vector, the 88-2SIO/88-VI boundary has regressed.

### 13.6 Fast/Cycle preservation

1. POWER OFF and choose a distinctive wiring such as DI -> VI3, EI -> disconnected.
2. Switch to **Fast 8080** and verify the selectors retain those values.
3. POWER OFF if necessary, switch to **Cycle Accurate 8080**, and verify them again.
4. Repeat the VI3 test in both engines. The raw line result and absence of direct PINT must agree.

Changing engines is not allowed to alter physical board wiring.

### 13.7 Persistence

1. POWER OFF.
2. Set DI -> VI5 and EI -> disconnected.
3. Close RusTair normally so configuration is saved.
4. Restart RusTair.
5. Return to **Configuration -> Serial board**.
6. The same DI/EI destinations must be restored.

For an older configuration file with no IRQ keys, the expected migration is DI -> PINT and EI -> PINT, preserving historical RusTair behavior rather than inventing a new installation.

## 14. Validation history

- Address/baud straps and the readdressed authentic loader are separately PASS as of the 2026-08-31 user session.
- Machine-level routed PINT plus separate VI0..VI7 mask were implemented through `418b684ef97f23e2d6ae95633f75ee8f18e614aa`.
- The first attempt to validate that checkpoint was interrupted by Windows commit-memory exhaustion (`os error 1455` / linker `LNK1102`) while a 4K game was running; it was not treated as a code failure.
- After the game was closed, the user reran the routing-focused checks and normal suite at normal Cargo parallelism and reported **all OK**. The electrical machine layer is therefore locally validated.
- Backend/config/persistence/UI commits after that green checkpoint require the next local validation before this interrupt-routing sub-block is marked PASS.
- GitHub Actions were not run.

## 15. Remaining before interrupt-routing PASS

1. Compile and run the new backend/config/persistence/UI focused regressions.
2. Run the normal full local test suite.
3. Spot-check the POWER-OFF selector behavior and, when convenient, one of the VI3 user procedures above.
4. Record the result here and in the parent 88-2SIO closeout ledger.

The 88-VI card itself is **not** a prerequisite for this board-level routing PASS. The faithful 88-2SIO boundary is the raw VI0..VI7 line state; vector arbitration and interrupt-acknowledge opcode generation belong to a future 88-VI hardware block.

## 16. Primary references

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

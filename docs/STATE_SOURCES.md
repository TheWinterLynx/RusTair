# RusTair runtime state authorities

This document distinguishes authoritative emulated state from derived observations,
presentation caches and remaining architectural duplication. A UI snapshot is allowed
to cache history, but it must never become an input that drives the emulated machine.

RusTair now has a single Adaptive Cycle execution engine. Full semantic windows and
exact Partial T-state execution are two execution strategies inside that one engine;
they do not own separate CPU, RAM, serial or chassis state.

## Authoritative runtime state

| Domain | Authority | Consumers |
| --- | --- | --- |
| Intel 8080 architectural state | `CycleAccurateMachineBackend::cpu` (`Cpu8080Cycle`) | Adaptive Cycle execution, debugger CPU snapshots, exact pin samples, Full semantic windows |
| Physical machine container | CPU-free `AltairChassis` | live `AltairBus`, power/RUN/front-panel state |
| Physical S-100 card inventory and RAM contents | `S100RuntimeFabric` and its installed runtime card instances, reached through `machine::memory::Memory` | CPU-board bus cycles, debugger peek/write, RAM Viewer, Full and Partial execution |
| Raw S-100 electrical/status state | `machine::panel_bus::S100BusState::signals` | front panel, CPU control inputs, Bus Teacher RAW state |
| Front-panel switch register/address-control state | `FrontPanelController` plus the live S-100 bus where appropriate | physical panel controls and CPU-board injection paths |
| UART / serial-board runtime state | installed serial card instance in the live S-100 runtime fabric | guest I/O, ASR/terminal/TCP/COM endpoints, I/O Inspector |

## CPU ownership and chassis composition

The unified runtime has one processor authority and one physical chassis:

```text
Adaptive Cycle backend
├── Cpu8080Cycle                 authoritative 8080 architectural state
└── AltairChassis                CPU-free physical machine container
    └── AltairBus
        └── live S-100 runtime fabric
            ├── CPU-board electrical boundary
            ├── RAM cards
            ├── serial cards
            └── front-panel / bus-visible state
```

`AltairChassis` is intentionally CPU-free. It owns only physical chassis/S-100 state;
operations that require processor state are completed by the backend-owned
`Cpu8080Cycle` core.

- Power-on creates the undefined register/INTE sample directly for `Cpu8080Cycle` and
  supplies only bus-visible values to `AltairChassis`.
- RESET asserts the physical chassis/S-100 reset path and clocks the same reset into
  the authoritative `Cpu8080Cycle` core.
- RUN/STOP, HOLD/HLDA, EXAMINE, DEPOSIT and PROTECT use the CPU-free chassis plus the
  exact Cycle core.
- Physical S-100 reconfiguration remounts the live card inventory; it does not replace
  an execution engine or create a second processor object.
- Memory configuration reaches the live chassis bus/runtime fabric rather than a
  processor-owning machine helper.
- There is no `sync_machine_cpu()` path and no architectural-state copy between
  separate CPU implementations.
- Full execution uses the semantic facilities of the same 8080 core as an internal
  acceleration strategy and commits back into that same authoritative core. It is not
  a second backend.

Therefore processor ownership is unambiguous:

```text
8080 architectural state -> CycleAccurateMachineBackend::cpu (Cpu8080Cycle)
physical machine state   -> AltairChassis -> AltairBus -> live S-100 runtime fabric
```

## Deliberately derived state

These objects are observations. They must never be read back to determine guest or
raw electrical behaviour:

- `PanelLampIntegrator` / `PanelLampSnapshot`: optical/presentation persistence only.
- `BusTeachingSnapshot`: selected/latest teaching observation. `EXACT` samples are
  captured from the real Cycle T-state plus canonical raw S-100 state; `CONTROL
  STATE` is a non-ticking lifecycle observation.
- Instruction history, I/O history and Memory Activity: bounded historical traces.
- Call-stack and loop inference: conservative interpretations of retained history.
- egui viewport state, including Freeze: presentation state only.
- persisted application configuration: desired/restored configuration, not a second
  live hardware register file.
- Adaptive Full/Partial metrics: execution observations only. They may report which
  strategy handled T-states but cannot drive CPU or hardware state.

## Rules enforced by the didactic debugger

1. RAW S-100 status must come from `S100BusState`, never from LED brightness.
2. `PanelLampSnapshot` may be displayed as `VISIBLE LED`, but cannot reconstruct RAW.
3. Exact Cycle teaching status is read back after the CPU-board sample updates the
   canonical S-100 bus; the backend owns no parallel teaching status latch.
4. RAM Viewer/debugger access the same physical RAM/card storage used by guest
   execution.
5. App UI consumes backend contracts rather than concrete CPU/bus implementations.
6. Historical snapshots may be stale by design and must be labelled as history or a
   frozen sample; they cannot drive execution.
7. The backend must own exactly one `Cpu8080Cycle` plus one CPU-free `AltairChassis`;
   no second architectural processor state may be introduced.
8. Full and Partial may differ in execution granularity, but every transition between
   them must preserve the same CPU T-state count and the same physical hardware state.

## Resolved structural debt

- **CPU/chassis type composition is resolved.** Adaptive Cycle owns one
  `Cpu8080Cycle` and one CPU-free `AltairChassis`. Architectural regression tests
  guard against reintroducing a second backend, processor-owning machine container,
  dormant CPU, implicit chassis wrapper or mirror synchronization path.
- **Execution-engine duplication is resolved.** Full semantic execution is internal to
  Adaptive Cycle and cannot be selected as a separate engine. Exact Partial remains
  the synchronization path for electrically sensitive instructions and boundaries.
- The previous `AltairBus::cpu_inte` duplicate has already been removed: canonical
  INTE is stored in `S100BusState::signals.inte`.

## Remaining source-of-truth / structural debt

1. **RUN latch duplication.** `AltairChassis::running` and `S100Signals::run` are kept
   synchronized. The physical S-100 RUN latch should ultimately be the canonical
   storage location, with host-facing `running` derived from it.
2. **Backend encapsulation.** Concrete backend/chassis escape hatches still exist in
   parts of the codebase. The application should increasingly depend on common
   backend contracts and capabilities rather than concrete machine/CPU/bus types.
3. **Compatibility facades.** Some aggregate memory/configuration helpers remain for
   historical configuration and tests. They must not become alternate guest-visible
   RAM, UART or CPU state authorities.

# RusTair runtime state authorities

This document distinguishes authoritative emulated state from derived observations,
presentation caches and remaining architectural duplication. A UI snapshot is allowed
to cache history, but it must never become an input that drives the emulated machine.

## Authoritative runtime state

| Domain | Authority | Consumers |
| --- | --- | --- |
| Fast 8080 architectural state | `AltairMachine::cpu` (`Cpu8080`) | Fast backend snapshots, Fast execution |
| Cycle Accurate 8080 architectural state | `CycleAccurateMachineBackend::cpu` (`Cpu8080Cycle`) | Cycle execution, debugger CPU snapshots, exact pin samples |
| Fast physical machine container | `AltairMachine` | Fast CPU, shared `AltairBus`, power/RUN/front-panel state |
| Cycle physical chassis container | `AltairChassis` | shared `AltairBus`, power/RUN/front-panel state; contains no CPU core |
| Physical RAM contents / installed size / protection latches | `machine::memory::Memory` | Fast and Cycle guest access, debugger peek/write, RAM Viewer |
| Raw S-100 electrical/status state | `machine::panel_bus::S100BusState::signals` | front panel, CPU control inputs, Bus Teacher RAW state |
| Front-panel switch register/address-control state | `FrontPanelController` plus the live S-100 bus where appropriate | physical panel controls and CPU-board injection paths |
| UART / serial board runtime state | `IoDevices` / installed serial device model | guest I/O, ASR/terminal/TCP/COM endpoints, I/O Inspector |

## CPU ownership and chassis composition

The two Rust engines now have physically separate processor ownership as well as
separate processor types:

```text
Fast backend
└── AltairMachine
    ├── Cpu8080
    ├── AltairBus
    └── Fast physical state

Cycle backend
├── Cpu8080Cycle
└── AltairChassis
    ├── AltairBus
    ├── powered
    ├── running
    └── run/stop switch latches
```

`AltairChassis` is intentionally CPU-free. It owns only physical chassis/S-100 state;
operations that require processor state receive that information from the backend or
are completed by the backend-owned CPU core.

- Cycle power-on creates the undefined register/INTE sample directly for
  `Cpu8080Cycle` and supplies only bus-visible values to `AltairChassis`.
- Cycle RESET asserts the physical chassis/S-100 reset path and clocks the same reset
  into the authoritative `Cpu8080Cycle` core.
- RUN/STOP, HOLD/HLDA, EXAMINE, DEPOSIT and PROTECT use the CPU-free chassis plus the
  exact Cycle core. No Fast CPU object participates.
- Serial-card replacement is a chassis reconfiguration: the chassis drops RUN/READY,
  while the backend that owns the CPU performs the processor reset semantics.
- Cycle memory reconfiguration accesses the shared RAM/bus path directly instead of
  invoking Fast-oriented `AltairMachine` CPU helpers.
- There is no `sync_machine_cpu()` path and no architectural-state copy from
  `Cpu8080Cycle` into `Cpu8080`.
- Cycle imports and owns `AltairChassis` explicitly; it does not alias it to
  `AltairMachine` and does not carry an unused Fast CPU in its object graph.

Therefore the CPU authorities are both logically and physically unambiguous:

```text
Fast  -> AltairMachine::cpu (Cpu8080)
Cycle -> CycleAccurateMachineBackend::cpu (Cpu8080Cycle)
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

## Rules enforced by the didactic debugger

1. RAW S-100 status must come from `S100BusState`, never from LED brightness.
2. `PanelLampSnapshot` may be displayed as `VISIBLE LED`, but cannot reconstruct RAW.
3. Exact Cycle teaching status is read back after the CPU-board sample updates the
   canonical S-100 bus; the backend owns no parallel teaching status latch.
4. RAM Viewer/debugger access the same physical `Memory` object as both CPU engines.
5. App UI consumes backend contracts rather than concrete CPU/bus implementations.
6. Historical snapshots may be stale by design and must be labelled as history or a
   frozen sample; they cannot drive execution.
7. Cycle must own `AltairChassis` plus `Cpu8080Cycle`; it must not import or embed the
   Fast `AltairMachine` as its physical container.

## Resolved structural debt

- **CPU/chassis type composition is resolved.** Cycle physically owns a CPU-free
  `AltairChassis`; Fast alone retains `AltairMachine::cpu` as its real `Cpu8080`.
  Architectural regression tests guard against reintroducing the old alias, dormant
  Fast CPU, `Deref` wrapper or mirror synchronization path.
- The previous `AltairBus::cpu_inte` duplicate has already been removed: canonical
  INTE is stored in `S100BusState::signals.inte`.

## Remaining source-of-truth / structural debt

1. **RUN latch duplication.** `AltairMachine::running` / `AltairChassis::running` and
   `S100Signals::run` are kept synchronized. The physical S-100 RUN latch should
   ultimately be the canonical storage location, with host-facing `running` derived
   from it.
2. **Backend encapsulation.** Concrete backend/chassis escape hatches still exist in
   parts of the codebase. The application should increasingly depend on common
   backend contracts and capabilities rather than concrete machine/CPU/bus types.

# RusTair runtime state authorities

This document distinguishes authoritative emulated state from derived observations,
presentation caches and remaining architectural mirrors. A UI snapshot is allowed to
cache history, but it must never become an input that drives the emulated machine.

## Authoritative runtime state

| Domain | Authority | Consumers |
| --- | --- | --- |
| Fast 8080 architectural state | `AltairMachine::cpu` (`Cpu8080`) | Fast backend snapshots, Fast execution |
| Cycle Accurate 8080 architectural state | `CycleAccurateMachineBackend::cpu` (`Cpu8080Cycle`) | Cycle execution, debugger CPU snapshots, exact pin samples |
| Physical RAM contents / installed size / protection latches | `machine::memory::Memory` | Fast and Cycle guest access, debugger peek/write, RAM Viewer |
| Raw S-100 electrical/status state | `machine::panel_bus::S100BusState::signals` | front panel, CPU control inputs, Bus Teacher RAW state |
| Front-panel switch register/address-control state | `FrontPanelController` plus the live S-100 bus where appropriate | physical panel controls and CPU-board injection paths |
| UART / serial board runtime state | `IoDevices` / installed serial device model | guest I/O, ASR/terminal/TCP/COM endpoints, I/O Inspector |

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

## Remaining source-of-truth debt

The following pre-existing mirrors still prevent a strict claim that *every* runtime
bit exists in exactly one storage location:

1. **Cycle CPU mirror in `AltairMachine::cpu`.** Cycle Accurate execution is owned by
   `Cpu8080Cycle`, and public Cycle CPU snapshots read that core directly, but the
   common `AltairMachine` chassis still contains a passive `Cpu8080` mirror used by
   some legacy chassis helpers. Remove it by making the chassis CPU-core agnostic.
2. **RUN latch mirror.** `AltairMachine::running` and `S100Signals::run` are kept in
   sync. The physical S-100 RUN latch should ultimately be the canonical source.
3. **INTE mirror.** `AltairBus::cpu_inte` duplicates `S100Signals::inte` for the Fast
   CPU-board adapter. The adapter should read the canonical S-100 value instead.

Until these three items are removed, RusTair has a clear authoritative path for UI,
memory and S-100 teaching, but it should not claim a mathematically strict
single-storage-location architecture for all machine state.

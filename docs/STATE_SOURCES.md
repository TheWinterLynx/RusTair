# RusTair runtime state authorities

This document distinguishes authoritative emulated state from derived observations,
presentation caches and remaining architectural duplication. A UI snapshot is allowed
to cache history, but it must never become an input that drives the emulated machine.

## Authoritative runtime state

| Domain | Authority | Consumers |
| --- | --- | --- |
| Fast 8080 architectural state | `AltairMachine::cpu` (`Cpu8080`) | Fast backend snapshots, Fast execution |
| Cycle Accurate 8080 architectural state | `CycleAccurateMachineBackend::cpu` (`Cpu8080Cycle`) | Cycle execution, debugger CPU snapshots, exact pin samples |
| Physical RAM contents / installed size / protection latches | `machine::memory::Memory` | Fast and Cycle guest access, debugger peek/write, RAM Viewer |
| Raw S-100 electrical/status state | `machine::panel_bus::S100BusState::signals` | front panel, CPU control inputs, Bus Teacher RAW state |
| Front-panel switch register/address-control state | `FrontPanelController` plus the live S-100 bus where appropriate | physical panel controls and CPU-board injection paths |
| UART / serial board runtime state | `IoDevices` / installed serial device model | guest I/O, ASR/terminal/TCP/COM endpoints, I/O Inspector |

## CPU ownership after the Cycle mirror removal

`AltairMachine` still contains a `Cpu8080` because that object is the real processor of
the Fast backend. The same chassis container is currently embedded by the Cycle
backend, so that field is physically present there too, but **it is dormant in Cycle**:

- Cycle power-on generates its undefined register/INTE sample directly for
  `Cpu8080Cycle` and passes only the required electrical values to the chassis.
- Cycle RESET, RUN/STOP, HOLD/HLDA, EXAMINE, DEPOSIT, PROTECT and lamp integration do
  not consult or update `AltairMachine::cpu`.
- Cycle memory reconfiguration accesses the shared RAM/bus path directly instead of
  invoking the Fast-oriented `AltairMachine::configure_memory` helper.
- There is no `sync_machine_cpu()` path and no architectural-state copy from
  `Cpu8080Cycle` into `Cpu8080`.

Therefore the two Rust engines have independent and unambiguous CPU authorities:

```text
Fast  -> AltairMachine::cpu (Cpu8080)
Cycle -> CycleAccurateMachineBackend::cpu (Cpu8080Cycle)
```

A later structural refactor may split `AltairMachine` into a CPU-agnostic chassis type
plus the Fast CPU owner. That would remove the dormant field from the Cycle object
graph entirely, but it is no longer a duplicated runtime CPU state or a source of
truth used by Cycle.

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
7. Cycle code must not read or write `AltairMachine::cpu`; its only CPU authority is
   `Cpu8080Cycle`.

## Remaining source-of-truth / structural debt

1. **RUN latch duplication.** `AltairMachine::running` and `S100Signals::run` are kept
   synchronized. The physical S-100 RUN latch should ultimately be the canonical
   storage location, with host-facing `running` derived from it.
2. **CPU/chassis type composition.** The Fast CPU still lives inside `AltairMachine`,
   which means a Cycle backend carries an unused `Cpu8080` field as part of that
   shared container. This is structural baggage, not a Cycle CPU mirror. A future
   `AltairChassis` extraction can remove it without changing either CPU core.

The previous `AltairBus::cpu_inte` duplicate has already been removed: canonical INTE
is stored in `S100BusState::signals.inte`.

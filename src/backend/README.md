# Emulator backend boundary

RusTair has two selectable Intel 8080 engines:

1. `RustFast8080` — instruction-level Rust 8080/Altair implementation.
2. `RustCycleAccurate8080` — T-state/pin-accurate Rust 8080 implementation.

Changing engines creates a fresh machine. Live state transfer between engines is intentionally outside the selector contract.

## Active-backend authority

Multiple CPU implementations are deliberate. The invariant is **exactly one CPU authority for the active backend**.

| State | Fast authority | Cycle authority |
| --- | --- | --- |
| Registers / flags / PC / SP | `AltairMachine.cpu: Cpu8080` | `CycleAccurateMachineBackend.cpu: Cpu8080Cycle` |
| INTE / HALT | `AltairMachine.cpu` | `Cpu8080Cycle` |
| CPU T-state total | `AltairMachine.cpu.cycles` | `Cpu8080Cycle::total_t_states()` |
| Exact machine cycle / T-state / package pins | not available | `Cpu8080Cycle` |
| RAM / I/O / S-100 state | `AltairMachine.bus` | `AltairChassis.bus` |
| Visible lamp persistence | presentation integrator only | presentation integrator only |
| RUN/STOP physical state | `AltairMachine` | `AltairChassis` |

### Fast

Fast owns an `AltairMachine`, which contains its real `Cpu8080`, `AltairBus` and physical machine state. Execution, debugger state and `MachineBackend::cpu_state()` read that CPU directly.

### Cycle Accurate

Cycle owns two separate physical components:

```text
CycleAccurateMachineBackend
├── Cpu8080Cycle          CPU execution authority
└── AltairChassis         CPU-free physical chassis
    ├── AltairBus
    ├── powered
    ├── running
    └── RUN/STOP switch latches
```

`AltairChassis` deliberately contains no `Cpu8080`. There is no passive Fast CPU mirror, no alias that disguises the chassis as `AltairMachine`, and no `sync_machine_cpu()` path. CPU transitions remain inside `Cpu8080Cycle`; the chassis and S-100 bus observe or drive the physical signals required by those transitions.

RESET is a real cross-boundary operation: the chassis asserts the S-100 reset line and the Cycle backend clocks that line into `Cpu8080Cycle`. EXAMINE, DEPOSIT, RUN/STOP, HOLD/HLDA and memory protection likewise use the Cycle core plus the CPU-free chassis rather than a hidden Fast CPU.

Serial-board replacement is split by ownership. The chassis performs the physical board change and drops RUN/READY as required; the backend resets the Cycle CPU when a powered machine changes board. Selecting the already installed board is a no-op.

The current Cycle memory configuration path mutates RAM on the existing chassis/bus rather than constructing a replacement `CycleAccurateMachineBackend`. When powered, it stops execution and resets the existing Cycle core after the storage change.

## Chassis and observation

S-100 state is the electrical authority seen by the front panel. CPU INTE originates in the active CPU: Fast projects its instruction-level value through its CPU-board path; Cycle projects the exact value produced by `Cpu8080Cycle`. The Teacher observes those results and must never feed presentation state back into CPU state.

Teacher is view-only. Exact mode consumes Cycle CPU samples plus canonical RAW S-100 state. Fast-mode cycle/T-state teaching remains explicitly approximate. LED persistence is presentation-only in both modes.

### Temporal semantics of exact Teacher samples

`BusTeachingAccuracy::Exact` means **an exact captured T-state sample**, not a promise that every field still describes the chassis at the later instant when the UI renders it.

For an exact sample, machine-cycle, T-state, CPU output pins, S-100 status and READY/HOLD/RESET inputs describe the electrical levels associated with that CPU tick. Host/debugger controls may subsequently change chassis state without clocking another 8080 T-state. A debugger `Step T`, for example, must retain the READY level the CPU actually saw rather than retroactively rewriting that historical sample.

```text
captured CPU sample: immutable historical truth for one real T-state
current chassis state: mutable RUN/STOP / READY / HOLD / RESET control state
```

A physical STOP is different from a debugger pause. Cycle clocks the real next PSYNC, T2 and first TW before freezing execution, so the resulting exact sample genuinely contains READY low and CPU-generated WAIT high. Merely changing READY outside a CPU tick must never fabricate WAIT.

RESET is also different from a passive host pause: it changes the CPU itself, so asserting RESET invalidates a retained exact T-state sample and the Teacher falls back to an explicit control-state observation until another real CPU T-state is clocked.

## Error handling debt

`MachineBackend` operations are fallible, but several `BackendHost` convenience methods still convert a returned `BackendError` into `panic!`. With the two built-in Rust engines this is mostly dormant, but operations that can legitimately fail should eventually expose an explicit application error boundary rather than relying on panic-based transport.

## Regression guards

- `tests/backend_authority.rs` verifies Fast ownership, Cycle ownership of a CPU-free `AltairChassis`, exact chassis controls and Fast/Cycle architectural agreement on deterministic guest programs.
- `tests/chassis_architecture.rs` prevents a CPU, `Deref` or `DerefMut` wrapper from being reintroduced into `AltairChassis`.
- `tests/state_source_architecture.rs` guards explicit Cycle chassis naming and the documented state-source architecture.
- `tests/cpu8080_cycle_differential.rs` compares all 256 Intel 8080 opcode byte values between the two Rust CPU cores.
- `tests/no_simh.rs` ensures the product remains limited to the two Rust 8080 engines and that retired external-backend artifacts do not reappear.

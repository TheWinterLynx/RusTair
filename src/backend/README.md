# Emulator backend boundary

RusTair has two selectable Rust 8080 engines today, with SIMH engines behind the same product-level boundary:

1. `RustFast8080` — instruction-accurate Rust 8080/Altair implementation.
2. `RustCycleAccurate8080` — T-state/pin-accurate Rust 8080 implementation.
3. `SimhAltair` — Open SIMH `altair` target.
4. `SimhAltairZ80` — Open SIMH `altairz80` target.

Changing engines creates a fresh machine. Live state transfer between engines is intentionally outside the selector contract.

## Active-backend authority

Multiple CPU implementations are deliberate. The invariant is **exactly one CPU authority for the active backend**, not one CPU object for all of RusTair.

| State | Fast authority | Cycle authority |
| --- | --- | --- |
| Registers / flags / PC / SP | `AltairMachine.cpu: Cpu8080` | `CycleAccurateMachineBackend.cpu: Cpu8080Cycle` |
| INTE / HALT | `AltairMachine.cpu` | `Cpu8080Cycle` |
| CPU T-state total | `AltairMachine.cpu.cycles` | `Cpu8080Cycle::total_t_states()` |
| Exact machine cycle / T-state / package pins | not available | `Cpu8080Cycle` |
| RAM | shared `Memory` | shared `Memory` |
| RAW S-100 state | shared `S100BusState` | shared `S100BusState` |
| Visible lamp persistence | presentation integrator only | presentation integrator only |
| RUN/STOP latch | shared Altair chassis | shared Altair chassis |

### Fast

`AltairMachine.cpu` is the real instruction-level processor. Execution, debugger state and `MachineBackend::cpu_state()` may read and execute it directly.

### Cycle Accurate

`CycleAccurateMachineBackend.cpu` is the only CPU execution authority. `AltairMachine.cpu` remains as a passive compatibility mirror because `AltairMachine` still owns common chassis/front-panel helpers originally built around the Fast CPU.

After exact CPU transitions, `sync_machine_cpu()` copies architectural state in one direction:

```text
Cpu8080Cycle (authority) -> AltairMachine.cpu (passive mirror)
```

The Cycle backend must never execute that legacy `Cpu8080` or use stale mirror values as CPU truth. Power-on initialization is the intentional exception: the common chassis establishes the initial register image before the new Cycle core becomes authoritative.

## Chassis and observation

`AltairMachine.running` represents the physical RUN/STOP latch. S-100 RUN/READY levels are consequences of that chassis state, not another CPU authority.

CPU INTE likewise originates in the active CPU. Fast projects its instruction-level value through the bus adapter; Cycle projects the exact value produced by `Cpu8080Cycle`. S-100 and Teacher observe those results and must never feed panel presentation state back into CPU state.

Teacher is view-only. Exact mode consumes Cycle CPU samples plus canonical RAW S-100 state. Fast-mode cycle/T-state teaching remains explicitly approximate. LED persistence is presentation-only in both modes.

### Temporal semantics of exact Teacher samples

`BusTeachingAccuracy::Exact` means **an exact captured T-state sample**, not a promise that every field still describes the chassis at the later instant when the UI happens to render it.

For an exact sample, machine-cycle, T-state, CPU output pins, S-100 status and the READY/HOLD/RESET inputs describe the electrical levels associated with that CPU tick. Host/debugger controls may subsequently change the chassis without clocking another 8080 T-state. For example, debugger `Step T` clocks one exact T-state with READY released and then returns the host to pause; the captured sample must retain the READY level the CPU actually saw rather than being rewritten after the fact.

That distinction is intentional:

```text
captured CPU sample: immutable historical truth for one real T-state
current chassis state: mutable RUN/STOP / READY / HOLD / RESET control state
```

A physical STOP is different from a debugger pause. The Cycle backend clocks the real next PSYNC, T2 and first TW before freezing execution, so the resulting exact sample genuinely contains READY low and CPU-generated WAIT high. Merely changing READY outside a CPU tick must never fabricate a WAIT transition.

RESET is also different from a passive host pause: RESET changes the 8080 itself, so asserting it invalidates any retained exact T-state sample and the Teacher falls back to an explicit control-state observation until another real CPU T-state is clocked.

## Regression guards

`tests/backend_authority.rs` enforces the CPU/chassis authority contract at machine level. It verifies Fast ownership, deliberately desynchronizes the Cycle compatibility mirror to prove it cannot drive execution, checks mirror invariants across POWER/RESET/STEP/RUN/EXAMINE/DEPOSIT/HOLD/HLDA/HLT recovery, and runs the same program through Fast and Cycle while comparing architectural state, timing and memory after every instruction.

`tests/bus_teaching.rs` additionally guards the temporal observation contract: exact READY/HOLD/RESET inputs cannot be retroactively rewritten by later debugger/chassis changes, RESET replaces stale exact samples with control-state observations, and power-on INTE remains consistent between the Cycle CPU authority and canonical RAW S-100 projection.

This complements `tests/cpu8080_cycle_differential.rs`, which compares all 256 8080 opcodes directly between the two Rust CPU cores.
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

## Regression guards

`tests/backend_authority.rs` enforces this contract at machine level. It verifies Fast ownership, deliberately desynchronizes the Cycle compatibility mirror to prove it cannot drive execution, checks mirror invariants across POWER/RESET/STEP/RUN/EXAMINE/DEPOSIT/HOLD/HLDA/HLT recovery, and runs the same program through Fast and Cycle while comparing architectural state, timing and memory after every instruction.

This complements `tests/cpu8080_cycle_differential.rs`, which compares all 256 8080 opcodes directly between the two Rust CPU cores.

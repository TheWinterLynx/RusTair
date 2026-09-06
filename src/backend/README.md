# Emulator backend boundary

RusTair has one Intel 8080 execution engine: **Adaptive Cycle**.

`EmulationEngine::RustCycleAccurate8080` is the only public engine identity. The
engine may execute a proven whole-instruction **Full** window or fall back to the
edge-by-edge **Partial** electrical oracle, but those are internal strategies over
the same processor, chassis, RAM and serial-card state. They are not selectable
backends and no state is copied between them.

## Runtime ownership

The active machine has one processor authority and one CPU-free physical chassis:

```text
CycleHostBackend                    host scheduling/debugger facade
└── CycleAccurateMachineBackend     Adaptive Full/Partial dispatcher
    ├── Cpu8080Cycle                sole 8080 architectural-state authority
    └── AltairChassis               CPU-free physical machine container
        └── AltairBus
            └── live S-100 runtime fabric
                ├── MITS 8080 CPU-board electrical boundary
                ├── RAM cards
                ├── 88-SIO / 88-2SIO cards
                └── front-panel / bus-visible state
```

| State | Authority |
| --- | --- |
| Registers / flags / PC / SP | `CycleAccurateMachineBackend::cpu` (`Cpu8080Cycle`) |
| INTE / HALT / exact T-state count | `Cpu8080Cycle` |
| Exact machine cycle / T-state / package pins | `Cpu8080Cycle` |
| Physical chassis power and RUN/STOP state | `AltairChassis` |
| RAM / installed S-100 cards | live `S100RuntimeFabric` reached through `AltairBus` |
| UART state | installed/live serial-card instance; aggregate compatibility routing never owns a second guest-visible UART |
| Raw S-100 electrical/status state | canonical `S100BusState` |
| Visible LED persistence | presentation integrator only |

`AltairChassis` deliberately contains no processor implementation or register
mirror. There is no `AltairMachine`, Fast CPU mirror, `sync_machine_cpu()` path,
or engine-recreation boundary.

## Adaptive Full and Partial

Partial is the exact electrical oracle. Every real T-state drives the MITS CPU
board and live S-100 fabric. READY, HOLD/HLDA, RESET, interrupts, serial timing,
front-panel activity and card-visible bus edges are resolved there.

Full is permitted only when the dispatcher proves that no installed hardware can
distinguish the omitted intermediate host-side work from the corresponding exact
T-state sequence. It uses the semantic 8080 executor internally, commits into the
same `Cpu8080Cycle` architectural state and the same live S-100 storage, preserves
exact T-state totals and projects equivalent observable front-panel/card timing.
Any barrier returns execution to Partial.

The critical invariant is:

```text
Full or Partial -> same CPU state + same physical S-100 state + same elapsed T-states
```

## Independent card clocks

Serial-board baud generators are physical clocks independent of whether useful
8080 instructions are executing.

- During normal RUN, exact Partial T-states clock the installed card once per real
  CPU-board quantum; Full advances the equivalent elapsed card time at its
  synchronization boundary.
- When CPU execution cannot cover elapsed wall time because the machine is STOPped,
  RESET is held, or HLDA parks useful execution, `CycleHostBackend` bridges the
  uncovered host duration to equivalent hardware quanta.
- That host bridge subtracts CPU T-states already executed since the previous
  panel commit, preventing double counting across state transitions.
- `AltairChassis::cycle_commit_panel_activity` remains presentation-only; it must
  not become a second wall-clock source for UARTs.

The UART/ACIA state itself still lives only in the physical serial-card instance.
The host scheduler supplies elapsed time; it does not own or duplicate card state.

## Chassis controls and observation

RESET is a real cross-boundary operation: the chassis asserts the S-100 reset
line and the Cycle backend clocks that condition into the authoritative
`Cpu8080Cycle`. EXAMINE, DEPOSIT, RUN/STOP, HOLD/HLDA and memory protection likewise
combine the exact CPU core with the CPU-free chassis rather than consulting a
second processor implementation.

S-100 state is the electrical authority seen by the front panel. CPU INTE originates
in `Cpu8080Cycle` and is projected through the CPU-board path. The Bus Teacher is
view-only and must never feed presentation state back into the machine.

`BusTeachingAccuracy::Exact` means an exact captured T-state sample. A later host
control action may change current chassis state without rewriting that historical
sample. Optical lamp persistence is likewise derived presentation state and cannot
reconstruct RAW bus levels.

## Configuration boundaries

Physical S-100 reconfiguration remounts the live slot inventory. It does not
replace an execution engine or create another CPU object. Moving cards requires
POWER OFF at the public S-100 configuration boundary.

Legacy aggregate RAM/configuration fields exist only as migration or compatibility
inputs. Runtime guest execution must use the mounted S-100 inventory, not recreate
an alternate topology from those fields.

## Remaining structural debt

- `AltairChassis::running` and the S-100 RUN signal are still synchronized storage;
  eventually the physical bus latch should be the sole canonical value.
- `CycleHostBackend` still contains debugger/scheduling policy around the concrete
  Adaptive Cycle backend. That facade must remain policy-only and must not acquire
  duplicate CPU, RAM or UART state.
- Some aggregate configuration helpers remain for migration/tests. They must never
  become alternate guest-visible hardware authorities.

See `docs/STATE_SOURCES.md` for the broader state-source inventory.

## Regression guards

- `tests/unified_cycle_architecture.rs` prevents removed Fast/semantic-machine
  architecture from re-entering source or tests.
- `tests/state_source_architecture.rs` guards one `Cpu8080Cycle` authority plus the
  CPU-free `AltairChassis`.
- `tests/backend_authority.rs` compares Adaptive dispatch against a forced Partial
  oracle for the same exact T-state budget and physical state.
- `tests/two_sio_idle_chassis_clock.rs` guards STOP/RESET/HLDA/RUN serial-clock
  semantics and prevents panel presentation from becoming a second idle clock.
- `tests/s100_physical_serial_authority.rs` verifies that CPU I/O reaches the same
  installed serial-card instance used by endpoints/debugger access.

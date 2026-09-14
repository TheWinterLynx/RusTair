# Emulator backend boundary

RusTair has one Intel 8080 execution engine: **Adaptive Cycle**.

`EmulationEngine::RustCycleAccurate8080` is the only public engine identity. The
engine may execute a proven whole-instruction **Full** window or fall back to the
edge-by-edge **Partial** electrical oracle, but those are internal strategies over
the same processor, chassis, RAM and serial-card state. They are not selectable
backends. A Full window exports the boundary registers into a transient semantic
executor and commits them back once at window exit; no independent machine or
persistent CPU mirror is maintained.

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
| Physical chassis power | `AltairChassis` |
| RUN/STOP latch | `S100BusState::signals.run`; `AltairChassis::running()` derives from it |
| RAM / installed S-100 cards | live `S100RuntimeFabric` reached through `AltairBus` |
| UART/register/shift state and serial oscillator/divider phase | installed/live serial-card instance |
| Serial elapsed-time source | managed physical-time scheduler in throttled modes; automatic `Instant` source in Unlimited |
| CPU/chassis-time mechanics such as DCDD | chassis virtual-time path, separate from serial physical time |
| Raw S-100 electrical/status state | canonical `S100BusState` |
| Visible LED persistence | presentation integrator only |

`AltairChassis` deliberately contains no processor implementation or register
mirror. There is no `AltairMachine`, Fast CPU mirror, `sync_machine_cpu()` path,
or engine-recreation boundary.

## Adaptive Full and Partial

Partial is the exact electrical oracle. Every exact T-state drives the MITS CPU
board and live S-100 fabric. READY, HOLD/HLDA, RESET, interrupts, front-panel
activity and card-visible bus edges are resolved there.

Full is permitted only when the dispatcher proves that no installed hardware can
distinguish the omitted intermediate host-side work from the corresponding exact
T-state sequence. It uses the semantic 8080 executor internally, commits into the
same `Cpu8080Cycle` architectural state and the same live S-100 storage, preserves
exact T-state totals and projects equivalent observable front-panel/card timing.
Any barrier returns execution to Partial.

An independently clocked UART being active is not by itself a reason to force the
CPU into Partial. Serial physical time is settled at explicit scheduler boundaries;
guest serial I/O remains an exact synchronization barrier.

The critical invariant is:

```text
Full or Partial -> same CPU state + same physical S-100 state + same elapsed T-states
```

## Independent serial physical time

Serial-board baud generators are physical clocks independent of guest CPU execution
speed. CPU T-states do **not** directly advance the 88-SIO/88-2SIO oscillator.
Authentic/2x/5x/10x/Unlimited therefore never multiply or divide selected baud.

The current throttled scheduler is event-driven rather than based on the retired
fixed 4 microsecond slice:

- `execution_frame.rs` queries the installed cards for the earliest serial clock
  deadline before each normal CPU service interval.
- A quiet UART publishes no deadline. The CPU keeps the normal 4096-T-state Adaptive
  service slice while the card retains its physical oscillator phase.
- An active 88-SIO publishes the next effective UART bit/frame boundary.
- An active 88-2SIO retains its free-running external 16x baud-tap phase while the
  scheduler observes only the next effective MC6850 `/1`, `/16` or `/64` boundary.
- After each managed CPU interval the application supplies only the corresponding
  elapsed **serial physical time** to the installed card.
- A guest serial `OUT` is an exact causal barrier. Adaptive execution stops before
  the output; exact progression across the barrier remains inside the Cycle backend;
  the host settles pre-write elapsed serial time and then replans from the card's
  newly changed state. A newly active UART therefore cannot inherit time from before
  the guest write.
- `CycleHostBackend` may orchestrate the boundary, but it must never own a T-state
  loop or duplicate card state.

`Unlimited` has no stable CPU-T-state-to-wall-time ratio. It therefore uses the
backend's automatic `Instant` physical-time source. Switching between managed
throttled timing and Unlimited creates one explicit handoff boundary so elapsed
serial time is neither replayed twice nor dropped.

STOP, sustained RESET, HOLD/HLDA and HALT may stop useful CPU execution while serial
physical time continues without fabricating CPU T-states. GUI repaint cadence is
never the UART clock.

CPU/chassis-time devices such as the current DCDD/FD-400 mechanics remain on their
separate chassis virtual-time path. Serial physical time must never be reused as a
generic peripheral clock.

The UART/ACIA state itself still lives only in the installed physical serial-card
instance. The scheduler supplies elapsed time and asks the card for its next event;
it does not own or duplicate UART state.

See `docs/SERIAL_CLOCK_DOMAINS.md` for the complete current contract and performance
evidence.

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

Endpoint pacing and card baud/straps are independent configuration domains. The
application must not silently restrap a card to match an ASR, terminal, TCP or COM
endpoint. A deliberate mismatch is a valid configuration state even where the
current endpoint model does not claim analog corruption from that mismatch.

## Remaining structural debt

- RUN/STOP already derives from the physical bus latch; the former chassis-side
  RUN mirror is gone and must not be reintroduced.
- `CycleHostBackend` still contains debugger/scheduling policy around the concrete
  Adaptive Cycle backend. That facade must remain policy-only and must not acquire
  duplicate CPU, RAM, UART or exact T-state-loop authority.
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
- `tests/debugger_architecture.rs` prevents `CycleHostBackend` from redispatching a
  host service budget as its own per-T-state loop.
- `tests/two_sio_idle_chassis_clock.rs` guards STOP/RESET/HLDA/RUN serial-clock
  independence and prevents panel presentation from becoming a serial clock.
- `tests/s100_physical_serial_authority.rs` verifies that CPU I/O reaches the same
  installed serial-card instance used by endpoints/debugger access.
- `tests/serial_scheduler_benchmark.rs` provides manual release evidence for idle,
  110-baud-active and 9600-baud-active event-driven scheduling without redefining
  UART timing.

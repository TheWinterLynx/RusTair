# RusTair Serial Clock Domains

This document is the current architecture contract for serial timing. It supersedes older fixed-slice descriptions in historical validation records. The hardware-specific fidelity documents remain useful for board/chip behavior and source evidence, but current scheduling/ownership is defined here together with `ARCHITECTURAL_INVARIANTS.md`, `RUNTIME_FLOWS.md` and `src/backend/README.md`.

## 1. Core rule

Host CPU execution speed and serial line rate are separate quantities.

`Authentic`, `2x`, `5x`, `10x` and `Unlimited` decide how aggressively RusTair executes guest CPU time on the host. They do **not** restrap an 88-SIO/88-2SIO, alter an MC6850 divider, change a COM2502 frame rate, or redefine an endpoint's configured pacing.

A 110-baud card remains 110 baud in every CPU speed mode. The same is true for 300, 1200, 2400, 4800, 9600 and any other supported physical card rate.

## 2. Distinct time domains

RusTair currently has at least two deliberately separate timing domains that matter to peripherals:

1. **CPU/chassis virtual time** — follows executed Altair CPU/chassis T-state time. Current DCDD/FD-400 mechanics use this domain.
2. **Serial physical time** — follows elapsed physical serial time independently of how many guest CPU T-states the host can execute. 88-SIO/88-2SIO baud generators use this domain.

Do not route one through the other merely because both can be expressed as durations or canonical 2 MHz quanta.

A CPU T-state therefore does not itself clock the serial UART. Conversely, advancing serial physical time must not advance DCDD mechanics or fabricate CPU T-states.

## 3. State ownership

The installed serial card remains the sole authority for UART-visible state:

- COM2502/MC6850 register state;
- receive/transmit shift progress;
- RDA/RDRF/TBMT/TDRE/error state;
- frame completion;
- IRQ state;
- selected baud/word format/divider behavior;
- oscillator/divider phase used to calculate the next serial event.

The application/backend scheduler owns only **when to settle elapsed physical time against that card**. A scheduler deadline is derived from card-owned phase; it is not a second UART clock or a second copy of UART state.

## 4. Event-driven scheduling

The old fixed 4 microsecond scheduling slice has been removed.

In throttled CPU modes, `src/app/execution_frame.rs` asks the backend for the earliest installed serial-card deadline before each normal CPU service interval.

### Idle UARTs

A serial channel whose current internal state cannot change at the next oscillator edge is quiet. Quiet installed UARTs publish no deadline (`None`).

When all installed serial channels are quiet, CPU execution keeps the normal Adaptive service slice (currently 4096 T-states). This allows long Full windows instead of forcing needless host returns merely because a serial card is installed.

The physical oscillator phase is still retained. "No deadline" means no guest-visible/card-state event needs host observation before the normal CPU service boundary; it does not mean the physical oscillator has ceased to exist.

### Active UARTs

An active channel publishes the number of canonical 2 MHz physical-time quanta until its next **effective UART event boundary**. The application converts that deadline to the selected guest CPU speed and caps only the CPU work required to avoid crossing that event.

For the 88-SIO this is the next effective COM2502 bit/frame clock boundary relevant to current state.

For the 88-2SIO, the external MITS baud-generator tap runs at 16x the labelled rate and retains free-running phase continuously. The MC6850 control register then selects `/1`, `/16` or `/64`. The scheduler interrupts CPU work at the next effective divided MC6850 boundary, not at every intermediate external tap pulse.

This distinction is critical at 9600 baud: in the normal `/16` mode the scheduler observes about 9600 effective boundaries per second, not 153600 external tap pulses per second.

## 5. Exact causal barrier for guest serial OUT

An idle UART can become active in the middle of an otherwise large CPU service interval when guest software executes `OUT` to an installed serial card.

RusTair must not execute a large CPU slice and then give the newly active UART the entire slice's elapsed physical time, because that would grant the transmitter time from before the guest actually wrote it.

The managed path therefore has an exact serial-output barrier:

1. Adaptive execution stops before an `OUT` targeting installed serial hardware.
2. The exact Cycle backend owns the T-state progression across the barrier; `CycleHostBackend` does not run its own T-state loop.
3. Physical serial time is settled for the CPU interval that occurred before the card mutation.
4. The serial `OUT` crosses at its real electrical point.
5. The next scheduler query observes the newly changed card state and obtains its new card-owned deadline.

Guest `IN`/`OUT` still use the real S-100 card path. The barrier is scheduling synchronization, not a shortcut around the bus.

## 6. Full/Partial relationship

Independently clocked serial activity is not, by itself, a reason to force all CPU execution into Partial.

Full may continue between observable serial deadlines because the UART advances only when elapsed physical serial time is explicitly settled at a safe scheduler boundary. Guest serial I/O remains an exact synchronization barrier.

This is why an installed idle serial card can coexist with approximately full Adaptive coverage instead of destroying Full with tiny fixed slices.

## 7. Unlimited mode

`Unlimited` has no stable CPU-T-state-to-wall-time ratio. It therefore does not convert CPU execution debt into serial elapsed time.

`CycleHostBackend` uses its automatic `Instant`-based serial physical-time source in Unlimited mode. Switching between managed throttled timing and Unlimited creates one explicit handoff boundary so elapsed serial time is neither replayed twice nor dropped.

GUI repaint cadence is never the UART clock.

## 8. STOP, RESET, HOLD/HLDA and HALT

Serial physical time may continue while useful CPU instruction execution is stopped or parked. STOP, sustained RESET, HOLD/HLDA and HALT must not freeze the independent serial oscillator merely because no ordinary instruction T-states are being consumed.

The backend/application settles elapsed serial physical time without inventing CPU cycles.

## 9. Endpoints and baud configuration

ASR-33, Text Terminal, TCP and host COM are endpoints downstream/upstream of the installed serial card. Their pacing/configuration does not become card baud authority.

Current examples:

- the ASR-33 can run at its authentic 110-baud / 10-cps behavior or explicit accelerated convenience modes;
- the Text Terminal exposes a discrete set of endpoint speeds including 300, 1200, 2400 and 9600 plus Instant;
- the 88-2SIO exposes its documented physical tap choices independently for each port, with the MC6850 divider applied on top.

Endpoint and card settings must not be silently auto-synchronized. A deliberate mismatch is a valid configuration state. The current byte-oriented endpoint model does not claim analog/noise-level corruption or arbitrary independently clocked remote-bit sampling for every mismatch; such effects remain outside the present digital endpoint claim unless explicitly modeled.

## 10. Performance evidence (2026-09-14)

The manual ignored release benchmark `tests/serial_scheduler_benchmark.rs` measures scheduler cost without redefining hardware timing. These numbers are host-specific evidence, not product guarantees.

With the historical 8800b starter configuration, 16K static RAM and an installed idle 88-2SIO, one second of guest CPU target time measured approximately:

| CPU mode | Effective host throughput | Realtime headroom | Full |
| --- | ---: | ---: | ---: |
| Authentic | 254.89 MHz | 127.45x | 99.61% |
| X2 | 252.20 MHz | 63.05x | 99.61% |
| X5 | 259.64 MHz | 25.96x | 99.61% |
| X10 | 259.05 MHz | 12.95x | 99.61% |

A continuously active 110-baud receive BREAK remained roughly 228-251 MHz with about 99.5-99.6% Full.

A deliberately extreme continuously active 9600-baud receive BREAK produced the expected first effective `/16` deadlines of 209/418/1045/2090 guest T-states for Authentic/X2/X5/X10 and still retained 16.49x/16.01x/12.10x/9.21x realtime headroom respectively.

The idle benchmark's roughly 1.5x `deadline` versus `coarse` cost compares normal 4096-T-state host service boundaries against an artificial comparator that executes the whole one-second CPU budget in one call. It is not a claim that an idle UART itself consumes 50% of CPU time.

For historical context, the retired fixed-4-microsecond scheduler reduced Authentic/X2 to 0% Full and failed to sustain realtime in X2/X5/X10 on the same investigation path. The event-driven design replaced that observation policy rather than weakening UART timing.

## 11. Regression evidence

The current timing contract is guarded by multiple layers:

- card-level deadline/phase unit tests in `sio.rs` and `two_sio.rs`;
- `tests/two_sio_idle_chassis_clock.rs` for STOP/RESET/HOLD/RUN clock independence;
- execution-frame tests for idle 4096T service and serial-OUT replanning;
- `tests/debugger_architecture.rs` to keep the exact T-state loop inside the Cycle backend;
- `tests/serial_scheduler_benchmark.rs` for idle and active release-performance evidence;
- broad `cargo test --all-targets` plus release build validation.

## 12. Review checklist

When changing serial timing, verify all of these:

- Is baud still independent from host CPU speed?
- Is card oscillator/divider phase still owned by the installed card?
- Does an idle UART avoid publishing needless deadlines while retaining phase?
- Does an active channel publish the next effective hardware event, not an arbitrary host slice?
- Can a guest serial `OUT` activate the UART without inheriting pre-write elapsed time?
- Does the Cycle backend, not `CycleHostBackend`, own exact T-state iteration?
- Does Unlimited hand off physical time exactly once?
- Do STOP/RESET/HOLD/HALT allow independent serial progress without fake CPU cycles?
- Does DCDD/chassis time remain separate?
- Can endpoint/card baud settings remain independently configured without hidden restrapping?

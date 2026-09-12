# RusTair — Active roadmap

This file contains **only unfinished work that is still relevant to the current architecture**. Completed history belongs in Git, release notes and focused closeout documents rather than in an ever-growing checklist.

## Non-negotiable architecture

- One physical Altair 8800 machine.
- One authoritative Intel 8080 architectural state at synchronization boundaries: `Cpu8080Cycle`.
- CPU board, RAM and I/O cards communicate only through the S-100 bus.
- The Display/Control front panel is attached to that same system bus.
- `AltairChassis` owns physical chassis state but no processor implementation.
- `S100HardwareConfig` slot inventory is the physical hardware configuration authority.
- Adaptive **Full** and **Partial** are internal execution strategies over the same machine, never separate user-visible emulators.
- Historical bugs/limitations remain reproducible; compatibility workarounds must be explicit and opt-in.
- Do not run GitHub Actions without explicit user instruction.

## P0 — Crystal-clean architecture closeout

- [ ] Remove the remaining aggregate/singleton serial configuration authority. 88-SIO/88-2SIO identity, straps and interrupt wiring must come only from the installed S-100 card.
- [ ] Make persistence write physical S-100 hardware once. Old aggregate serial/RAM keys remain read-only migration inputs.
- [ ] Ensure host serial cables resolve unambiguously to an installed card/port. Until slot-aware cable routing exists, do not expose ambiguous multi-serial-card configurations in the UI.
- [ ] Keep migration-only `FastRamCompatibility` readable but prevent creation of new compatibility cards from the normal hardware editor.
- [ ] Make a fresh default configuration use only hardware that the normal physical inventory can represent.
- [ ] Remove stale Fast-vs-Cycle wording, dead transitional comments and obsolete configuration controls.
- [ ] Remove temporary one-off profiling/tests that are not useful long-term regression gates.
- [ ] Keep architecture guards that forbid reintroduction of the old Fast-machine/mirror/synchronization design.

## P0 — UI behavioral closeout

- [ ] Configuration → S-100 Chassis / Cards is the sole place to install/move/configure physical cards; changes require POWER OFF.
- [ ] ASR-33, Text Terminal, External TCP and External COM expose truthful cable connections and never silently rewire the machine.
- [ ] Direct cable compatibility follows the selected physical interface (TTY/current loop, RS-232, TTL); no hidden level converters.
- [ ] BASIC auto-open only reveals the configured console; it must not alter hardware or cable routing.
- [ ] Quick Load remains an explicit host convenience; Authentic Load remains an emulated paper-tape/bootstrap path.
- [ ] Loading a program leaves machine power/reset/run state in the documented deterministic state for that workflow.
- [ ] RAM viewer/debugger/history windows remain layout-stable while execution is running.
- [ ] Perform a release-build manual UI smoke test after the cleanup: POWER, RESET, RUN/STOP, EXAMINE/DEPOSIT, S-100 editor, ASR-33, Text Terminal, BASIC Quick Load, Authentic Load, RAM viewer and debugger.

## P0 — Exact/Partial performance

Performance work must not weaken electrical or timing fidelity.

- [ ] Finish the bounded `Cpu8080Cycle` hot-path audit only where changes show a material win on representative diagnostics.
- [ ] Profile the full Forced-Partial path (`Cpu8080Cycle → CPU board → S-100 settle → cards/front panel`) and identify the dominant cost of the current ~2.7 MHz physical path.
- [ ] Replace redundant resolver work with event/dirty propagation only where the resulting S-100 edges and card-visible state remain identical.
- [ ] Keep classic diagnostics as performance/correctness gates: 8080PRE, TST8080, CPUTEST and 8080EXM.

## P1 — Adaptive Full coverage

- [ ] Extend Full only one instruction family at a time with exact differential/front-panel tests.
- [ ] Re-measure remaining barriers before choosing another Full coverage change; conditional CALL/RET, DI and guarded EI -> LHLD are already supported.
- [ ] Preserve exact synchronization boundaries for I/O, interrupts, HOLD/HLDA, RESET, HLT and any instruction whose physical schedule is not representable by Full.
- [ ] Re-run classic diagnostics and panel-duty differential tests after each coverage expansion.

## P1 — Hardware-fidelity follow-up

The supported base machine should not be reopened generically; add work only for a demonstrated regression or source-backed discrepancy.

- [ ] Re-audit `docs/BASE_HARDWARE_FIDELITY_CLOSEOUT.md` after architecture cleanup and update any statements made obsolete by the unified physical-card runtime.
- [ ] Implement the MITS 88-VI only as a real S-100 card with raw VI inputs/arbitration; serial cards must never fabricate restart opcodes themselves.
- [ ] Add further RAM/I/O/CPU boards only after the base Altair remains green and the card can be represented through normal S-100 ownership.

## P1 — MITS 88-DCDD / 88-DISK + Pertec FD-400

Implementation contract: [`docs/88_DCDD_FD400_IMPLEMENTATION_PLAN.md`](docs/88_DCDD_FD400_IMPLEMENTATION_PLAN.md). Phase-0 source contract: [`docs/88_DCDD_FD400_HARDWARE_CONTRACT.md`](docs/88_DCDD_FD400_HARDWARE_CONTRACT.md). The authentic and accelerated disk paths must share the same physical controller/drive state machines; Fast Disk removes waits but must never become a guest-visible sector API or be implied by host `Unlimited` execution speed.

- [x] **Phase 0 — source-backed hardware contract:** ports, polarities, controller-board wiring, drive selection/topology, base timing, interrupt option and the first physical-media contract are locked down from MITS documentation; unresolved schematic/mechanical details are explicitly deferred rather than guessed.
- [ ] **Phase 1 — physical topology/config skeleton:** add the source-backed DCDD S-100 card set, documented inter-board/controller harness and external drive-bus ownership with no direct CPU/card shortcuts and no intentional idle per-T-state work.
- [ ] **Phase 2 — decode/reset/register surface:** implement real S-100 I/O decode and controller state/reset behavior without media, proving accesses through public 8080/S-100 cycles and preserving front-panel visibility.
- [ ] **Phase 3 — FD-400 mechanics/time engine:** implement rotational phase, hard-sector/index position, track/head/step state and source-backed deadlines using virtual-time epochs/events rather than per-T-state ticking.
- [ ] **Phase 4 — media abstraction/read-only surface:** mount/eject a validated physical hard-sector image below the drive electronics, with disk-present/write-protect state separate from host file/path handling.
- [ ] **Phase 5 — authentic read path:** reproduce synchronization/data-ready cadence and late-service behavior end to end through FD-400 -> DCDD -> S-100 -> 8080, using arithmetic catch-up where observationally equivalent.
- [ ] **Phase 6 — authentic write path:** reproduce write-enable/data-ready/sync/erase timing, write protection and physical media mutation without filesystem/sector shortcuts.
- [ ] **Phase 7 — interrupts/edge cases/revisions:** implement only source-backed raw interrupt signaling, door/no-media/address/track-zero cases and separately selectable historical revisions where documentation proves a behavioral difference.
- [ ] **Phase 8 — Fast Disk timing policy:** collapse mechanical/rotational/byte waits to the earliest safe observable transitions while preserving the exact guest protocol, state ordering and logical error conditions of the authentic hardware.
- [ ] **Phase 9 — persistence/chassis/media UI:** persist physical cards, drive addressing and timing policy through authoritative config; add truthful mount/eject/write-protect controls without UI shadow state.
- [ ] **Phase 10 — authentic software/bootstrap validation:** boot representative MITS disk software through CPU -> S-100 -> DCDD -> FD-400 -> media with no PC trap, memory injection or direct sector hook; run the same software unchanged in Fast mode.
- [ ] **Phase 11 — performance/closeout:** add installed-idle, polling, continuous read/write, late-byte and many-idle-drive benchmarks; target <2% idle regression, <8% polling regression and <15% continuous-transfer regression before final fidelity/docs closeout.

Phase rules:

- [ ] Each phase lands only when its focused PASS gate in the implementation contract is green; do not weaken an earlier gate to unlock a later phase.
- [ ] One vs sixteen idle drives must not create O(drives × T-states) work; rotational/mechanical state should be epoch/deadline-derived where possible.
- [ ] Authentic peripheral timing is guest virtual time. 1x/5x/10x/Unlimited changes host throughput only and must not change the historical FD-400 clock/timing model.
- [ ] Any new Rust source or integration-test file is added to `docs/SOURCE_REFERENCE.md` or `docs/TEST_REFERENCE.md` in the same phase.
- [ ] Do not run GitHub Actions unless explicitly requested.

## P1 — Configuration and persistence

- [ ] Keep current config schema atomic and crash-safe.
- [ ] Old formats must migrate without losing user hardware/wiring choices, then normalize to the current physical representation on the next save.
- [ ] Invalid or electrically ambiguous hardware inventories must be rejected explicitly rather than silently repaired.
- [ ] Configuration defaults should be useful, truthful and expressible through the same UI a user sees.

## P2 — Tools and polish

- [ ] Nested/adjacent loop-inspector support without presenting speculative boundaries as certain.
- [ ] Continue debugger/teacher presentation improvements only when they do not add execution hot-path work while closed.
- [ ] Improve native-window responsiveness and visual polish after the machine/runtime architecture is stable.
- [ ] Release/licensing/documentation hygiene for a distributable build.

## Release gate

A cleanup/performance/fidelity phase is not complete until:

1. `cargo test` passes locally.
2. Relevant ignored classic diagnostic tests pass when production CPU/S-100 behavior changed.
3. `cargo run --release` passes the manual UI smoke test for the affected workflows.
4. No GitHub Actions were launched unless explicitly requested.

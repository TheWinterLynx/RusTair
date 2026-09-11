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
- [ ] Finish any remaining dynamic-RAM timing/refresh behavior that is still explicitly marked non-PASS in the card model/UI.
- [ ] Implement the MITS 88-VI only as a real S-100 card with raw VI inputs/arbitration; serial cards must never fabricate restart opcodes themselves.
- [ ] Add further RAM/I/O/CPU boards only after the base Altair remains green and the card can be represented through normal S-100 ownership.

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

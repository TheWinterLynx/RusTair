# RusTair Support Scope and Known Limitations

This document states what the current project **does** model, what it deliberately does **not** claim, and which areas are still known work in progress.

It is not a release-note history. For current unfinished work, `TODO.md` remains the active roadmap. For detailed hardware claims, use the component-specific fidelity documents under `docs/`.

---

## 1. What RusTair currently models

The current production architecture models a MITS Altair family machine around a live S-100 chassis/backplane and an Intel 8080 CPU board.

Current implemented areas include:

- Intel 8080 architectural execution;
- exact T-state accounting and digital PHI1/PHI2 edge sequencing in the exact core;
- MITS 8080 CPU-board/S-100 electrical boundary;
- Altair 8800 / 8800a / 8800b chassis connector-population models;
- physical S-100 RAM card inventory with slot-native address/population/timing/protection behavior;
- MITS 88-SIO serial hardware;
- MITS 88-2SIO with MC6850-based ports;
- front-panel Display/Control behavior including RUN/STOP, RESET, EXAMINE/DEPOSIT and protection-related behavior;
- ASR-33 teletype/peripheral workflows;
- text terminal endpoint;
- TCP serial endpoint;
- host COM serial endpoint;
- debugger, history, memory viewer, I/O inspector, Bus Teacher and CPU-pin teaching tools;
- direct/Quick Load workflows;
- authentic paper-tape/BASIC loading workflows through emulated serial/peripheral hardware;
- embedded classic Intel 8080 diagnostics;
- persistent application/hardware configuration with migration support for older schemas.

The exact supported details of each card are documented separately. Do not infer that every possible historical strap/revision/accessory is implemented merely because the board family exists.

---

## 2. Adaptive Cycle execution scope

RusTair has one production execution engine: Adaptive Cycle 8080.

It internally combines:

- **Partial** exact T-state execution;
- **Full** accelerated semantic windows for proven-safe instruction/chassis situations.

Full coverage is intentionally incomplete. Falling back to Partial is correct and expected whenever the physical machine may observe behavior that Full does not represent exactly.

Current Full support must not be interpreted as a promise that every 8080 instruction, interrupt state, READY condition or external event is accelerated.

The active roadmap explicitly calls for extending Full one instruction family at a time with exact differential/front-panel evidence.

---

## 3. Digital timing claim versus analog-electronics claim

RusTair models documented digital ordering and hardware-visible state. The exact 8080 core represents T-states and decomposes them into non-overlapping PHI1/PHI2 digital edges.

RusTair does **not** claim transistor-level or analog circuit simulation of:

- voltage rise/fall time;
- exact analog pulse width;
- line impedance/reflections;
- per-gate propagation-delay distributions;
- metastability;
- power-rail ripple/noise;
- component-temperature variation;
- analog UART waveform shape.

If a historical behavior depends on one of those effects, it should be documented as outside the current model rather than presented as exact.

---

## 4. RAM fidelity scope

Status: **PASS — bus-visible digital RAM hardware fidelity is closed for the supported MITS RAM inventory.** Canonical closeout record: [`RAM_HARDWARE_FIDELITY.md`](RAM_HARDWARE_FIDELITY.md).

The runtime uses physical installed RAM-card instances as the authoritative guest storage. The supported MITS RAM inventory now has a closed **bus-visible digital timing** model rather than a static-RAM-only PASS claim.

In particular:

- the original 1K static board retains its documented two read wait states;
- the 88-4MCD tracks its 32-CLOC refresh cadence and asserts PRDY only when a selected memory access collides with the documented one/two-WAIT refresh window;
- the 88-S4K derives its synchronous refresh cadence from PHI2 and uses the corrected MITS pDBIN-falling-edge timing reference while sM1 is asserted to identify the hidden T4 refresh slot without asserting PRDY; its card logic also accepts the documented STOP/HLTA refresh path;
- the 88-4MCS, 88-16MCS and 88-16MCD remain processor-no-wait boards in the digital S-100 timing model;
- open bus, overlapping decoders, card-scoped protection and the one authoritative runtime storage remain unchanged.

Supported historical dynamic RAM may participate in Adaptive Full when the rest of the chassis satisfies Full's safety proof. Full does not replace those boards with a static-memory shortcut: the 88-4MCD advances the same physical card refresh phase and feeds collision TW states back into the semantic instruction timing, the 88-S4K advances the same synchronous refresh state without PRDY, and the crystal-controlled 88-16MCD remains processor-transparent at this digital boundary. Differential regressions compare long Full runs against forced Partial for T-state count, instruction progression, registers, guest RAM and front-panel state.

This is deliberately **not** an analog DRAM-cell simulator. RusTair does not model capacitor leakage, charge decay, temperature-dependent retention margins or destructive failure after missed refresh. Those effects are outside the digital S-100 fidelity claim described in section 3.

`FastRamCompatibility` remains a migration/compatibility concept and should not become the preferred way to create new hardware from the normal editor.

---

## 5. Serial configuration/persistence cleanup still in progress

The architecture already requires one physical installed serial-card authority, but the active roadmap still calls out cleanup around older aggregate/singleton serial configuration and persistence.

Known constraints include:

- older aggregate serial/RAM keys may still exist as migration inputs;
- persistence should converge on writing physical S-100 hardware once;
- host serial cable routing must resolve unambiguously to an installed physical card/port;
- until slot-aware routing is fully unambiguous, the UI should avoid exposing ambiguous multi-serial-card configurations;
- direct cable compatibility must follow the selected physical interface; hidden level conversion is not part of the base model.

These are architecture-cleanup constraints, not permission to introduce another runtime UART/configuration authority.

---

## 6. Interrupt-controller scope

Serial cards can drive the configured S-100 interrupt lines, and Intel 8080 interrupt/INTE timing is modeled in the CPU path.

The active roadmap identifies the **MITS 88-VI** as future work. It must eventually be implemented as a real S-100 card with raw vectored-interrupt inputs/arbitration.

Until such a card exists in production:

- peripheral cards must not fabricate restart opcodes to simulate a missing interrupt controller;
- do not claim completed 88-VI arbitration/vector hardware fidelity;
- keep interrupt-source wiring and CPU acknowledge semantics separated from future vector-controller behavior.

---

## 7. Chassis/card support is not universal S-100 support

RusTair provides a generic S-100 electrical card/backplane contract, but only explicitly implemented card families are supported.

A generic S-100 interface does not mean arbitrary historical S-100 hardware can be loaded as a plugin without an implementation.

New boards should be added only when they can be represented as physical cards that observe/drive the existing bus contract and have source-backed behavior/tests.

---

## 8. Front-panel fidelity scope

RusTair distinguishes:

- raw bus/status state;
- T-state-weighted lamp activity/duty;
- host-side visual LED persistence/brightness.

The first two are part of emulator fidelity; the final optical presentation is a UI model.

The project does not claim analog LED/electrical-lamp simulation such as exact diode current, resistor tolerance or photographic brightness calibration against every physical Altair unit.

A visually pleasing LED effect must never overwrite raw bus truth.

---

## 9. Debugger/history limitations

Debugger-derived tools intentionally use bounded observations.

Consequences:

- instruction history can evict older entries;
- inferred call stacks can be incomplete;
- loop detection is deliberately conservative and currently focuses on high-confidence simple backward loops;
- a frozen/historical Bus Teacher snapshot is not necessarily the current machine state;
- inferred structures are not architectural CPU state.

The active roadmap mentions future nested/adjacent loop-inspector support, with the explicit requirement that speculative boundaries not be presented as certain.

---

## 10. Host execution speed and throughput are not hardware-support claims

Measured equivalent MHz depends on:

- host CPU;
- compiler/toolchain;
- build profile;
- profiler instrumentation;
- guest workload;
- installed hardware/configuration;
- Full/Partial admission pattern.

Performance figures in historical documents are evidence from a particular test configuration, not a permanent product guarantee.

Authentic/5×/10×/Unlimited speed modes affect host scheduling only. They do not change the hardware identity of the installed 2 MHz-class MITS CPU board.

---

## 11. GUI and platform limitations

The desktop UI is based on eframe/egui/WGPU and includes large photographic assets. Normal GUI use is documented against release builds.

Host features may depend on platform capabilities:

- COM-port enumeration/access depends on host OS and `serialport` support;
- audio availability depends on a usable host playback device;
- missing host audio is intentionally non-fatal;
- file dialogs are host-native UI facilities rather than emulated hardware.

None of these host facilities may become a source of guest CPU/card truth.

---

## 12. Configuration changes require POWER OFF

Moving/installing/restrapping physical S-100 cards represents a physical hardware change. The configuration UI therefore treats those operations as POWER-OFF changes.

This is deliberate behavior, not a UI inconvenience to bypass.

Runtime register state inside an already installed card is distinct from physical strap/card configuration.

---

## 13. Quick Load is intentionally not historically authentic

Quick/direct load is an explicit emulator convenience.

It can place bytes into the physical RAM storage without reproducing the historical transport mechanism.

Authentic Load is the separate path for users/tests that want bootstrap + paper tape + serial/peripheral behavior.

Do not describe Quick Load as hardware-authentic merely because it writes the same final RAM bytes.

---

## 14. Historical documentation can describe older architecture/performance states

The repository intentionally preserves hardware-fidelity investigations and performance experiments.

Some records describe:

- earlier internal architecture;
- a specific experimental branch;
- an optimization that was later rejected;
- measurements on an older commit/toolchain.

Current contributor documents (`docs/README.md` reading path) take precedence for present-day architecture. Historical files remain evidence and should not be silently rewritten to appear current.

---

## 15. Current structural debt explicitly acknowledged by the project

`STATE_SOURCES.md` and `TODO.md` currently acknowledge remaining structural cleanup, including:

- further backend encapsulation so application code increasingly depends on common contracts rather than concrete machine internals;
- compatibility facades retained for migration/tests that must not become guest-visible alternate authorities;
- persistence normalization to the current slot-native physical model;
- legacy/transitional wording/configuration cleanup.

A contributor should not "solve" this debt by introducing another compatibility layer that makes ownership less clear.

---

## 16. What is not automatically a bug

These behaviors can be intentional:

- Full falls back to Partial;
- unmapped space returns open-bus behavior rather than useful data;
- overlapping decoders create multiple responders/contention;
- a configuration is rejected as electrically ambiguous;
- an endpoint cannot connect because the selected interfaces are incompatible;
- a debugger inference says incomplete/unknown;
- a host benchmark is slower under symbols/profiling;
- a historical software quirk requires an explicit compatibility option;
- changing S-100 card installation is blocked while POWER is on.

Before "fixing" such behavior, identify the hardware/documentation contract first.

---

## 17. Where to confirm a support claim

For any claim such as "RusTair correctly supports feature X", use this evidence order:

1. current production source;
2. focused automated tests/oracles;
3. current architecture/state-source documents;
4. component-specific hardware-fidelity record;
5. historical research/measurements as supporting evidence;
6. manual release-build behavior where the feature is inherently UI/host dependent.

A README bullet alone is not enough evidence for a low-level hardware fidelity claim.

---

## 18. Maintaining this document

Update this file when:

- a planned hardware component becomes implemented;
- a known non-PASS area is closed;
- a compatibility/migration limitation is removed;
- a new deliberate non-claim is introduced;
- a platform limitation changes materially.

Do not use this file as a substitute for detailed card fidelity documentation. It should remain the high-level truth about **scope and caveats**.

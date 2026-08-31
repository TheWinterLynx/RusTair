# Hardware fidelity documentation standard

This document defines the minimum documentation required for any RusTair component that claims historical, electrical, timing, or behavioral fidelity to physical Altair-era hardware.

The documentation is part of the implementation. A hardware item is not `PASS` merely because software runs or tests are green; its physical counterpart, evidence, model boundaries, code realization, and known divergences must also be documented.

## 1. Source hierarchy

Use sources in this order whenever possible:

1. Original manufacturer theory/assembly/service manuals for the exact board or device.
2. Original semiconductor manufacturer data sheets / application manuals for chips used on that board.
3. Original CPU documentation for bus/timing interactions.
4. Contemporary MITS software manuals, Computer Notes, application notes, schematics and field modifications.
5. Period photographs, surviving board schematics and documented restorations as corroborating evidence.
6. Other emulators only as secondary comparison or differential oracles; they are never the historical authority.

Every hardware document must contain a `Primary references` section with stable title, publisher/manufacturer, date/revision when known, URL/archive location, and the pages/sections actually used. If a primary source is unavailable, that absence must be stated explicitly instead of silently substituting a modern implementation.

## 2. Required sections for every hardware piece

Each dedicated hardware document should contain, at minimum:

### Scope and fidelity claim

State exactly what is being modeled and what is not. Distinguish digital/electrical fidelity from analog effects, mechanical presentation, host integration and compatibility modes.

### Physical hardware

Describe the physical board/device: major ICs, connectors, jumpers/straps, clocks, address decoding, bus signals, interrupt/READY/HOLD behavior, modem/control lines, mechanical interfaces and any revision-sensitive behavior relevant to emulation.

### Register / signal / timing tables

Where applicable, reproduce the semantics as compact tables derived from the original manuals: register addresses, status bits, control bits, state transitions, clocks, wait states and externally visible lines. Do not paste long copyrighted manual passages; paraphrase the behavior and cite the source.

### RusTair model

Explain which Rust types own each piece of physical state and why. Include a physical-to-software mapping table, for example:

| Physical element | RusTair owner | Notes |
| --- | --- | --- |
| ACIA receive data register | `Mc6850::rdr` | One-byte hardware register |
| S-100 PRDY contribution | `IoDevices::ready_for_input_t_state` | Card-level READY source |

### Supporting code snippets

Include short, current snippets that demonstrate the implementation contract. Snippets should be small enough to remain maintainable and must name the source file and symbol. Prefer snippets that encode an invariant rather than large copied functions.

### Fast versus Cycle Accurate

Document separately what each engine can claim. Cycle Accurate may claim exact T-state/pin sequencing only where it truly samples those signals. Fast may reconstruct total elapsed T-states and guest-visible behavior but must not be described as pin-exact when it cannot expose sub-instruction events.

### Peripheral / host boundary

Explain where physical hardware ends and the host UI, terminal, ASR-33, TCP socket, COM port, debugger or file loader begins. Host presentation must never become the authority for hardware bits such as TDRE/RDRF/READY/IRQ.

### Regression evidence

List exact unit/integration tests and what physical invariant each one protects. A green test name without the invariant is insufficient documentation.

### Validation history

Record meaningful local validation checkpoints (date and command class, not necessarily every run). Do not claim GitHub Actions validation unless it was explicitly requested and run.

### Known gaps / non-goals

List unresolved fidelity gaps and explicitly label harmless analog/presentation omissions separately from digital/electrical blockers.

### Primary references

Provide original documentation links and page/section notes.

## 3. Documentation and closeout policy

From this point forward, a hardware item may move to `PASS` only when all of the following are true:

- primary-source behavior has been identified;
- the physical-to-software ownership model is documented;
- implementation snippets are included;
- relevant Fast/Cycle differences are documented;
- focused regressions exist;
- the normal local suite is green after the change;
- the dedicated Markdown document has no undocumented fidelity blockers.

The live closeout ledger (`docs/BASE_HARDWARE_FIDELITY_CLOSEOUT.md`) should link to the dedicated document instead of duplicating every implementation detail.

## 4. Change discipline

When implementation changes invalidate a snippet, status table or fidelity claim, update the hardware document in the same logical change set. Documentation that describes an older model is considered a regression even if the Rust compiler is green.

When a source is ambiguous or revision-specific, write the uncertainty down. RusTair prefers an explicit `UNKNOWN / revision-sensitive` statement over an invented universal behavior.

## 5. Naming convention

Use descriptive all-caps filenames for dedicated physical components, for example:

- `88_2SIO_MC6850_HARDWARE_FIDELITY.md`
- `88_SIO_HARDWARE_FIDELITY.md`
- `S100_BUS_HARDWARE_FIDELITY.md`
- `ALTAIR_FRONT_PANEL_HARDWARE_FIDELITY.md`

A later documentation-only refactor may split very large components further, but the physical ownership boundary should remain obvious from the filename.

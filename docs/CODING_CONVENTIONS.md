# RusTair Coding Conventions

This guide explains the coding habits expected in RusTair. It is not a generic Rust style manual: the emphasis is on making hardware-fidelity code reviewable and keeping one authoritative emulated machine.

New Rust developers should read [DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md) first.

---

## 1. Optimize for explicit physical meaning

Prefer names that tell a reviewer what physical concept a value represents.

Good examples from the project vocabulary:

- `cpu_control_lines`
- `memory_responder_mask`
- `interrupt_request`
- `read_wait_states`
- `panel_activity`
- `boundary_pins`
- `serial_clocked`

Avoid generic names such as `state2`, `fast_data`, `temp_bus` unless the lifetime/meaning is genuinely local and obvious.

If a field is a cache/derived observation, make that visible in the name or type. A reviewer should not confuse a cached display sample with the authoritative S-100 state.

---

## 2. Keep authoritative state single-owned

Before adding a field, ask:

> Is this new storage the owner of a physical state, or only a derived/cache/presentation copy?

If it duplicates an existing authority, prefer:

- computing it on demand;
- a read-only snapshot;
- an explicitly invalidated cache;
- moving the original authority to the correct layer rather than adding a mirror.

Do not synchronize two permanent copies of CPU, RAM, UART or RUN state merely because that is easier locally.

---

## 3. Prefer typed hardware state over magic integers

Use enums/structs for:

- machine-cycle kind;
- T-state/clock edge;
- card/interface type;
- interrupt target;
- chassis/slot/card config;
- signal identity;
- electrical drive type.

Raw bit masks are appropriate in hot electrical/decode code when the representation itself is meaningful and documented. Hide encoding details behind methods/constants rather than spreading literal shifts/masks throughout unrelated modules.

Hardware literals such as Intel status bytes should be named/commented where the numeric value is not self-explanatory.

---

## 4. Separate semantic behavior from timing behavior

For the 8080:

- instruction result/flags belong to semantic/ALU logic;
- machine-cycle/T-state/edge placement belongs to the exact cycle core;
- board-level translation belongs to the CPU-board/S-100 layer.

Do not "fix" a timing bug by adding arbitrary delay to semantic code, or fix an ALU bug by altering bus timing.

When both layers change, keep the reason for each part clear in the diff/tests.

---

## 5. Hardware cards use the bus contract

A card implementation should behave like a device plugged into the S-100 backplane:

- inspect relevant bus signals;
- update its own internal state;
- drive/release its connector outputs;
- expose host inspection only through separate controlled handles.

Do not add direct calls from one card implementation to another.

Generic backplane code must remain unaware of card family whenever possible.

---

## 6. Host inspection is not a guest bus transaction

Debugger/UI may need to inspect a physical card without spending guest T-states. Make the distinction explicit.

Examples:

- RAM viewer read: host inspection of authoritative RAM bytes;
- guest memory read: S-100 machine cycle with decoder/card/bus semantics;
- debugger memory edit: explicit host mutation operation;
- front-panel DEPOSIT: modeled physical panel/bus operation.

Do not reuse a guest-transaction function for host inspection if that would create false timing, and do not reuse host inspection to bypass guest hardware.

---

## 7. Full optimizations need an equivalence story

When writing Full/compiled code, comments should explain **why omitted host work cannot change physical behavior**, not simply that the code is faster.

For a special case, document:

- admission preconditions;
- exact observable behavior preserved;
- event/synchronization boundary;
- invalidation/fallback condition;
- oracle tests.

If the proof is complicated enough that a future reviewer might "simplify" it incorrectly, keep the explanation next to the code and link a focused test/document.

---

## 8. Use assertions intentionally

### `debug_assert!`

Good for invariants that should be impossible if admission/state-machine logic is correct and whose production check would be unnecessary overhead.

Examples: an admitted opcode has expected timing, a transient Full arm is cleared before finish, an internal state is at a clean boundary.

### `assert!`

Use in tests and for truly unconditional programmer invariants where continuing would be invalid.

### `Result`

Use for invalid user/configuration/environment inputs that should be reported rather than panic.

Do not turn physical fallback behavior into `unwrap()`/`expect()` solely to shorten code.

---

## 9. Error policy: guest hardware versus host services

Distinguish:

- **guest/emulated hardware fault/state** — represented in emulator state/behavior;
- **invalid configuration** — returned as typed validation/error;
- **optional host service failure** (e.g. audio device unavailable) — may be non-fatal if the emulator can continue correctly;
- **programmer invariant violation** — assertion/panic in the appropriate context.

A failed host speaker should not corrupt the guest CPU. A physically impossible S-100 configuration should not be silently accepted.

---

## 10. Visibility

Use the narrowest useful Rust visibility:

- private helper when only a module needs it;
- `pub(super)` for parent/submodule collaboration;
- `pub(crate)` for cross-crate-internal architecture;
- `pub` only when genuinely part of the crate's public/API surface.

Do not expose internals just so a UI can bypass the backend. Add a backend snapshot/command instead.

Test-only helpers should normally remain under `#[cfg(test)]`.

---

## 11. Comments: explain invariants, not syntax

Useful comment:

> The final LHLD T-state is split so the delayed EI transition is accounted with INTE high only on the exact edge; the admission guard prevents this presentation split from becoming a rejoin boundary.

Low-value comment:

> Increment the counter.

Preserve historical/hardware-source reasoning near unusual code. Emulation often contains logic that looks redundant until its electrical timing reason is known.

---

## 12. Module documentation

Important low-level modules should say:

- what physical/software component they model;
- what state they own;
- what they deliberately do **not** own;
- their main inputs/outputs;
- fidelity scope/non-claims when relevant.

If a new `src/**/*.rs` file is created, add it to `docs/SOURCE_REFERENCE.md`; the documentation inventory test will enforce the path listing.

If a new integration test is created, document it in `docs/TEST_REFERENCE.md`.

---

## 13. Formatting and diff discipline

Run:

```powershell
cargo fmt
```

but inspect the diff. Avoid formatting unrelated files as part of a fidelity change.

Before commit:

```powershell
git diff --stat
git diff
git status --short
```

Small, focused diffs make hardware review much safer.

---

## 14. Lints/warnings

Final validation uses `RUSTFLAGS=-Dwarnings`.

Prefer fixing:

- dead code;
- unused imports;
- stale assignments;
- unreachable variants;
- obsolete compatibility wrappers.

Use `#[allow(...)]` only when the construct is intentional and the reason remains true. Include a comment when the reason is non-obvious.

Do not keep speculative variants/APIs solely because "we may need them later". Add hardware variants when the actual modeled board/core exists.

---

## 15. `unsafe`

Introducing Rust `unsafe` requires exceptional justification, especially in emulator hot paths where a memory-safety bug can masquerade as guest-state corruption.

If it becomes necessary:

- isolate it in the smallest possible helper;
- document the complete safety invariant;
- add tests around the boundary;
- prove the performance/interop need;
- do not use `unsafe` merely to silence the borrow checker around unclear ownership.

---

## 16. Avoid allocation/locking in hot physical loops unless required

Exact T-state/S-100 paths execute extremely frequently. Prefer:

- fixed-size arrays/bitsets for fixed hardware topology;
- preallocated bounded history for observers;
- cache structures with explicit invalidation;
- stack/local small values;
- opt-in metrics rather than global synchronized counters.

But do not change representation based on intuition alone: profile first and preserve an equivalence oracle.

---

## 17. Keep observers opt-in where they are expensive

Instruction history, memory activity, I/O tracing, teaching snapshots and performance counters can be expensive at emulator frequencies.

Where practical:

- capture only when the relevant tool is enabled;
- bound history;
- avoid per-T-state allocation;
- disable/clear cleanly without affecting machine state.

An observer may slow execution; it must not change guest results.

---

## 18. Configuration names should describe physical/user concepts

Examples:

- card model/slot/address straps;
- baud tap;
- electrical interface;
- interrupt target;
- host endpoint address;
- terminal pacing preference.

Avoid exposing internal optimizer toggles as user hardware configuration unless there is a real use case and the fidelity/performance mode is explicitly defined.

---

## 19. Commit organization

Prefer logical commits such as:

- `Add 88-SIO Rev0 device-ready latch oracle`
- `Model Rev0 data handshake through S-100`
- `Update 88-SIO fidelity documentation`

rather than one large mixed `cleanup and fixes` commit.

Performance experiments should stay isolated so KEEP/REJECT is evidence-based. Do not mix an optimization with unrelated refactoring that makes A/B attribution impossible.

---

## 20. Review questions for any low-level diff

A reviewer should be able to answer:

- What physical component owns the changed state?
- Which real signal/event causes each transition?
- Is timing expressed at the right T-state/edge?
- Does any new shortcut bypass the S-100?
- Does UI/debugger state remain derived?
- What happens on open bus / overlap / High-Z?
- What happens with READY/HOLD/interrupt/reset?
- If Full is involved, what is the Partial oracle?
- What test would fail if this invariant regressed?
- Is performance work removing redundant host work or removing physical behavior?

If the diff cannot answer these clearly, it needs stronger documentation/tests before merge.

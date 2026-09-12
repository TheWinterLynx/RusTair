# MITS 88-DCDD / 88-DISK + Pertec FD-400 implementation plan

Status: **planned; implementation not started**

This document is the staged implementation contract for adding the original Altair 8-inch floppy subsystem without weakening RusTair's hardware-fidelity architecture.

The target is not a sector API disguised as an S-100 card. The target is the physical chain:

```text
8080 CPU board
    |
    v
S-100 backplane
    |
    +-- MITS 88-DCDD controller board #1
    +-- MITS 88-DCDD controller board #2
             |
             v
      documented controller harness
             |
             v
       disk-unit cable/bus
             |
       +-----+-----+ ...
       |           |
       v           v
   disk unit 0  disk unit N
       |
       +-- unit-address / buffer electronics
       +-- Pertec FD-400 mechanism/electronics
       +-- removable hard-sectored 8-inch media
```

A direct physical interconnect documented by the real hardware is allowed and must be represented as such. No card may gain a software-only reference to another card or to the CPU. Guest I/O still crosses the live S-100 fabric.

## Non-negotiable design rules

1. **One physical machine.** The DCDD is installed through normal S-100 hardware configuration and participates in the same live bus seen by the CPU and front panel.
2. **No sector-level guest shortcut.** Guest `IN`/`OUT` operations must not directly index a disk-image sector or call a host filesystem API.
3. **Physical ownership.** Controller-board state belongs to the controller boards/interconnect; mechanism state belongs to the FD-400; media state belongs to the mounted medium.
4. **Real external wiring is explicit.** The documented board-to-board/controller-to-drive harness is a physical signal interface, not an exception that permits arbitrary direct card calls.
5. **Virtual time is authoritative.** Authentic peripheral timing advances in guest-machine time, not wall-clock time and not host execution speed.
6. **Unlimited host execution does not make historical hardware faster.** Normal 1x/5x/10x/Unlimited execution controls only how quickly guest virtual time is processed.
7. **Fast Disk is explicit.** An accelerated disk policy may collapse mechanical/data-transfer delays, but it must use the same controller protocol and state machines rather than bypass them.
8. **Fast and Authentic share functional logic.** Timing policy is injected at the FD-400/controller timing boundary; there must not be a second fast disk controller implementation.
9. **Inactive hardware is cheap.** An installed but idle DCDD/drive must not add per-T-state work to the CPU hot path.
10. **No fabricated interrupts.** The DCDD may assert only the documented raw interrupt request/signals. Priority/vector generation belongs to the real interrupt hardware path.
11. **No magic boot.** Authentic boot software must execute on the 8080 and reach the disk through the real S-100 I/O path.
12. **Source-backed behavior only.** Ports, polarities, timings, revisions, jumpers, drive addressing and edge behavior must be backed by primary hardware documentation before being declared PASS.

## Timing modes

The drive/controller subsystem will expose an explicit timing policy independent of host execution speed.

### Authentic

The physical model preserves documented behavior such as:

- 360 RPM rotational phase;
- 32 hard sectors per revolution;
- 77 tracks;
- 250 kbit/s serial transfer rate / one byte interval every 32 microseconds once synchronized;
- track-to-track stepping and head-load/settle delays;
- sector/index timing;
- read-data-ready and write-data-ready timing;
- write/trim-erase timing;
- byte loss/overwrite or other documented consequences when guest software fails to service the controller in time.

Timing constants are not considered final until Phase 0 records the exact source and revision to which each constant applies.

### Fast

Fast mode removes waiting, not hardware structure.

It must preserve:

- the same guest-visible I/O ports and polarity;
- the same board decode and S-100 bus cycles;
- the same controller state transitions/order;
- drive selection and address matching;
- disk-present, door/open, write-protect and track-limit semantics;
- the same media contents and controller data path.

It may collapse:

- rotational latency;
- head-load/settle latency;
- step latency;
- byte pacing;
- post-write mechanical/electrical delays where doing so cannot remove a guest-visible state transition required by real software.

Fast mode must never become `read_sector()`/`write_sector()` from the guest path. A driver must still execute its polling and byte I/O instructions; the impossible drive simply becomes ready at the earliest safe observable transition.

## Event/deadline model

The implementation must not tick every installed drive every T-state and must not schedule one host callback per magnetic bit.

Prefer absolute virtual-time state and arithmetic catch-up:

```text
rotation_epoch
next_sector_edge
head_ready_at
step_complete_at
next_read_byte_at
next_write_byte_at
trim_erase_complete_at
```

When observed at virtual time `now`, the device derives the physical state that would exist at `now`. Intermediate repetitive transitions may be collapsed arithmetically when no external observer could distinguish them. Externally significant edges such as an interrupt-producing sector transition must still be delivered at the correct virtual-time boundary.

Sixteen idle drives should be nearly the same host cost as one idle drive. Rotational phase should normally be derivable from an epoch/deadline rather than advanced by repeated ticks.

## Staged implementation

Each phase is independently reviewable. Do not begin a phase by weakening the PASS criteria of an earlier one.

### Phase 0 — Source-backed hardware contract

Deliverables:

- Create a source matrix for the 88-DCDD controller boards, disk-unit electronics, Pertec FD-400, connectors/harness and known controller revisions/modifications.
- Record the exact decode of the historical I/O channels (nominally 08h-0Ah), read/write meaning of every bit, active polarity and reset/default behavior.
- Record every documented controller-board interconnect and external drive signal.
- Record sector/index, head load, stepping, read, write, erase and interrupt timing with revision/source attribution.
- Resolve the exact raw S-100 interrupt wiring before implementing interrupt behavior.
- Record drive-address strapping, maximum supported units and daisy-chain/buffer behavior.
- Record original and later timing/revision differences (including NWD only if primary documentation proves the exact behavioral delta).
- Define the first supported media representation and clearly distinguish physical on-disk bytes from filesystem/OS sectors.

PASS gate:

- No unresolved assumption is encoded as production behavior.
- Every guest-visible signal/bit/timing in the first implementation has a source reference or is explicitly marked deferred/non-claimed.
- Architecture review agrees on ownership boundaries before code creates them.

### Phase 1 — Physical topology and configuration skeleton

Deliverables:

- Add the DCDD controller-card identities to the physical S-100 inventory.
- Represent both controller boards as the real fitted S-100 hardware required by the subsystem rather than one synthetic monolith, subject to Phase 0 confirming the exact slot/topology requirements.
- Add an explicit controller-board harness/interconnect signal object.
- Add external disk-bus/unit types with no media implementation yet.
- Establish ownership and lifetime under the runtime fabric/machine chassis without giving one board direct mutable access to another.
- Add configuration validation for incomplete/impossible physical assemblies rather than silently repairing them.

PASS gate:

- Existing machine behavior and tests are unchanged with no DCDD fitted.
- Fitted DCDD boards participate only through S-100 plus the documented physical harness.
- Architecture guards prevent a direct CPU/card/sector shortcut.
- Idle fitted hardware adds no intentional per-T-state polling loop.

### Phase 2 — Controller decode, reset and register/state surface

Deliverables:

- Implement S-100 I/O address decoding for the source-backed controller channels.
- Implement read/write direction, active-low/active-high behavior and reset/power-off state without a drive attached.
- Implement board-to-board signal ownership through the harness.
- Make front-panel ADDRESS/DATA/STATUS behavior come naturally from the same S-100 cycles.
- Define open-bus/non-enabled behavior from sources rather than convenience.

PASS gate:

- Focused integration tests drive real 8080/S-100 I/O cycles and observe exact controller-visible results.
- No test reaches private controller state to simulate a guest access when the production path is being validated.
- Power/reset/disabled/no-drive cases have explicit tests.

### Phase 3 — FD-400 mechanics and virtual-time engine

Deliverables:

- Model drive power/ready conditions required by the documented unit.
- Model rotational epoch/phase, hard-sector/index position, current track, head load state and step direction.
- Implement documented mechanical/electrical deadlines with no per-T-state drive ticking.
- Keep platter rotation independent of CPU RUN/HLT/STOP when machine power and drive state say it should continue.
- Add deterministic test-time control of virtual time.

PASS gate:

- Sector position advances from virtual time even when the CPU is not executing instructions.
- Stepping/head readiness changes only at documented deadlines in Authentic mode.
- Large time jumps produce the same observable drive state as incremental advancement.
- One vs sixteen idle drives shows no O(number_of_drives × T-states) execution path.

### Phase 4 — Media abstraction and read-only physical surface

Deliverables:

- Introduce a medium abstraction below the FD-400 rather than below the DCDD guest port.
- Support the first source-backed raw 8-inch hard-sector image layout read-only.
- Represent disk inserted/ejected and write-protect state separately from image bytes.
- Validate image geometry/size and reject incompatible media explicitly.
- Keep room for future preservation/flux-level media without changing controller APIs.

PASS gate:

- Media can be mounted/ejected without the controller knowing a host pathname.
- Track/sector/physical-byte addressing belongs below the drive electronics.
- Invalid images cannot silently reshape themselves into valid disks.

### Phase 5 — Authentic read path end to end

Deliverables:

- Implement serial/read-electronics state required to reproduce synchronization and New Read Data Available behavior.
- Deliver bytes at source-backed virtual-time intervals.
- Implement the documented effect of the CPU reading too late: overwrite/loss/overrun semantics must follow hardware, not pause the disk for the guest.
- Drive status/data through the real DCDD boards and S-100 `IN` cycles.
- Use arithmetic catch-up for elapsed byte intervals where equivalent.

PASS gate:

- A real 8080 polling loop can select a drive, find a sector and read a known physical record through ports only.
- Byte cadence matches documented timing in Authentic mode.
- Deliberately late service produces the documented failure/result instead of stretching virtual disk time.
- Incremental vs catch-up execution is observationally equivalent.

### Phase 6 — Authentic write path

Deliverables:

- Implement write enable/data-ready behavior and serial write path.
- Implement source-backed write start, byte cadence, sync handling and trim-erase/post-write timing.
- Enforce write protection and illegal/not-ready conditions electrically/statefully.
- Commit modified physical media bytes only through the drive/media layer.
- Define crash-safe host persistence separately from guest electrical completion.

PASS gate:

- Guest code writes a physical record using only DCDD I/O and reads it back through the authentic read path.
- Write-protected media remains byte-identical.
- Early/late/invalid write sequences follow documented controller behavior.
- No write path calls a filesystem-sector shortcut.

### Phase 7 — Interrupts, edge cases and documented revisions

Deliverables:

- Implement controller interrupt enable/disable and raw request behavior only after Phase 0 resolves the wiring.
- Route any request through the existing physical S-100/interrupt architecture; do not fabricate restart opcodes inside the DCDD.
- Add door/media removal, no-drive, drive-address mismatch, track-zero and other source-backed edge behavior.
- Add separately selectable historical timing/revision behavior only where documentation proves a meaningful hardware difference.

PASS gate:

- Interrupt assertion/deassertion occurs on the documented physical event and line.
- CPU interrupt acceptance remains owned by the normal machine path.
- Edge conditions are regression-tested at the public bus/device boundary.

### Phase 8 — Fast Disk timing policy

Deliverables:

- Introduce a timing-policy abstraction shared by the same FD-400/controller state machines.
- Implement `Authentic` and `Fast` policies; optionally expose `Follow machine preference` only if its semantics are explicit and do not conflate host speed with guest hardware speed.
- Collapse mechanical/rotational/data waits to the earliest safe observable transition.
- Preserve required edge/state ordering so polling software cannot deadlock because a transition was optimized away.
- Disable timing-induced byte loss in Fast mode only as a consequence of the accelerated readiness policy, not by bypassing status/data logic.

PASS gate:

- The same boot/read/write guest code works unmodified in both modes.
- Fast mode never calls a guest-facing sector API.
- Logical/error conditions such as no media and write protect remain identical.
- Fast is materially faster than Authentic on disk-heavy workloads.

### Phase 9 — Persistence, chassis editor and media UI

Deliverables:

- Persist the physical DCDD/card configuration through the authoritative S-100 schema.
- Add disk-unit address/configuration and timing-mode persistence with explicit defaults.
- Provide mount/eject/write-protect controls without creating a second hardware authority in UI state.
- Require POWER OFF for physical controller-card topology changes according to the existing chassis rules; define separately which removable-media operations are allowed while powered.
- Reject electrically ambiguous/incomplete configurations explicitly.

PASS gate:

- Save/load round trips preserve the physical assembly and disk settings exactly.
- UI state reflects runtime authority instead of shadowing it.
- Old configurations migrate without accidentally installing disk hardware.

### Phase 10 — Authentic software/bootstrap validation

Deliverables:

- Add preserved test media/software only when redistribution/licensing is acceptable; otherwise document reproducible user-supplied validation inputs.
- Validate a source-backed authentic disk bootstrap path executed by the 8080.
- Validate representative MITS disk software, then CP/M or other compatible software only after the MITS path is trustworthy.
- Add focused regression fixtures that do not make the normal suite dependent on large copyrighted disk images.

PASS gate:

- Authentic boot reaches the disk exclusively through CPU -> S-100 -> DCDD -> FD-400 -> media.
- No loader hook, PC trap, direct memory injection or sector shortcut is required for the authentic path.
- Fast mode boots the same software through the same guest protocol.

### Phase 11 — Performance and closeout

Deliverables:

- Add stable benchmarks/profiling cases for DCDD installed-idle, sector polling, continuous read, continuous write, missed-byte stress and many idle drives.
- Compare against a pinned pre-DCDD baseline on the same host/profile.
- Remove temporary instrumentation after conclusions are captured.
- Update source/test references and hardware-fidelity documentation.
- Record supported/non-supported revisions, formats and behaviors explicitly.

Performance targets are budgets, not permission to weaken fidelity:

- installed but idle DCDD: target less than 2% regression in the representative CPU benchmark;
- active sector polling: target less than 8% host-throughput regression;
- continuous disk transfer: target less than 15% host-throughput regression;
- sixteen idle drives: should be close to one idle drive and must not introduce per-T-state iteration over units.

If an idle installed DCDD causes a material CPU-hot-path regression, stop and fix the event/deadline architecture before adding more functionality.

PASS gate:

- `cargo fmt` on touched Rust code.
- `$env:RUSTFLAGS='-Dwarnings'; cargo test --locked --all-targets` passes.
- Relevant ignored diagnostics pass when CPU/S-100 behavior changed.
- `cargo build --locked --release` passes.
- Manual release-build smoke test covers controller configuration, mount/eject, Authentic and Fast disk workflows.
- No GitHub Actions are launched unless explicitly requested by the user.

## Test strategy by boundary

Prefer tests at the narrowest physical boundary that proves the invariant, then add end-to-end tests only where integration itself is the claim.

Expected long-lived suites include:

```text
88_dcdd_hardware_fidelity.rs
88_dcdd_io_decode.rs
88_dcdd_timing.rs
88_dcdd_read_path.rs
88_dcdd_write_path.rs
88_dcdd_interrupts.rs
fd400_mechanics.rs
fd400_media.rs
fd400_fast_vs_authentic.rs
```

Names may change to match repository conventions. Any new integration-test file must be registered in `docs/TEST_REFERENCE.md` in the same phase.

## First implementation boundary

The first code phase after this plan is approved is **Phase 0, then Phase 1 only**. Do not implement image parsing, boot shortcuts or UI mounting first. The first mergeable code slice should prove that the physical two-board/controller topology can exist in the current S-100 runtime without changing behavior or hot-path cost when idle.

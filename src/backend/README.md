# Emulator backend boundary

RusTair is being prepared for four product-level emulator engines:

1. `RustFast8080` — current instruction-level Rust 8080/Altair implementation.
2. `RustCycleAccurate8080` — T-state/pin-accurate Rust 8080 core, developed independently.
3. `SimhAltair` — Open SIMH `altair` target.
4. `SimhAltairZ80` — Open SIMH `altairz80` target.

## Branch ownership

`agent/machine-backend-abstraction` owns the machine-level contract only. It must not copy or partially reimplement `cpu8080_cycle` from the parallel cycle-accurate branch.

`agent/cycle-accurate-8080-core` owns the Intel 8080 chip model through complete instruction coverage, machine-cycle/T-state timing, pins, READY/Tw, HOLD/HLDA, interrupts, HALT and differential/diagnostic validation. It should stop before UI/backend integration.

A SIMH branch may depend on this backend contract but must keep SIMH-specific FFI/process types behind its own module.

## Integration rule

After the cycle-accurate core and this abstraction are merged to `main`, create a dedicated integration branch. That branch will adapt `Cpu8080Cycle` to the shared Altair hardware and implement `MachineBackend` for `RustCycleAccurate8080`.

Changing engines creates a fresh machine. Live state transfer is explicitly outside the first four-engine selector.

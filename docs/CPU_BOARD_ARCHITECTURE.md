# RusTair CPU-board architecture

## Principle

RusTair models the processor as a physical S-100 CPU board installed in the Altair chassis. The emulator implementation used to execute that board is a separate concern.

The current machine therefore has two distinct axes:

```text
Physical machine                    Emulator implementation
----------------                    -----------------------
MITS 8080 CPU Board                 Adaptive Cycle 8080
        |                            Full / Partial strategies
        v
Intel 8080 @ 2 MHz
        |
        v
S-100 chassis / RAM / serial boards / front panel
```

Full and Partial are internal execution strategies for the same installed MITS 8080 board. Partial is the exact electrical oracle; Full preserves equivalent timing, architectural state, physical card effects, panel duty and synchronization boundaries under its admission proof.

## Current code contract

`CpuModel` identifies the processor carried by a board. Today it contains only `Intel8080`.

`CpuBoard` identifies the physical S-100 CPU board. Today it contains only `Mits8080` and owns the board-level processor mapping and authentic clock rate.

`MachineConfig::s100_hardware` owns the installed slot inventory. `S100HardwareConfig::active_cpu_board()` resolves the CPU-board identity from that inventory. Persistence writes `machine.s100_hardware`; old aggregate keys are read only for migration, and the legacy `machine.cpu_model` selector is ignored because only the MITS 8080 board is supported.

Runtime scheduling resolves the installed CPU board and derives the authentic execution budget from `CpuBoard::clock_hz()`. Host speed selection changes how quickly virtual time advances, not the installed board clock. Unlimited uses the host-time deadline in `run_cpu_frame`; it is not capped by the former fixed budget per repaint.

Do not add dormant processor or board variants merely to reserve names. A new variant belongs in production only when its core/board integration exists.

## Future Z80 board

The intended future design is a historically documented Z80 S-100 CPU board, with the Cromemco ZPU as the leading candidate. The exact board must be selected from primary documentation before implementation so its clocking, S-100 signal adaptation and board-specific behaviour are not guessed.

The future shape should be:

```text
AltairChassis / S-100 bus
|
+-- MITS 8080 CPU Board
|   +-- Intel 8080
|       +-- Adaptive Cycle 8080
|           +-- Full / Partial strategies
|
+-- historical Z80 S-100 CPU Board
    +-- Zilog Z80
        +-- future Rust Z80 core
```

A Z80 CPU board is not a SIMH backend and must not reintroduce the removed SIMH integration.

## Integration requirements for the future Z80 feature

1. Select and document the exact historical S-100 Z80 board from primary sources before writing board-specific behaviour.
2. Implement or integrate a real Z80 CPU core independently of the S-100 chassis.
3. Add `CpuModel::ZilogZ80` only when that core exists.
4. Add the selected `CpuBoard` variant only when its board adapter exists.
5. Keep authentic runtime scheduling driven by the installed CPU board's `clock_hz()`; never reintroduce a global assumption that every machine runs at 2 MHz.
6. Implement the board adapter between Z80 CPU signals and the existing S-100 electrical authority. Front-panel/S-100 state must continue to come from the bus, not from UI-side projections.
7. Preserve the existing chassis, RAM boards, serial boards and front-panel model when swapping CPU boards where historically/electrically compatible.
8. Require POWER OFF for CPU-board replacement. Do not migrate live registers or hidden execution state between boards.
9. Extend the physical S-100 slot codec for the selected board while preserving existing inventory files and legacy Intel-8080 configuration migration.
10. Add engine/board compatibility checks so an 8080-only execution engine cannot be selected for a Z80 board and vice versa.
11. Add Z80-specific CPU state/debugger/disassembly support only as part of the real core integration; do not restore a placeholder `Z80State` in advance.
12. Validate reset, WAIT/READY behaviour, HOLD/HLDA or equivalent bus-master interactions, interrupts, I/O cycles, memory cycles and front-panel observations against the selected board documentation.
13. Validate 8080-compatible software on the Z80 board separately from Z80-specific instruction tests.

## Non-goals until the Z80 core exists

- No `CpuModel::ZilogZ80` placeholder.
- No `CpuState::Z80` placeholder.
- No fake Z80 engine in `EmulationEngine`.
- No disabled Z80 menu item.
- No guessed Cromemco/Tarbell/TDL signal behaviour.
- No SIMH dependency or external-process emulation path.

# MITS 88-SIO interrupt routing fidelity

Status: **IMPLEMENTED — machine-level routing locally validated; physical-configuration persistence/UI awaits the current local checkpoint. Rev 0 external device-ready handshake remains explicitly open.**

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope

This document covers the interrupt-generation and interrupt-routing hardware of the original single-channel MITS 88-SIO. It deliberately separates three different layers that must not be collapsed into one emulator boolean:

1. the serial/device condition that requests service;
2. the software-controlled input/output interrupt-enable flip-flops;
3. the physical wiring that takes the resulting IN or OUT request to the processor interrupt line or to an 88-Vector Interrupt input.

The COM2502 UART core itself is documented in `docs/88_SIO_HARDWARE_FIDELITY.md`.

## 2. Primary sources

Primary evidence used for this block:

- MITS, *88-SIO Serial I/O Board Documentation* (original 1975 documentation):
  `https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIO%20Documentation.pdf`
- MITS, *88-SIOB Rev 1 Schematic*:
  `https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIOB%20Rev%201%20Schematic.pdf`
- MITS, *88-SIO Rev 0 Errata*:
  `https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-SIO%20Serial%20Board/88-SIO%20Rev%200%20Errata.pdf`
- MITS *Computer Notes* Rev-1 software examples, used only to corroborate the later D0/D7 polling convention.

## 3. Software interrupt-enable flip-flops

The even 88-SIO I/O address is status on IN and control on OUT. For interrupt control, only D0 and D1 matter:

| Control bit | Function |
| ---: | --- |
| D0 | input interrupt enable |
| D1 | output interrupt enable |

Therefore:

| D1 | D0 | Enabled request sources |
| ---: | ---: | --- |
| 0 | 0 | none |
| 0 | 1 | input only |
| 1 | 0 | output only |
| 1 | 1 | input + output |

RusTair preserves these as a runtime latch (`sio_interrupt_control`). They are **not** configuration-menu settings: software running on the 8080 changes them by writing the card control address.

## 4. Physical IN / OUT / BH routing

The board exposes separate interrupt request paths for input and output, as well as a combined path historically identified as BH. The requests can be connected to the direct processor interrupt path or to one of the eight vector-interrupt inputs.

RusTair models the electrically relevant result per source:

```rust
pub enum SioInterruptTarget {
    Disconnected,
    Pint,
    Vi0, Vi1, Vi2, Vi3, Vi4, Vi5, Vi6, Vi7,
}

pub struct SioInterruptWiring {
    pub input: SioInterruptTarget,
    pub output: SioInterruptTarget,
}
```

Wiring both input and output to the same destination represents the same resulting connectivity as using the combined BH path for that destination. Keeping the two source results independently configurable also permits historically meaningful arrangements where IN and OUT go to different VI priorities.

## 5. PINT is not VIx

`PINT` is the direct processor interrupt request path. In the current direct Altair interrupt mechanism, the interrupt acknowledge supplies `FFh`, the 8080 `RST 7` opcode.

`VI0..VI7` are different physical wires. An 88-SIO wired to `VI3`, for example, does **not** itself tell the CPU to execute `RST 3` or any other opcode. The request terminates at the raw chassis boundary until a real 88-Vector Interrupt board is present to arbitrate requests and provide the interrupt instruction.

RusTair therefore exposes raw VI state separately and never converts a selected VI level into a fabricated direct CPU restart.

## 6. Rev 1 request sources

For the later Rev-1/internal-ready behavior represented by RusTair, the two request sources are derived from the same internal UART-ready conditions used by the modified D0/D7 status convention:

- input source: D0 control enable set **and** receive-ready condition active;
- output source: D1 control enable set **and** transmitter-buffer-empty condition active.

The physical destination is then applied **after** this source logic:

```text
COM2502 receive ready
        |
        +-- D0 software enable -- IN request -- physical jumper --> PINT / VIx / disconnected

COM2502 TBMT
        |
        +-- D1 software enable -- OUT request - physical jumper --> PINT / VIx / disconnected
```

This separation is why a UART can be requesting service while the CPU sees no PINT: the physical request may be disconnected or routed to an unimplemented 88-VI input.

## 7. Rev 0 is deliberately not fabricated

The original Rev-0 documentation distinguishes the UART RDA/TBMT flags from external **Input device Ready** and **Output device Ready** flip-flops:

- D5 is UART receiver-data-available;
- D1 is UART transmitter-buffer-empty;
- D0 is the active-low input-device-ready state;
- D7 is the active-low output-device-ready state.

The data-channel handshakes reset those external ready flip-flops as part of the device protocol. Therefore an unmodified Rev-0 board must not silently use COM2502 RDA/TBMT as substitutes for the external device-ready interrupt sources.

Until the A/B/C device-ready handshake is fully modeled, RusTair intentionally produces **no Rev-0 interrupt request from COM2502 ready state alone**. This is an explicit fidelity gap rather than a compatibility shortcut.

## 8. Physical configuration and persistence

Interrupt routing is physical board wiring, so it is stored inside the same atomic `SioHardwareConfig` as revision, interface, address, baud and word format.

Current persistence form:

```text
revision,interface,address,baud,data,parity,stops,input_irq,output_irq
```

Example:

```text
rev1,c-tty,00,110,8,none,2,vi3,disconnected
```

The previous seven-field RusTair v4 form remains accepted. It preserves the old card fields and migrates only the newly explicit routing to `PINT/PINT`, matching the previous RusTair behavior. An eight-field half-configuration is rejected.

The `PINT/PINT` default is a **RusTair migration default**, not a claim about a universal MITS factory jumper arrangement.

## 9. Fast versus Cycle

Both engines receive the same `SioHardwareConfig` through the backend-neutral hardware configuration path.

Fast may observe interrupt requests at instruction boundaries because its CPU implementation is instruction-level, but the source condition and total serial T-state timing are the same physical model.

Cycle observes the same request through the shared S-100 chassis while executing exact CPU T-states. A raw VI request remains raw in both engines.

## 10. Regression coverage

Machine-level tests protect:

- input request routed to VI3 does not assert PINT;
- output request routed to PINT does assert the direct processor request;
- input and output may simultaneously occupy different VI levels;
- Rev 0 does not fabricate device-ready interrupts from COM2502 RDA/TBMT.

Configuration tests protect:

- PINT and VI targets are distinct states;
- legacy seven-field SIO hardware persistence migrates without losing card configuration;
- partial eight-field configuration is rejected;
- Fast and Cycle preserve the same interrupt wiring inside `SioHardwareConfig`.

UI source guardrails protect:

- input and output routing selectors exist;
- the selectors live inside the POWER-OFF physical configuration block;
- the UI explains D0/D1 runtime enables versus physical routing;
- the UI explicitly states the raw VI boundary and the Rev-0 handshake gap.

## 11. User validation

### 11.1 Configuration persistence

1. POWER OFF.
2. Select MITS 88-SIO, Rev 1.
3. Set `Input IRQ source` to `VI3`.
4. Set `Output IRQ source` to `Disconnected`.
5. Change Fast ↔ Cycle while still powered off.
6. Re-open `Configuration -> Serial board`.
7. Expected: both selections remain exactly unchanged.
8. Exit/restart RusTair and inspect the same menu.
9. Expected: both selections remain unchanged after persistence reload.

A regression is present if an engine switch or application restart silently returns the wiring to PINT/PINT.

### 11.2 Direct PINT versus VI

Using Rev 1 and a debugger/test program that enables the corresponding 88-SIO D0/D1 control bit:

1. Route the active source to a VI level.
2. Cause its ready condition.
3. Expected: the CPU must **not** take the direct `RST 7` interrupt.
4. POWER OFF and route the same source to PINT.
5. Repeat the ready condition with CPU interrupts enabled.
6. Expected: the direct interrupt path is now visible and the existing Altair PINT acknowledge mechanism supplies `FFh` / `RST 7`.

A regression is present if selecting `VI3` makes the CPU jump directly to `0018h` or to `0038h` without an 88-VI board.

### 11.3 Rev 0 guardrail

1. POWER OFF and select Rev 0.
2. Route input to PINT.
3. POWER ON and set D0 input-interrupt enable.
4. Inject/receive a character so COM2502 RDA becomes active.
5. Until external device-ready handshake support is enabled by the later closeout, expected: COM2502 RDA alone does **not** assert the Rev-0 PINT source.

This is intentionally a temporary negative validation. The final Rev-0 validation will replace it once the documented external ready flip-flops are modeled.

## 12. Remaining work

This interrupt-routing block is complete for Rev 1 after local validation of the configuration/UI layer.

The remaining 88-SIO blocker is the physical device-ready/handshake path, particularly:

- original Rev-0 input-device-ready flip-flop;
- original Rev-0 output-device-ready flip-flop;
- effects of DATA IN / DATA OUT on those latches;
- which external connector/interface signals drive or consume those ready states on the A/B/C variants;
- resulting Rev-0 interrupt behavior.

That work must be based on the original board schematic/interface documentation and must not borrow MC6850 CTS/DCD/RTS semantics from the later 88-2SIO.

# MITS 88-2SIO physical address and baud straps

Status: **UI/PERSISTENCE/DECODER LOCALLY GREEN — readdressed authentic bootstrap and Fast debugger-wait isolation implemented, next local validation pending.**

Parent hardware document: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope and fidelity claim

This document covers the two options that the March 1977 MITS 88-2SIO manual identifies as being under **hardware control**:

1. the I/O address-select jumpers A2-A7;
2. the baud-rate-select wiring for each MC6850/ACIA channel.

The goal is not merely to let the user type arbitrary port numbers or baud rates. RusTair models the restrictions of the physical board:

- the address decoder selects one block of four consecutive I/O addresses;
- the base is aligned to a multiple of four because A0 and A1 are not part of the board-select jumpers;
- A0/A1 select register/data and first/second ACIA inside the selected block;
- the block containing FFh is not offered because MITS identifies FFh as the front-panel sense-switch address and says it should not be used for the 88-2SIO;
- each ACIA has one selected physical clock tap from the eight rates printed on the board;
- the selected tap is not the complete baud-rate decision: the MC6850 CR1:CR0 /1, /16 or /64 divider remains software controlled;
- changing these straps from the normal application UI requires POWER OFF, because RusTair treats them as physical jumper changes rather than runtime preferences;
- software bootstraps that contain immediate 8080 IN/OUT port operands must name the block actually selected by A2-A7. RusTair does not keep a hidden `10h/11h` compatibility alias when the card is moved.

Analog oscillator tolerance, propagation delay through individual TTL gates and wire capacitance are outside this digital strap-level claim.

## 2. Primary hardware evidence

### 2.1 MITS 88-2SIO manual — hardware-controlled options

Primary source:

**MITS, _Altair 88-2-SIO Documentation_, reprinted March 1977**, Theory of Operation, manual pages 1-2 through 1-5.

Archive:

https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

Manual page 1-2 identifies address select and baud-rate select as the two hardware-controlled configuration choices on the board.

### 2.2 Address select: A2-A7 choose a four-address block

Manual page 1-4 explains that six address-select lines provide 64 possible selections in increments of four. A0 and A1 then select the register/port inside the chosen block.

The effective decode is:

| A1 | A0 | Offset | Function |
| ---: | ---: | ---: | --- |
| 0 | 0 | +0 | first ACIA control on OUT / status on IN |
| 0 | 1 | +1 | first ACIA data |
| 1 | 0 | +2 | second ACIA control on OUT / status on IN |
| 1 | 1 | +3 | second ACIA data |

MITS gives **68 decimal** as an explicit example. 68 decimal is `44h`, so the board appears at:

```text
44h  first ACIA control/status
45h  first ACIA data
46h  second ACIA control/status
47h  second ACIA data
```

The manual describes blocks from 0-3 through 252-255, then warns that address 255 is the front-panel sense-switch address and should not be used. RusTair therefore treats `FCh-FFh` as an invalid installable 88-2SIO block rather than permitting an address collision with the front panel.

### 2.3 Baud taps and the MC6850 divider

Manual pages 1-4/1-5 describe the normal incoming clock as 16 times the labelled baud rate. The board exposes the following eight physical selections:

- 110
- 150
- 300
- 1200
- 1800
- 2400
- 4800
- 9600

For the normal labelled rate the MC6850 is programmed for `/16`.

MITS also documents five additional effective rates using `/64`:

| Selected board tap | MC6850 divider | Effective baud |
| ---: | ---: | ---: |
| 110 | /64 | 27.5 |
| 150 | /64 | 37.5 |
| 300 | /64 | 75 |
| 1800 | /64 | 450 |
| 2400 | /64 | 600 |

The relationship is expected because the physical tap supplies a clock at sixteen times its printed baud value and `/64` therefore divides the printed value by four.

## 3. RusTair physical configuration types

`src/config/two_sio.rs` owns the physical installation description rather than the endpoint UI.

### 3.1 Physical baud tap

```rust
pub enum TwoSioBaudTap {
    Baud110,
    Baud150,
    Baud300,
    Baud1200,
    Baud1800,
    Baud2400,
    Baud4800,
    Baud9600,
}
```

The enum intentionally does not contain arbitrary modern rates such as 19,200 or 115,200. Those are not straps on this MITS board.

### 3.2 Address block

```rust
pub const fn try_new(base: u8) -> Option<Self> {
    if base & 0x03 != 0 || base > 0xf8 {
        None
    } else {
        Some(Self { base })
    }
}
```

This encodes two physical facts:

- A2-A7 imply four-address alignment;
- `FCh` is rejected so the board cannot claim the front-panel `FFh` port.

The four decoded ports are derived, never stored as unrelated values:

```rust
pub const fn port0_status(self) -> u8 { self.base }
pub const fn port0_data(self) -> u8 { self.base + 1 }
pub const fn port1_status(self) -> u8 { self.base + 2 }
pub const fn port1_data(self) -> u8 { self.base + 3 }
```

### 3.3 Complete board straps

```rust
pub struct TwoSioStraps {
    pub address: TwoSioAddressBlock,
    pub port0_baud: TwoSioBaudTap,
    pub port1_baud: TwoSioBaudTap,
}
```

The defaults preserve RusTair's previously hard-coded physical installation:

```text
base       = 10h
Port 0 tap = 110
Port 1 tap = 9600
```

Keeping these defaults means existing software/bootstrap behavior does not change merely because the jumpers have become explicit hardware state.

`MachineConfig` owns these straps and persistence writes them independently of the selected serial board. This represents a physical 88-2SIO card whose jumpers remain what the user set even if the application temporarily selects another serial board or recreates the Fast/Cycle backend.

## 4. Dynamic board decoder

`src/machine/io_devices.rs` stores `TwoSioStraps` as board state.

The production decoder no longer tests literal `10h`, `11h`, `12h`, `13h`. It asks the selected address block for the A1:A0 offset:

```rust
fn two_sio_offset(&self, port: u8) -> Option<u8> {
    if self.serial_board != SerialBoard::TwoSio88 { return None; }
    self.two_sio_straps.address.offset(port)
}
```

Input selection is then the physical A1:A0 decode:

```rust
match self.two_sio_straps.address.offset(port) {
    Some(0) => self.two_sio[0].read_status(),
    Some(1) => self.two_sio[0].read_data(),
    Some(2) => self.two_sio[1].read_status(),
    Some(3) => self.two_sio[1].read_data(),
    _ => S100_OPEN_BUS_VALUE,
}
```

The same offset controls OUT writes.

This is important because address strapping must move **all card behavior together**. RusTair therefore derives from the same block:

- IN/OUT register decode;
- S-100 open-bus ownership;
- the 88-2SIO PRDY/Tw input wait;
- debugger data-port recognition;
- I/O trace addresses;
- endpoint Port 0 / Port 1 data-address labels at the machine boundary.

There is no separate hidden wait-state address table that can remain at 10h-13h after the board is moved.

## 5. Physical baud ownership

When straps change, `IoDevices` rebuilds both `TwoSioPort` channel objects with their selected physical taps. In-flight state is intentionally discarded: moving board jumpers is a power-off hardware reconfiguration, not a runtime baud-register write.

The existing board clock remains authoritative:

```rust
let numerator_per_t_state = u64::from(self.baud_tap.baud()) * 16;
let threshold = u64::from(cpu_clock_hz) * u64::from(divider);
```

Thus the same physical 110 tap can produce different effective rates solely through the real MC6850 control register:

- `/16` -> 110 baud;
- `/64` -> 27.5 baud;
- `/1` -> the corresponding high-rate clock interpretation supported by the ACIA model.

Endpoint speed settings must not overwrite this card clock.

## 6. Backend, persistence, UI and software mapping

Both backend families expose the same backend-neutral strap contract:

```rust
fn configure_two_sio_straps(&mut self, straps: TwoSioStraps) -> BackendResult<()>;
fn two_sio_straps(&mut self) -> BackendResult<TwoSioStraps>;
```

Fast delegates to its `AltairMachine`; Cycle delegates through its CPU-independent `AltairChassis`. In both cases the actual decoder/clock state remains in `AltairBus`/`IoDevices`.

When an emulation engine is recreated while POWER is OFF, the application explicitly reapplies the configured physical straps:

```rust
self.machine.configure_serial_board(self.config.machine.serial_board);
self.machine.configure_two_sio_straps(self.config.machine.two_sio_straps);
```

This prevents switching Fast <-> Cycle from silently returning a readdressed card to `10h` or resetting its baud taps.

Persistence stores:

```text
machine.two_sio.address_base
machine.two_sio.port0_baud
machine.two_sio.port1_baud
```

Old configuration files that lack these keys retain the historical RusTair defaults `10h / 110 / 9600`. An invalid edited base such as `FCh` is rejected during parsing and cannot override the safe default.

The normal user UI is under **Configuration -> Serial board -> Physical 88-2SIO straps**. It exposes:

- one valid aligned A2-A7 block from `00h-03h` through `F8h-FBh`;
- Port 0 physical baud tap;
- Port 1 physical baud tap.

`FCh-FFh` is not presented. All three controls are disabled while POWER is ON, and the UI explains that they represent moving real jumpers on the board.

The endpoint wiring labels are generated from the actual straps, so moving the card to `44h` changes the visible Port 0 label to `44h/45h` and Port 1 to `46h/47h` rather than leaving stale `10h-13h` text.

### 6.1 Authentic BASIC bootstrap follows A2-A7

The MITS/default RusTair 88-2SIO bootstrap template is preserved verbatim as:

```text
3E 03 D3 10  3E 11 D3 10  21 AE 0F 31 1A 00 DB 10
0F D0 DB 11  BD C8 2D 77  C0 E9 0B 00
```

Those `10h/11h` operands describe a card installed at the default block. A physical 8080 has no way to discover that a card has been moved to `44h`; software must contain the new immediate port values.

`BootstrapDefinition::resolved_bytes()` therefore changes **only four bytes** for a readdressed 88-2SIO:

| Bootstrap address | Instruction role | Default | 44h block |
| ---: | --- | ---: | ---: |
| `0003h` | operand of master-reset `OUT` | `10h` | `44h` |
| `0007h` | operand of framing-control `OUT` | `10h` | `44h` |
| `000Fh` | operand of status `IN` | `10h` | `44h` |
| `0013h` | operand of data `IN` | `11h` | `45h` |

All opcodes, control values, addresses, return-vector bytes and checksum-loader logic remain unchanged. The assisted front-panel loader deposits these resolved bytes through the real EXAMINE/DEPOSIT path, and the row-by-row teaching UI displays the same resolved bytes and dynamic mnemonics (`OUT $44`, `IN $44`, `IN $45`).

This is deliberately **not** implemented by letting the old `10h/11h` program alias the card at its new address. After readdressing, `10h-13h` remain open bus as required by the S-100 decoder model.

## 7. Fast versus Cycle Accurate

### Fast

Fast uses the configured address decoder for guest IN/OUT and adds the 88-2SIO's documented one-Tw input penalty to total elapsed T-states only when the selected physical block responds.

If the board is moved from `10h` to `44h`, an `IN 10h` is now unmapped/open bus and receives no 88-2SIO wait. An `IN 44h-47h` receives the card's wait.

Fast does not claim pin-exact placement of Tw within the instruction.

A debugger-triggered register read is not an 8080 instruction even when it intentionally performs the register's real side effect. `NativeMachineBackend::debugger_input_port()` therefore discards Fast wait debt before and after that host observation. The next guest instruction cannot inherit the debugger's 88-2SIO `+1T` PRDY wait.

### Cycle Accurate

Cycle uses the same decoder to decide whether the card pulls its READY contribution during the actual I/O machine cycle. Therefore moving A2-A7 also moves the real T2->Tw->T3 behavior seen in Bus Teacher.

The selected baud tap feeds the same independent chassis-clock model in both engines.

## 8. Regression evidence

### `src/config/two_sio.rs`

- `address_strap_is_one_aligned_four_port_block`
  - protects the MITS A2-A7/A1:A0 decode and the manual's 68-decimal example;
- `address_strap_rejects_unaligned_and_front_panel_conflicting_blocks`
  - prevents arbitrary bases and the `FCh-FFh` collision;
- `default_straps_preserve_existing_installation`
  - protects 10h/110/9600 backwards-compatible physical defaults;
- `physical_taps_are_the_eight_mits_silkscreen_rates`
  - prevents modern arbitrary rates from being silently presented as board straps.

### `src/machine/io_devices.rs`

- `physical_address_strap_moves_decoder_waits_and_open_bus_together`
  - moves the board to `44h-47h`;
  - verifies the new block responds and receives PRDY wait;
  - verifies the old `10h-13h` block becomes open bus and no longer waits;
  - verifies `FFh` cannot become a 2SIO wait source;
- `baud_straps_are_independent_per_acia`
  - sets Port 0 to 300 and Port 1 to 9600;
  - starts RX frames on both;
  - proves the 9600-baud RDR fills while the 300-baud channel is still shifting.

### backend/persistence/UI guards

- `both_backends_project_the_same_physical_two_sio_straps`
  - requires Fast and Cycle to decode the same readdressed board;
- persistence round-trip tests
  - preserve address base and both baud taps;
  - reject an invalid persisted `FCh` base;
- `tests/two_sio_strap_ui.rs`
  - requires engine recreation to reapply straps;
  - requires the application POWER-OFF guard;
  - requires the UI controls themselves to be disabled with POWER ON;
  - forbids fixed `88-2SIO Port 0 [10h/11h]` / `Port 1 [12h/13h]` labels;
  - requires ASR-33, Text Terminal, TCP and COM connection surfaces to receive the same physical straps;
  - guards the `FCh-FFh` exclusion and user explanation.

### authentic loader and debugger isolation

`src/app/authentic_loader.rs` now contains regressions that:

- preserve the canonical published/default 10h/11h byte image;
- prove a 44h installation changes only bootstrap offsets `0003h`, `0007h`, `000Fh`, `0013h`;
- deposit 44h/45h operands through the real front-panel path on both Fast and Cycle;
- execute the readdressed bootstrap and prove I/O occurs at 44h/45h with zero legacy 10h/11h activity.

`tests/two_sio_debugger_wait_isolation.rs` requires a Fast debugger `IN 10h` followed by a guest NOP to consume exactly the NOP's normal **4 T-states**, never 5.

## 9. How the user can validate it

These steps are deliberately written for a normal RusTair user, not for a developer running unit tests.

### 9.1 Address straps / decoder / open bus

1. Start RusTair and ensure the Altair is **POWER OFF**.
2. Open **Configuration -> Serial board** and select **MITS 88-2SIO**.
3. Under **Physical 88-2SIO straps**, choose address block **44h-47h**. This is the 68-decimal example used in the MITS manual.
4. Verify the menu now reports:
   - Port 0: `44h` status/control, `45h` data;
   - Port 1: `46h` status/control, `47h` data.
5. Verify all endpoint cable labels use the same dynamic addresses.
6. POWER ON.
7. Open **I/O Inspector**.
8. Inspect/read `44h-47h`; they must be the two MC6850 register pairs.
9. Inspect/read `10h-13h`; they must now behave as unmapped S-100 open bus (`FFh`) rather than aliasing the card.
10. Switch to **Cycle Accurate** only after POWER OFF, restore POWER ON, and use **T-STATE TEACHER** while executing an `IN 44h` or `IN 45h`. The input cycle must contain the documented single Tw.
11. Execute `IN 10h`. It must not receive the 88-2SIO Tw because the card no longer decodes that address.

**Failure indicators:** old and new blocks both respond; waits remain at 10h-13h; UI labels disagree with I/O Inspector; Fast and Cycle see different blocks; `FFh` becomes selectable as part of the card.

### 9.2 POWER-OFF interlock

1. With the 88-2SIO installed, POWER ON the Altair.
2. Open **Configuration -> Serial board -> Physical 88-2SIO straps**.
3. Address and both baud selectors must be disabled.
4. The menu must explain that POWER OFF is required because the controls represent physical jumpers.
5. POWER OFF; the controls must become selectable immediately.

A live jumper change while POWER is ON is a regression even if the backend happens to survive it.

### 9.3 Persistence

1. POWER OFF.
2. Select block `44h-47h`, Port 0 tap `300`, Port 1 tap `4800`.
3. Close RusTair normally after the configuration has been persisted.
4. Reopen RusTair.
5. Return to **Configuration -> Serial board**.
6. Verify the same block and both taps are present.
7. With POWER OFF switch Fast <-> Cycle and verify the values do not revert to `10h / 110 / 9600`.

### 9.4 Baud timing

A practical digital timing validation is:

1. POWER OFF.
2. Set Port 0 physical tap to **110**.
3. POWER ON and program the Port 0 MC6850 with control `11h`: `/16`, 8N2.
4. Present one receive character. The 11-bit frame should reach RDR after approximately **100 ms** (`11 / 110`).
5. POWER OFF, change only Port 0 physical tap to **300**, POWER ON, program the same `11h` control word and repeat. The frame should now take approximately **36.67 ms** (`11 / 300`).
6. For the slow-rate cross-check, select 110 and program `/64`. The effective rate is 27.5 baud, so an 11-bit frame takes approximately **400 ms**.

I/O Inspector can show when RDRF changes. A real COM/loopback endpoint or an external logic analyzer may also be used once connected, but is not required for the basic digital validation.

The key observation is that changing the **physical board tap**, not an endpoint presentation-speed setting, changes the ACIA clock.

### 9.5 Authentic BASIC bootstrap after readdressing

This is the most direct user-visible proof that address strapping is not merely a UI label.

1. POWER OFF and configure the 88-2SIO for **44h-47h** with Port 0 at **110 baud**.
2. POWER ON, STOP the machine, and open **File -> Microsoft 4K BASIC 3.2 -> Authentic Load — paper tape**.
3. In **Machine / loader status**, verify RusTair reports status `44h` / data `45h`.
4. Expand **Operator-assisted row-by-row bootstrap entry**.
5. Inspect these data rows:
   - address `0003h` must contain `44h`;
   - address `0007h` must contain `44h`;
   - address `000Fh` must contain `44h`;
   - address `0013h` must contain `45h`.
6. Hover the corresponding instruction rows. The teaching text must say `OUT $44`, `IN $44`, and `IN $45`, not 10h/11h.
7. Use **Install bootstrap via front panel** or enter it row by row. The Bootstrap RAM indicator must become verified for the current straps.
8. Open **I/O Inspector**, mount the BASIC tape, set the required sense switches and run the authentic loader.
9. The polling/read activity must appear at `44h` and `45h`. `10h` and `11h` must remain unused/open bus.
10. POWER OFF, return the board to `10h-13h`, and reopen the loader. The four operand bytes must return to the canonical `10h,10h,10h,11h` values.

A regression is present if the loader still deposits 10h/11h while the card is at 44h, if old ports alias the new card, or if the teaching table disagrees with the actual I/O trace.

### 9.6 Fast debugger wait isolation

This behavior is mainly protected automatically because a host/debugger observation should not alter guest time. A user can nevertheless sanity-check it in **Fast 8080** with instruction tracing enabled:

1. Install the 88-2SIO at its default `10h-13h` block.
2. Put a single `NOP` (`00h`) at the current stopped PC.
3. Use a debugger/I/O inspection action that performs a real read of 88-2SIO status at `10h`.
4. Single-step the NOP.
5. The instruction must account for its normal **4 T-states**. It must not appear as a 5-T-state NOP merely because the preceding host observation encountered the 88-2SIO PRDY wait.

The authoritative regression for this boundary remains `tests/two_sio_debugger_wait_isolation.rs` because not every UI inspection surface performs a side-effecting debugger `IN`.

## 10. Validation history

- The 88-TYA Reader Control / physical RX-line block passed the full normal local `cargo test` suite on 2026-08-31.
- The CTS/DCD/BREAK External COM block passed its focused tests and the full normal local `cargo test` suite on 2026-08-31, as reported by the user.
- The physical decoder/tap core, backend contract and persistence layer passed their focused tests and the full normal local `cargo test` suite on 2026-08-31, as reported by the user.
- The user-facing POWER-OFF strap controls, dynamic labels, engine preservation and all four endpoint label call sites passed the focused/full normal local suite on 2026-08-31; the user reported **all green** through commit `cde13a9a3b1697c59755b2596196cd4ae8aa144b`.
- Readdressed authentic-bootstrap generation/execution and Fast debugger wait isolation were implemented after that green checkpoint and require the next focused/full local validation.
- GitHub Actions were not run.

## 11. Remaining work before 88-2SIO strap PASS

1. Locally validate the new readdressed bootstrap tests and Fast debugger-wait isolation test.
2. Run or spot-check the user-visible 44h bootstrap procedure in section 9.5 when convenient.
3. If focused/full tests remain green, mark this **physical straps** sub-block PASS in this document and the parent 88-2SIO ledger.

These items are specifically for the strap sub-block. Separate remaining 88-2SIO fidelity questions such as system-level interrupt routing and non-COM BREAK/interface boundaries are tracked in the parent hardware closeout and are not hidden by a strap PASS.

## 12. Primary references

### MITS 88-2SIO

MITS, _Altair 88-2-SIO Documentation_, reprinted March 1977.

https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

Sections used:

- manual page 1-2: address select and baud-rate select are the hardware-controlled options;
- manual page 1-4: six A2-A7 address straps, four-address blocks, A0/A1 register/port selection, address-68 example, warning about address 255;
- manual pages 1-4/1-5: normal /16 board clocks, eight physical baud selections and five /64 derived low rates.

### Motorola MC6850

Motorola Semiconductor Products Inc., _MC6800 Microcomputer System Design Data_, 1976, MC6850 section.

https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf

Used for the ACIA CR1:CR0 clock-divider semantics that operate downstream of the physical MITS baud tap.

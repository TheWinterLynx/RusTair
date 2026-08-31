# MITS 88-2SIO physical address and baud straps

Status: **CORE IMPLEMENTED — backend/UI/persistence wiring and local validation pending.**

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
- the selected tap is not the complete baud-rate decision: the MC6850 CR1:CR0 /1, /16 or /64 divider remains software controlled.

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

## 4. Dynamic board decoder

`src/machine/io_devices.rs` now stores `TwoSioStraps` as board state.

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

## 6. Fast versus Cycle Accurate

### Fast

Fast uses the configured address decoder for guest IN/OUT and adds the 88-2SIO's documented one-Tw input penalty to total elapsed T-states only when the selected physical block responds.

If the board is moved from `10h` to `44h`, an `IN 10h` is now unmapped/open bus and receives no 88-2SIO wait. An `IN 44h-47h` receives the card's wait.

Fast does not claim pin-exact placement of Tw within the instruction.

### Cycle Accurate

Cycle uses the same decoder to decide whether the card pulls its READY contribution during the actual I/O machine cycle. Therefore moving A2-A7 also moves the real T2->Tw->T3 behavior seen in Bus Teacher.

The selected baud tap feeds the same independent chassis-clock model in both engines.

## 7. Regression evidence

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

## 8. How the user can validate it

### 8.1 Current checkpoint

At this exact checkpoint the physical decoder/tap core exists, but the Configuration UI and persistence layer do not yet expose `TwoSioStraps`. Therefore a normal user cannot yet perform the complete strap-change experiment without a test/debug build.

That limitation is deliberate and is why this document is **not** marked PASS. The automated regressions above are the current validation path.

### 8.2 Manual address validation once UI wiring lands

The final UI validation procedure will be:

1. POWER OFF the Altair.
2. Select **MITS 88-2SIO**.
3. Set the A2-A7/base-address block to **44h** (68 decimal, the example used by MITS).
4. POWER ON.
5. Open **I/O Inspector**.
6. Read/inspect `44h`, `45h`, `46h`, `47h` and confirm they are the two ACIA register pairs.
7. Inspect `10h-13h`; they must now resolve as S-100 open bus (`FFh`) rather than aliasing the board.
8. In **Cycle Accurate**, execute an `IN 44h` or `IN 45h` while Bus Teacher is visible. The I/O machine cycle must contain the documented Tw.
9. Execute `IN 10h`. It must not receive the 88-2SIO Tw because the card is no longer selected there.

A regression is present if either the old and new blocks alias each other, the wait remains at the old block, or the selected block does not move all four functions together.

### 8.3 Manual baud validation once UI wiring lands

A practical visible test is:

1. POWER OFF.
2. Set Port 0 physical tap to **110**.
3. POWER ON and write control `11h` to Port 0: `/16`, 8N2.
4. Present one receive character. The 11-bit frame should reach RDR after approximately **100 ms** (`11 / 110`).
5. POWER OFF, change only Port 0 physical tap to **300**, POWER ON, write the same `11h` control word and repeat. The frame should now take approximately **36.67 ms** (`11 / 300`).
6. For the slow-rate cross-check, select 110 and program `/64`. The effective rate is 27.5 baud, so an 11-bit frame takes approximately **400 ms**.

I/O Inspector can show when RDRF changes. A real COM/loopback endpoint or an external logic analyzer may also be used once connected, but is not required for the basic digital validation.

The key observation is that changing the **physical strap**, not an endpoint presentation-speed setting, changes the ACIA clock.

## 9. Validation history

- The preceding 88-TYA Reader Control / physical RX-line block passed the full normal local `cargo test` suite on 2026-08-31.
- The subsequent CTS/DCD/BREAK External COM block also passed its focused tests and the full normal local `cargo test` suite on 2026-08-31, as reported by the user.
- This physical-straps core was implemented after those green checkpoints and still requires local compilation/focused/full-suite validation.
- GitHub Actions were not run.

## 10. Remaining work before strap PASS

1. Carry `TwoSioStraps` through the backend-neutral configuration contract.
2. Store the selected straps in `MachineConfig`.
3. Persist base/Port0 tap/Port1 tap in RusTair configuration files.
4. Add POWER-OFF-only Configuration UI controls.
5. Replace hard-coded 10h-13h endpoint labels with the selected block.
6. Add shared Fast/Cycle public-backend regressions for readdressing.
7. Perform the manual user validations described above.
8. Update this document from CORE IMPLEMENTED to VALIDATED/PASS only after the focused and full local suites are green.

## 11. Primary references

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

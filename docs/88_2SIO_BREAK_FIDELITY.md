# MITS 88-2SIO / MC6850 BREAK fidelity

Status: **IMPLEMENTED — local focused/full validation pending.**

Parent document: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Scope and fidelity claim

This document covers the MC6850 transmit BREAK function as installed on the MITS 88-2SIO.

The fidelity boundary is deliberately electrical:

- CR6:CR5=`11` forces a continuous spacing/BREAK level on the physical Tx Data output;
- BREAK is not represented as `00h`, NUL, or any other fabricated byte;
- the MC6850 transmitter holding/shift-register machinery continues to follow its normal clocking and TDRE rules while Tx Data is overridden;
- a character frame that overlaps BREAK is not a valid downstream character and therefore must not enter RusTair's completed-wire byte queue;
- releasing BREAK does not repair a frame already corrupted by the spacing override;
- the next complete post-BREAK frame can be delivered normally.

External COM can express the electrical condition through the host serial driver's native BREAK control. Byte-only endpoints stop at the byte/line boundary and do not invent a character to approximate BREAK.

## 2. Physical hardware

The Motorola MC6850 contains a Transmit Data Register (TDR), a transmit shift register and transmitter-control logic. The MITS 88-2SIO exposes the resulting serial Tx Data line through its interface circuitry.

Motorola defines CR6:CR5 as follows:

| CR6 | CR5 | RTS | TX-empty IRQ | Tx Data behavior |
| ---: | ---: | --- | --- | --- |
| 0 | 0 | LOW | disabled | normal transmission |
| 0 | 1 | LOW | enabled | normal transmission |
| 1 | 0 | HIGH | disabled | normal transmission |
| 1 | 1 | LOW | disabled | continuous BREAK / spacing level |

The important point is that BREAK is specified as a level on **Transmit Data Output**. Motorola separately documents the normal TDR-to-shift-register transfer and identifies CTS HIGH, not BREAK, as the condition that inhibits TDRE/TDR transfer behavior.

The expanded MC6850 block diagram also places transmitter-control logic at the Tx Data output path rather than defining BREAK as a byte written into the TDR.

## 3. Primary evidence

### 3.1 Motorola MC6850

Primary semiconductor reference:

Motorola Semiconductor Products Inc., *MC6800 Microcomputer System Design Data*, 1976, MC6850 section.

https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf

Relevant statements:

- writing the TDR makes TDRE LOW;
- when the transmitter is idle, TDR transfers to the shift register within one bit time;
- when a character is already transmitting, the next TDR character starts immediately after the previous character completes;
- the TDR-to-shift transfer makes TDRE HIGH again;
- CTS HIGH inhibits TDRE / transmitter transfer;
- CR6:CR5=`11` transmits a BREAK level (space) on Tx Data and disables transmit interrupt.

A later Motorola MC6850/MC68A50/MC68B50 data sheet contains the same register/transmit descriptions and an expanded block diagram showing the transmit data/shift/control path.

### 3.2 MITS 88-2SIO

MITS, *Altair 88-2-SIO Documentation*, reprinted March 1977.

https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

The 88-2SIO control-register description identifies CR6:CR5=`11` as the BREAK selection and says it produces a break level on the transmit data output. The board uses the MC6850 transmitter control directly; it does not define a byte encoding for BREAK.

The MITS scan/OCR is not used to override Motorola's silicon-level RTS polarity definition where typesetting/OCR is ambiguous. RusTair follows the MC6850 electrical definition: BREAK mode drives RTS LOW.

## 4. Pre-closeout RusTair behavior

Before this block, RusTair already exposed:

```rust
pub(crate) fn break_active(&self) -> bool {
    self.control & 0x60 == 0x60
}
```

and External COM projected that state using the host serial driver's real BREAK control.

However, `TwoSioPort::transmitter_bit_boundary()` still completed every internal TSR byte into `wire_tx`, even if Tx Data was physically held in BREAK for part or all of that frame. That allowed a byte-only endpoint to receive a character which could not have existed as a valid serial frame on the physical wire.

## 5. RusTair model

`src/machine/two_sio.rs` now separates two truths:

1. **internal transmitter progress** — TDR/TSR/TDRE;
2. **valid completed serial character on the external wire** — `wire_tx`.

Each active transmitter frame carries one board-level condition:

```rust
tx_frame_corrupted_by_break: bool
```

This flag becomes true if BREAK overlaps the frame at any point.

### 5.1 Entering BREAK mid-frame

`write_control()` detects the physical transition into BREAK. If a TSR character is already active, the current frame is marked corrupted immediately.

This matters because control-register writes are asynchronous with respect to the next emulated bit boundary: Tx Data changes when the control mode changes, not one bit later merely for implementation convenience.

### 5.2 TDR and TSR continue

`transmitter_bit_boundary()` does **not** stop when BREAK is active.

The normal sequence remains:

```text
CPU writes TDR
    ↓
next transmitter bit boundary
    ↓
TDR -> TSR
    ↓
TDRE becomes HIGH
    ↓
TSR consumes configured frame clocks
    ↓
TSR completion / possible next TDR promotion
```

BREAK only changes whether the completed frame can be called a valid external character.

### 5.3 Wire completion

At TSR completion:

```rust
if !corrupted {
    self.wire_tx.push_back(byte);
}
```

A BREAK-corrupted internal byte therefore disappears at the electrical character boundary instead of being delivered as a false byte.

### 5.4 Releasing BREAK

Clearing CR6:CR5 from `11` does not clear the current frame's corruption flag. A frame already damaged by a spacing interval remains invalid.

When the next TDR character is promoted after BREAK has been released, it starts with a clean per-frame state and can become a normal `wire_tx` byte after one complete valid frame.

## 6. Fast versus Cycle Accurate

The BREAK logic is card hardware and lives below both processor engines.

### Fast

Fast advances the same 88-2SIO transmitter from reconstructed elapsed T-states. TDRE timing remains tied to TDR->TSR transfer, and a BREAK-overlapped frame is suppressed only at the completed-wire boundary.

### Cycle Accurate

Cycle advances the same card by exact CPU-clock T-state samples. The serial oscillator continues through STOP/RESET/HOLD according to the already validated chassis-clock rules. BREAK therefore cannot accidentally become an engine-specific TX pause.

The public regression `both_engines_keep_tdr_tsr_clocking_under_break_without_fabricating_wire_bytes` requires the same result from both engines while the CPU is STOPped and the chassis clock continues.

## 7. Peripheral and host boundary

### 7.1 External COM

External COM is capable of expressing an electrical serial BREAK. RusTair already uses:

```rust
self.external_com.port.set_break_active(break_active);
```

The COM worker maps this to the host serial API's native `set_break()` / `clear_break()` operations.

No byte is inserted to represent BREAK.

### 7.2 External TCP

Raw TCP is byte-stream transport and has no native asynchronous serial BREAK condition.

RusTair therefore does **not** send NUL/00h or another escape byte when the MC6850 enters BREAK. A future explicit framing protocol could carry out-of-band line state, but plain TCP must remain a byte-only endpoint.

### 7.3 Text Terminal

The internal Text Terminal currently consumes completed serial characters, not a bit-level electrical line. It therefore receives no fabricated character while BREAK is active.

### 7.4 ASR-33

A real neutral-current-loop Teletype subjected to continuous spacing can enter the classic **running-open** condition: the selector/mechanism repeatedly operates without producing normal printed characters.

RusTair's current ASR-33 printer presentation is character/mechanics-event based rather than a complete selector-magnet bit-level simulation. Consequently:

- the 88-2SIO electrical BREAK boundary is modeled correctly;
- no NULs are fabricated or printed;
- a full visual/audio simulation of ASR-33 running-open chatter is a peripheral presentation/mechanics follow-up, not an excuse to corrupt the serial byte model.

This limitation must remain explicit until the ASR selector mechanism itself is modeled at signal level.

## 8. Regression evidence

### `src/machine/two_sio.rs`

`break_overrides_wire_without_freezing_tdr_tsr_or_tdre`

- programs 8N1 `/16` BREAK mode;
- writes a TDR byte;
- requires TDRE to return after TDR->TSR while BREAK remains active;
- requires the TSR to finish clocking;
- requires no valid downstream byte to appear.

`break_asserted_mid_frame_irreversibly_corrupts_only_that_frame`

- begins a normal character;
- asserts BREAK part-way through the frame;
- releases BREAK before the nominal frame end;
- requires the damaged byte to remain absent;
- requires the following complete post-BREAK character to be delivered normally.

### `tests/two_sio_break_fidelity.rs`

`both_engines_keep_tdr_tsr_clocking_under_break_without_fabricating_wire_bytes`

- runs through public `BackendHost` on Fast and Cycle;
- verifies BREAK is visible as physical MC6850 line state;
- verifies TDRE recovers while BREAK is active;
- verifies internal TX becomes idle after the frame duration;
- verifies no endpoint byte is produced;
- releases BREAK and verifies the next clean byte appears.

`byte_only_internal_and_tcp_endpoints_do_not_invent_a_break_character`

- guards External TCP, Text Terminal and ASR-33 against introducing BREAK-to-byte policy;
- explicitly rejects common fake-NUL patterns.

Existing `two_sio_external_com_signals.rs` continues to require native host BREAK for External COM.

## 9. How the user can validate it

### 9.1 Electrical BREAK and no fake byte

1. POWER OFF.
2. Configure **MITS 88-2SIO** and connect Port 0 to a byte-visible endpoint such as Text Terminal or External TCP.
3. POWER ON and RESET.
4. Use I/O Inspector/debugger to write the Port 0 control register for the installed address block with 8N1 `/16`, CR6:CR5=`11` (for the default base this is `OUT 10h,75h`).
5. Confirm the serial hardware view reports BREAK active.
6. Write a data byte to Port 0 data (`11h` at the default base).
7. Wait longer than one complete configured frame.
8. TDRE must return to ready, demonstrating that the transmitter did not freeze.
9. The attached byte-only endpoint must **not** receive the written character and must not receive NUL/00h in its place.

### 9.2 External COM

1. Connect External COM to the same 88-2SIO channel and enable a real/virtual host serial port.
2. Enter BREAK with the MC6850 control word.
3. A serial analyzer or the peer end of a virtual COM pair should observe the host serial BREAK condition.
4. Clear BREAK.
5. The host BREAK condition must clear without any synthetic byte being sent by RusTair.

### 9.3 Recovery after BREAK

1. Assert BREAK while a character is already being transmitted.
2. Clear BREAK before that character's nominal frame completes.
3. The interrupted character must not appear at the endpoint.
4. Send a new character after BREAK is clear.
5. That complete new frame must arrive normally.

## 10. Known gaps and non-goals

- RusTair does not yet render the ASR-33's running-open selector chatter caused by a continuous spacing receive line.
- Plain TCP cannot represent an electrical BREAK without defining a separate protocol; RusTair deliberately does not invent one silently.
- This block does not model host-side BREAK **input** into the Altair receiver. The current physical COM work covers MC6850 Tx BREAK output plus CTS/DCD inputs; receive-side host BREAK detection would require a separate explicit host/receiver contract and evidence.
- BREAK does not imply an 8080 interrupt by itself.
- This block does not alter 88-VI interrupt routing.

## 11. Validation history

- External COM native BREAK projection was locally validated previously on 2026-08-31.
- Interrupt routing and physical address/baud straps were subsequently locally validated.
- Internal BREAK frame suppression and public Fast/Cycle regressions were added after those green checkpoints.
- Local focused and full-suite validation is therefore required before this BREAK block, and then the parent 88-2SIO block, can be marked PASS.
- GitHub Actions were not run.

## 12. References

### Motorola MC6850

Motorola Semiconductor Products Inc., *MC6800 Microcomputer System Design Data*, 1976.

https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf

Additional scan of the MC6850/MC68A50/MC68B50 data sheet:

https://www.sprow.co.uk/bbc/hardware/extraserial/6850datasheet.pdf

### MITS 88-2SIO

MITS, *Altair 88-2-SIO Documentation*, reprinted March 1977.

https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf

Relevant sections: control-register bits 5/6, status-register TDRE/CTS descriptions, electronic theory and TTY interconnections.

### Teletype Model 33

Teletype Corporation, Model 33 technical/circuit documentation, including selector-magnet and neutral-signaling behavior.

Document index:

https://www.soemtron.org/teletypemanuals.html

The running-open observation is used only to document the real peripheral consequence of continuous spacing. RusTair does not use it to fabricate byte data.

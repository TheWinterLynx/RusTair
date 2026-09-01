# MITS 88-2SIO / MC6850 BREAK fidelity

Status: **PASS — transmit BREAK electrical override and no-fake-byte behavior locally validated.**

Parent: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Primary references:

- Motorola, *MC6800 Microcomputer System Design Data*, 1976, MC6850 section: `https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf`
- MITS, *Altair 88-2-SIO Documentation*, March 1977: `https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf`

## Scope

MC6850 transmitter control CR6:CR5=`11` forces a continuous spacing/BREAK level on Tx Data. BREAK is electrical line state, not ASCII NUL and not a byte written to TDR.

RusTair separates:

1. internal TDR/TSR progression;
2. the validity of a completed external serial character.

A character whose frame overlaps BREAK cannot be delivered as a valid downstream byte.

## Internal transmitter behavior

TDR/TSR clocking continues while BREAK is active. TDRE therefore remains governed by TDR availability rather than being frozen by the line override.

Each active transmitted frame tracks whether BREAK overlapped it. At nominal frame completion:

- a clean frame enters the completed-wire queue;
- a BREAK-corrupted frame does not.

Releasing BREAK does not repair a frame already corrupted. A later complete post-BREAK frame is delivered normally.

## Endpoint behavior

### External COM

External COM can express real serial BREAK, so RusTair maps the MC6850 state to the host serial driver's native `set_break()` / `clear_break()` controls.

### Text Terminal and External TCP

These are byte-oriented endpoints. They do not receive NUL or any escape byte to represent BREAK.

### ASR-33

Transmit BREAK into a real Teletype can produce mechanical running-open behavior. RusTair does not yet claim a bit-level selector-magnet simulation, but it preserves the correct electrical/byte boundary: no fake character is emitted.

ASR-33 **receive** BREAK in the opposite direction is separately modeled as a held SPACE on the selected UART receive line and is covered by `tests/serial_receive_break_fidelity.rs`.

## Fast / Cycle

The BREAK state and serial transmitter live in shared card hardware below both processor engines. Both engines preserve TDR/TSR progression and suppress BREAK-corrupted wire bytes identically. Cycle does not gain a separate endpoint policy and Fast does not synthesize characters.

## Code map

- `src/mc6850.rs` — CR6:CR5 transmitter-control state.
- `src/machine/two_sio.rs` — timed frame progress and BREAK-corruption tracking.
- `src/backend/mod.rs` — physical modem-line exposure.
- `src/app/serial_hardware.rs` — selected cable boundary.
- `src/app/external_com.rs`, `src/io/com_serial.rs` — native host COM BREAK.

## Regression coverage

- unit tests in `src/machine/two_sio.rs` cover continuous and mid-frame BREAK;
- `tests/two_sio_break_fidelity.rs` covers public Fast/Cycle behavior and prohibits fake BREAK bytes;
- `tests/two_sio_external_com_signals.rs` protects native COM BREAK control;
- `tests/serial_receive_break_fidelity.rs` protects the separate receive-BREAK path.

The focused BREAK regressions and complete normal `cargo test` suite were reported green locally before final closeout on **2026-09-02**.

## Non-claims

This PASS does not claim full ASR-33 running-open mechanical rendering, analog line transition shape, or a custom BREAK framing protocol for raw TCP.

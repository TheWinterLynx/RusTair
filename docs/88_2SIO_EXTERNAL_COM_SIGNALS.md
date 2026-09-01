# 88-2SIO external COM electrical signals: CTS, DCD and BREAK

Status: **PASS — CTS/DCD polarity, stale-line cleanup and native host BREAK projection locally validated.**

Parent: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Primary references:

- MITS, *Altair 88-2-SIO Documentation*, March 1977: `https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf`
- Motorola, *MC6800 Microcomputer System Design Data*, 1976, MC6850 section: `https://www.bitsavers.org/components/motorola/6800/MC6800_Microcomputer_System_Design_Data_1976.pdf`

## Scope

External COM is a physical serial endpoint, not only a byte stream. RusTair therefore keeps MC6850 modem/control signals as electrical state:

- CTS is an MC6850 input;
- DCD is an MC6850 input;
- RTS is an MC6850 output;
- BREAK is a continuous transmit SPACE level, not a byte.

Host-side convenience state never edits RDRF, TDRE or IRQ directly; it only drives/observes physical inputs/outputs and lets the ACIA derive register consequences.

## CTS and DCD

MITS requires unconnected CTS and DCD to be grounded. RusTair's default External COM modem-input mode therefore drives both MC6850 pins LOW.

When host modem pins are selected, the OS serial API reports logical assertion while the MC6850 model consumes literal pin HIGH/LOW. The conversion is explicit and active-LOW:

```text
host CTS asserted -> MC6850 CTS LOW
host CTS absent   -> MC6850 CTS HIGH
host CD asserted  -> MC6850 DCD LOW
host CD absent    -> MC6850 DCD HIGH
```

The MC6850 remains responsible for TDRE inhibition, DCD status, receiver behavior and IRQ sequencing.

## Cable cleanup

Moving or disconnecting External COM cannot leave the old emulated channel with stale CTS/DCD HIGH. The previous channel is restored to the MITS grounded state before ownership changes.

## Transmit BREAK

CR6:CR5=`11` is exposed as MC6850 transmit BREAK. The External COM worker maps it to the host serial API's native `set_break()` / `clear_break()` operation.

No `00h` or other synthetic data byte is written to approximate BREAK.

Internal byte-only endpoints follow the same rule: they receive only complete valid characters and never a fabricated BREAK byte.

## Fast / Cycle

Both engines expose the same backend-neutral modem-line contract:

```rust
serial_modem_lines(...)
serial_set_modem_inputs(...)
```

External modem input changes refresh the shared hardware interrupt projection; modem semantics are not engine-specific.

## Configuration

`ComModemInputMode` distinguishes:

- grounded — MITS no-modem jumpers;
- host pins — follow host CTS/Carrier Detect with active-LOW conversion.

This is separate from the host OS flow-control setting. Enabling host RTS/CTS transport does not silently rewire the historical MC6850 inputs.

## Code map

- `src/mc6850.rs` — CTS/DCD/RTS/BREAK register semantics.
- `src/machine/two_sio.rs` — channel pin exposure.
- `src/backend/mod.rs`, `native.rs`, `cycle_host.rs` — modem-line backend contract.
- `src/app/serial_hardware.rs` — selected-port signal bridge.
- `src/app/external_com.rs` — host assertion to MC6850 pin conversion and cleanup.
- `src/io/com_serial.rs` — real COM pin polling and native BREAK control.
- `src/config/external_com.rs` — modem-input wiring mode.

## Regression coverage

- `tests/two_sio_modem_pins.rs`
- `tests/two_sio_external_com_signals.rs`
- configuration and External COM unit tests.

Coverage protects grounded defaults, active-LOW conversion, DCD/CTS status consequences, native BREAK calls and stale-pin cleanup.

The focused COM signal regressions and complete normal `cargo test` suite were reported green locally before final closeout on **2026-09-02**.

## Non-claims

This PASS does not model exact RS-232 analog voltage magnitude/loading or host adapter electrical imperfections. Receive-side BREAK detection from a host COM driver is not claimed here; the validated ASR-33 receive BREAK path is documented at the parent serial boundary.

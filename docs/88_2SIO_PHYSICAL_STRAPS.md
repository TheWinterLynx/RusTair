# MITS 88-2SIO physical address and baud straps

Status: **PASS — address decode, baud taps, persistence, UI, bootstrap mapping and Fast/Cycle parity locally validated.**

Parent: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Primary source: MITS, *Altair 88-2-SIO Documentation*, March 1977: `https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf`.

## Address selection

A2-A7 select one four-address-aligned block. A0/A1 select the two ACIAs and control/status versus data inside that block.

`TwoSioAddressBlock` therefore accepts aligned bases from `00h` through `F8h`. `FCh-FFh` is deliberately rejected because FFh belongs to the Altair front-panel sense-switch input.

For a base of `44h` the board decodes:

```text
44h Port 0 control/status
45h Port 0 data
46h Port 1 control/status
47h Port 1 data
```

Moving A2-A7 moves all card ownership together: register decode, open bus, PRDY wait, debugger data-port mapping, trace addresses, endpoint labels and authentic bootstrap operands. The old address block is not retained as a compatibility alias.

## Baud taps

Each ACIA independently selects one documented board tap:

```text
110 150 300 1200 1800 2400 4800 9600
```

The physical tap is the input clock source; MC6850 CR1:CR0 still selects /1, /16 or /64. Endpoint speed settings never replace the board clock.

## Current configuration type

`src/config/two_sio.rs` owns the installed hardware:

```rust
pub struct TwoSioStraps {
    pub address: TwoSioAddressBlock,
    pub port0_baud: TwoSioBaudTap,
    pub port1_baud: TwoSioBaudTap,
    pub port0_interface: TwoSioSignalInterface,
    pub port1_interface: TwoSioSignalInterface,
}
```

The signal-interface fields are documented separately in `88_2SIO_SIGNAL_INTERFACES.md`, but they travel with the same installed card state.

Default RusTair installation:

```text
base             10h
Port 0 baud      110
Port 1 baud      9600
Port 0 interface TTY 20 mA
Port 1 interface RS-232
```

## S-100 wait behavior

The selected 88-2SIO input block owns the documented one-Tw PRDY delay. If the card moves, the wait moves with it. Old/unmapped ports return S-100 open bus and do not inherit the 88-2SIO wait.

Fast accounts +1 total T-state. Cycle exposes the real `T1 -> T2 -> Tw -> T3` sequence. Debugger-only `IN` operations are isolated so they cannot leave Fast wait debt for a later guest instruction.

## Authentic bootstrap

The stock bootstrap contains immediate `IN`/`OUT` port operands. Readdressing the card therefore resolves those operands to the selected block instead of creating a hidden alias at `10h/11h`.

For a `44h` installation, only the relevant immediate port bytes change; the loader algorithm remains the same.

## Backend and persistence

Fast and Cycle expose the same `configure_two_sio_straps` / `two_sio_straps` contract. Recreating an engine while POWER is OFF reapplies the stored card configuration.

Canonical persistence keys are:

```text
machine.two_sio_base
machine.two_sio_port0_baud
machine.two_sio_port1_baud
machine.two_sio_port0_interface
machine.two_sio_port1_interface
```

Invalid bases or unknown enum values do not create unsupported hardware states.

## UI

Configuration exposes the A2-A7 block, Port 0 baud, Port 1 baud and both signal interfaces as physical POWER-OFF-only controls. Endpoint labels derive from the current block rather than fixed `10h-13h` text.

## Regression coverage

Key coverage includes:

- configuration unit tests in `src/config/two_sio.rs`;
- `physical_address_strap_moves_decoder_waits_and_open_bus_together`;
- `baud_straps_are_independent_per_acia`;
- backend Fast/Cycle strap parity;
- `tests/two_sio_strap_ui.rs`;
- `tests/two_sio_debugger_wait_isolation.rs`;
- authentic bootstrap readdressing regressions;
- persistence round trips.

These regressions and the complete normal `cargo test` suite were reported green locally before final closeout on **2026-09-02**.

## Non-claims

This PASS does not model oscillator tolerance, individual TTL gate propagation, wire capacitance or analog signal integrity.

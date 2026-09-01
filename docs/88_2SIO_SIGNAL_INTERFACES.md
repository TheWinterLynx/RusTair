# MITS 88-2SIO per-port signal interfaces

Status: **PASS — independent RS-232 / TTL / TTY 20 mA port wiring and endpoint compatibility locally validated.**

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

Parent: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

## Scope

The MITS 88-2SIO allows each MC6850 port to be hardwired independently for one of three external signal families:

- RS-232;
- TTL;
- TTY 20 mA current loop.

RusTair represents this as installed hardware, not as a terminal preference or an implicit level converter.

Primary source: MITS, *88-2SIO Serial I/O Board Documentation*, March 1977: `https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf`.

## Configuration model

`src/config/two_sio.rs` defines:

```rust
pub enum TwoSioSignalInterface {
    Rs232,
    Ttl,
    Tty20mA,
}
```

`TwoSioStraps` contains independent `port0_interface` and `port1_interface` fields alongside the address block and baud taps.

RusTair's explicit default installation is:

- Port 0: 110 baud, TTY 20 mA current loop;
- Port 1: 9600 baud, RS-232.

That is RusTair's default card installation, not a claim that every historical board was wired identically.

## Direct endpoint contract

| Endpoint | Direct requirement |
| --- | --- |
| Built-in ASR-33 | TTY 20 mA current loop |
| External COM | RS-232 |
| Text Terminal | explicit virtual peer, adapts to selected family |
| External TCP | explicit virtual peer, adapts to selected family |

A direct ASR-33 cannot be connected to RS-232 or TTL. External COM cannot be connected directly to TTL or current loop. RusTair rejects those combinations instead of adding a hidden converter.

## Rewiring

Changing either interface is POWER-OFF-only. If a new selection makes an attached physical endpoint incompatible:

1. the old line state is cleaned up;
2. if ASR BREAK is active, its previous UART receive line is returned to MARK;
3. the new card wiring is applied;
4. the incompatible cable is disconnected.

Persisted connections are sanitized by the same matrix when `config.ini` is restored.

## BREAK

The ASR-33 BREAK key is a held physical SPACE condition, never ASCII NUL. It is only meaningful as a direct Model 33 cable when the selected 88-2SIO port is TTY 20 mA current loop. Moving or rewiring the cable releases BREAK from the old channel before ownership changes.

## Fast / Cycle

The interface selection travels inside the same backend-neutral `TwoSioStraps` structure for both engines. Engine replacement while POWER is OFF reapplies the same physical card configuration.

## Persistence and UI

Canonical keys:

```text
machine.two_sio_port0_interface=rs232|ttl|tty20ma
machine.two_sio_port1_interface=rs232|ttl|tty20ma
```

The Configuration menu exposes separate Port 0 and Port 1 interface selectors in the POWER-OFF physical 88-2SIO section. Cable labels include the selected family.

## Code map

- `src/config/two_sio.rs` — physical interface types.
- `src/io/serial_router.rs` — endpoint compatibility matrix.
- `src/app/mod.rs` — POWER-OFF rewiring, disconnect and cable labels.
- `src/app/runtime.rs` — user-facing selectors.
- `src/app/persistence.rs` — persistence and stale-cable sanitization.
- `src/backend/mod.rs`, `native.rs`, `cycle_host.rs` — shared hardware transport.

## Regression coverage

- `tests/two_sio_signal_interfaces.rs`
- `tests/two_sio_strap_ui.rs`
- configuration/router/persistence unit tests.

The focused signal-interface regressions and complete normal `cargo test` suite were reported green locally on **2026-09-02**.

## Non-claims

This PASS does not model exact analog voltage/current magnitude, thresholds, line impedance, cable capacitance, noise, contact bounce or marginal-signal corruption.

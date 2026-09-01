# MITS 88-2SIO per-port signal interfaces

Status: **IMPLEMENTED — final local validation pending.**

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

Related documents:

- `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`
- `docs/88_2SIO_PHYSICAL_STRAPS.md`
- `docs/88_2SIO_EXTERNAL_COM_SIGNALS.md`
- `docs/BASE_HARDWARE_FIDELITY_CLOSEOUT.md`

## 1. Scope

The MITS 88-2SIO does not have one immutable electrical serial connector type. Its two MC6850 channels share the same UART/register semantics, but **each port is independently hardwired for one of three external signal families**:

- RS-232 voltage levels;
- TTL voltage levels;
- TTY 20 mA current loop.

RusTair models that choice as installed board/cable hardware. It is not a terminal preference and it does not insert an invisible level converter when an incompatible physical endpoint is selected.

This document covers the digital configuration/cabling contract. Exact RS-232 voltages, TTL thresholds, current magnitude, transistor slew, line impedance, cable capacitance and noise are outside the current digital claim.

## 2. Primary MITS evidence

Primary source:

**MITS, _88-2SIO Serial I/O Board Documentation_, March 1977**

`https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf`

The assembly/interconnection instructions describe the two ports separately and provide wiring for three signal-interface choices: RS-232, TTL and TTY/current-loop operation. This is board-level interconnect hardware; it does not change the MC6850 register map or word-format control bits.

The same manual separately documents the address-select jumpers, baud-generator taps and DI/EI interrupt wiring. RusTair therefore keeps those concepts distinct even though they form one installed card configuration from the user's point of view.

## 3. RusTair configuration model

`src/config/two_sio.rs` defines the explicit family:

```rust
pub enum TwoSioSignalInterface {
    Rs232,
    Ttl,
    Tty20mA,
}
```

`TwoSioStraps` stores independent Port 0 and Port 1 selections alongside the existing A2-A7 address block and per-port baud taps.

RusTair's default installed 88-2SIO configuration is intentionally explicit:

- Port 0: 110 baud tap, TTY 20 mA current loop — suitable for the built-in ASR-33 direct cable;
- Port 1: 9600 baud tap, RS-232 — suitable for a conventional serial endpoint.

Those defaults describe RusTair's default installation, not a claim that every historical 88-2SIO shipped or was field-wired identically.

## 4. Endpoint compatibility contract

The direct physical endpoints have fixed electrical expectations:

| Endpoint | Direct 88-2SIO requirement |
| --- | --- |
| Built-in ASR-33 | TTY 20 mA current loop |
| External COM | RS-232 |
| Text Terminal | explicit virtual peer; may instantiate selected family |
| External TCP | explicit virtual peer; may instantiate selected family |

A direct ASR-33 is therefore rejected on an RS-232 or TTL port. A direct External COM cable is rejected on TTL or current loop. RusTair does not silently add a converter merely because the byte stream would otherwise be convenient.

Text Terminal and External TCP are not claims about one fixed historical electrical terminal. They are deliberately virtual peers whose connector side follows the selected family. Their virtual nature does not grant them undocumented modem/ready wires or alter the MC6850 itself.

## 5. Rewiring behavior

Changing Port 0 or Port 1 interface represents changing physical board/interconnect wiring and is POWER-OFF-only in the normal UI.

If the new interface makes an already attached direct endpoint impossible:

1. the old physical line is cleaned up first;
2. if the ASR-33 was holding BREAK, its old receive line is returned to MARK before routing changes;
3. the new card wiring is applied;
4. the incompatible cable is disconnected;
5. no replacement adapter is fabricated.

The same compatibility rule applies when the user explicitly chooses a cable destination and when persisted wiring is restored from `config.ini`. A stale configuration file cannot bypass the electrical family selected on the card.

## 6. BREAK interaction

ASR-33 BREAK is a held physical SPACE condition, not ASCII NUL.

That rule remains valid on 88-2SIO only when the Model 33 is attached to a current-loop-configured port. Moving or rewiring the cable releases the old BREAK state before the endpoint loses ownership of that UART port, preventing a detached channel from remaining permanently SPACE.

External COM transmitter BREAK and MC6850 modem/control semantics remain documented separately in `docs/88_2SIO_EXTERNAL_COM_SIGNALS.md`.

## 7. Fast versus Cycle ownership

The interface selection is part of the same `TwoSioStraps` hardware configuration already carried through `MachineBackend` / `BackendHost`.

Consequently:

- Fast and Cycle receive the same per-port signal selections;
- switching engines while POWER is OFF re-applies the selected board wiring;
- no engine owns a private alternative electrical configuration;
- the signal-family choice does not fabricate sub-instruction CPU timing in Fast.

The MC6850 digital core remains common hardware truth; selecting RS-232, TTL or current loop affects the external connector/cable contract, not the UART's status/data register semantics.

## 8. Persistence and UI

Persistent keys:

```text
machine.two_sio_port0_interface=...
machine.two_sio_port1_interface=...
```

Accepted canonical values are `rs232`, `ttl` and `tty20ma`.

Old configuration files that predate these keys migrate to the explicit RusTair default installation. Unknown values do not create a fourth electrical family.

Configuration UI exposes independent Port 0 and Port 1 signal-interface selectors inside the existing POWER-OFF physical 88-2SIO section. Port summaries and endpoint cable labels include the selected family so the user can see the electrical assumption instead of relying on an implicit default.

## 9. Physical-to-code mapping

- `src/config/two_sio.rs` — signal-family enum and per-port installed configuration.
- `src/config/mod.rs` — public configuration export.
- `src/io/serial_router.rs` — direct endpoint compatibility matrix.
- `src/app/mod.rs` — POWER-OFF rewire, automatic incompatible-cable disconnect, cable labels and explicit connection rejection.
- `src/app/runtime.rs` — per-port interface selectors and user-visible physical explanation.
- `src/app/persistence.rs` — persistence, migration and persisted-cable sanitization.
- `src/backend/mod.rs`, `src/backend/native.rs`, `src/backend/cycle_host.rs` — existing backend-neutral `TwoSioStraps` transport shared by both engines.

## 10. Regression coverage

Coverage includes:

- the three documented signal families are distinct typed states;
- default Port 0 current-loop / Port 1 RS-232 installation;
- Fast and Cycle preserve independent Port 0/Port 1 interface selections;
- ASR-33 accepts only TTY 20 mA as a direct 88-2SIO endpoint;
- External COM accepts only RS-232 as a direct 88-2SIO endpoint;
- Text Terminal/TCP remain explicitly virtual selected-family peers;
- UI controls are POWER-OFF-only;
- cable labels expose the selected interface;
- rewiring disconnects incompatible direct endpoints;
- ASR BREAK is released before an incompatible rewire/cable move;
- persistence round-trips both interfaces and rejects unknown families;
- persisted cables pass through the same compatibility matrix.

Important files:

- `tests/two_sio_signal_interfaces.rs`
- `tests/two_sio_strap_ui.rs`
- unit tests in `src/config/two_sio.rs`
- unit tests in `src/app/persistence.rs`
- unit tests in `src/io/serial_router.rs`

## 11. User-observable validation

1. POWER OFF and select MITS 88-2SIO.
2. Confirm Port 0 and Port 1 each expose an independent signal-interface selector.
3. Set Port 0 to TTY 20 mA and connect the ASR-33: the direct cable is allowed.
4. POWER OFF and change that port to RS-232 or TTL: the ASR-33 cable is disconnected rather than converted.
5. Set a port to RS-232 and connect External COM: the direct cable is allowed.
6. POWER OFF and change that port to TTL or TTY 20 mA: External COM is disconnected.
7. Text Terminal or External TCP may remain as explicit virtual peers on any selected family.
8. Restart RusTair and confirm both port interface selections persist.
9. Switch Fast/Cycle while POWER is OFF and confirm the selected interface labels remain unchanged.

## 12. Known limits / non-claims

This digital fidelity item does not claim:

- exact EIA RS-232 positive/negative voltage magnitude or loading;
- exact TTL VIH/VIL thresholds, fan-out or rise/fall time;
- exact current-loop current magnitude or source/sink compliance;
- cable resistance/capacitance, opto/transistor analog behavior, contact bounce or electrical noise;
- automatic corruption caused by an analog marginal signal;
- a specific historical terminal model behind the explicitly virtual Text Terminal/TCP peers.

Those omissions remain explicit rather than being replaced by compatibility hacks.

Final PASS is withheld only until the new signal-interface regressions and the complete local `cargo test` suite are reported green.

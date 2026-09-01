# MITS 88-2SIO interrupt routing

Status: **PASS — DI/EI routing, PINT separation, raw VI boundary, persistence and UI locally validated.**

Parent: `docs/88_2SIO_MC6850_HARDWARE_FIDELITY.md`.

Primary source: MITS, *Altair 88-2-SIO Documentation*, March 1977: `https://www.bitsavers.org/pdf/mits/8800/Altair_88-2-SIO_Documentation_197703.pdf`.

## Scope

The two MC6850 IRQ outputs are independent board signals. MITS names the Port 0 request lead `DI` and the Port 1 request lead `EI`. Each can be:

- left disconnected;
- wired to the single processor interrupt request line `PINT`;
- wired to one of the eight `VI0..VI7` request inputs for a separate MITS 88-Vector Interrupt system.

MC6850 IRQ state and system interrupt routing are therefore distinct. An ACIA may have IRQ/status bit 7 active while processor PINT remains low.

## RusTair topology

`src/config/two_sio.rs` defines:

```rust
pub enum TwoSioInterruptTarget {
    Disconnected,
    Pint,
    Vi0, Vi1, Vi2, Vi3, Vi4, Vi5, Vi6, Vi7,
}

pub struct TwoSioInterruptWiring {
    pub port0: TwoSioInterruptTarget,
    pub port1: TwoSioInterruptTarget,
}
```

Port 0 maps literally to DI and Port 1 to EI.

The migration default is PINT/PINT so old RusTair configuration retains its previous direct-interrupt behavior. This is a compatibility default, not a claim that every historical board was built that way.

## Machine behavior

`src/machine/io_devices.rs` first asks whether each MC6850 is requesting service, then applies the selected DI/EI destination.

- `Disconnected` drives neither PINT nor a VI line.
- `Pint` drives canonical processor PINT.
- `Vi0..Vi7` set only the corresponding raw vector-request bit.

VI requests do not automatically assert PINT and do not create an 8080 RST opcode inside the serial card. A future 88-VI board must consume/arbitrate those raw lines explicitly.

## Fast / Cycle

Both engines consume the same board wiring through the backend-neutral contract:

```rust
configure_two_sio_interrupt_wiring(...)
two_sio_interrupt_wiring()
two_sio_vector_interrupt_requests()
```

Cycle samples the routed canonical PINT line in the exact core. Fast sees the same final PINT routing at its instruction boundary. VI requests remain separate from direct CPU interrupt service in both engines.

Engine replacement while POWER is OFF reapplies the configured DI/EI wiring.

## Persistence and UI

Canonical keys:

```text
machine.two_sio_port0_irq
machine.two_sio_port1_irq
```

Accepted values are `disconnected`, `pint`, and `vi0` through `vi7`.

Configuration exposes separate `DI / Port 0 IRQ` and `EI / Port 1 IRQ` controls. They are POWER-OFF-only and the UI explicitly identifies VI0..VI7 as raw 88-VI request lines rather than CPU vectors.

## Code map

- `src/config/two_sio.rs` — routing types and persistence names.
- `src/machine/io_devices.rs` — ACIA IRQ to PINT/raw-VI projection.
- `src/backend/mod.rs`, `native.rs`, `cycle_host.rs` — shared backend contract.
- `src/app/mod.rs`, `runtime.rs`, `persistence.rs` — configuration, UI and persistence.

## Regression coverage

Coverage includes:

- every legal disconnected/PINT/VI0..VI7 target;
- independent DI/EI wiring;
- live ACIA IRQ remaining active while DI/EI is disconnected;
- VI routing not masquerading as PINT;
- simultaneous raw VI levels;
- Fast/Cycle parity;
- persistence migration and round trip;
- `tests/two_sio_interrupt_ui.rs`.

The focused interrupt-routing regressions and complete normal `cargo test` suite were reported green locally before final closeout on **2026-09-02**.

## Non-claims

This PASS stops at the 88-2SIO's raw VI output boundary. It does not claim a complete installed 88-VI controller, arbitration network or vector-opcode generator.

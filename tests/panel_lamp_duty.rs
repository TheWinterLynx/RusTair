use std::time::Duration;

use rustair::backend::{
    CycleAccurateMachineBackend, FastMachineBackend, MachineBackend,
};
use rustair::machine::PanelLampSnapshot;

fn assert_close(actual: f32, expected: f32, label: &str) {
    let delta = (actual - expected).abs();
    assert!(
        delta <= f32::EPSILON,
        "{label}: expected {expected:.6}, got {actual:.6} (delta {delta:.6})"
    );
}

fn assert_snapshot_equal(actual: PanelLampSnapshot, expected: PanelLampSnapshot, label: &str) {
    for bit in 0..16 {
        assert_close(actual.address[bit], expected.address[bit], &format!("{label} address A{bit}"));
    }
    for bit in 0..8 {
        assert_close(actual.data[bit], expected.data[bit], &format!("{label} data D{bit}"));
    }

    for (name, actual, expected) in [
        ("INTE", actual.inte, expected.inte),
        ("PROT", actual.prot, expected.prot),
        ("MEMR", actual.memr, expected.memr),
        ("INP", actual.inp, expected.inp),
        ("M1", actual.m1, expected.m1),
        ("OUT", actual.out, expected.out),
        ("HLTA", actual.hlta, expected.hlta),
        ("STACK", actual.stack, expected.stack),
        ("W/O", actual.wo, expected.wo),
        ("INT", actual.int_ack, expected.int_ack),
        ("WAIT", actual.wait, expected.wait),
        ("HLDA", actual.hlda, expected.hlda),
    ] {
        assert_close(actual, expected, &format!("{label} {name}"));
    }
}

fn prepare_fast() -> FastMachineBackend {
    let mut backend = FastMachineBackend::default();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x00, 0x00, 0x76]).unwrap(); // NOP / NOP / HLT
    backend
}

fn prepare_cycle() -> CycleAccurateMachineBackend {
    let mut backend = CycleAccurateMachineBackend::default();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x00, 0x00, 0x76]).unwrap(); // NOP / NOP / HLT
    backend
}

#[test]
fn two_nop_fetches_have_identical_raw_address_and_status_duty_fast_vs_cycle() {
    let mut fast = prepare_fast();
    let mut cycle = prepare_cycle();

    fast.run().unwrap();
    cycle.run().unwrap();
    fast.service_execution(8).unwrap();
    cycle.service_execution(8).unwrap();

    assert_eq!(fast.machine().cpu.pc, 0x0002);
    assert_eq!(cycle.cpu().registers().pc, 0x0002);
    assert_eq!(cycle.cpu().total_t_states(), 8);

    fast.commit_panel_activity(Duration::from_millis(16)).unwrap();
    cycle.commit_panel_activity(Duration::from_millis(16)).unwrap();

    let fast_duty = fast.machine().bus.raw_panel_lamp_duty();
    let cycle_duty = cycle.machine().bus.raw_panel_lamp_duty();

    // Both backends represent the same two four-T-state opcode fetches here.
    // Fast is reconstructed, but no unavailable sub-cycle detail is needed for
    // ADDRESS or the latched fetch status lines, so these duties must agree.
    assert_snapshot_equal(fast_duty, cycle_duty, "Fast/Cycle two-NOP duty");

    assert_close(cycle_duty.address[0], 0.5, "A0 duty");
    for bit in 1..16 {
        assert_close(cycle_duty.address[bit], 0.0, &format!("A{bit} duty"));
    }
    assert_close(cycle_duty.memr, 1.0, "MEMR duty");
    assert_close(cycle_duty.m1, 1.0, "M1 duty");
    assert_close(cycle_duty.wo, 1.0, "W/O duty");
    assert_close(cycle_duty.inp, 0.0, "INP duty");
    assert_close(cycle_duty.out, 0.0, "OUT duty");
}

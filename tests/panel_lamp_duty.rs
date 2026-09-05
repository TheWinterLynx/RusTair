use std::time::Duration;

use rustair::backend::{CycleAccurateMachineBackend, MachineBackend};

fn assert_close(actual: f32, expected: f32, label: &str) {
    let delta = (actual - expected).abs();
    assert!(
        delta <= f32::EPSILON,
        "{label}: expected {expected:.6}, got {actual:.6} (delta {delta:.6})"
    );
}

fn prepare_cycle() -> CycleAccurateMachineBackend {
    let mut backend = CycleAccurateMachineBackend::default();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.load_bytes(0, &[0x00, 0x00, 0x76]).unwrap(); // NOP / NOP / HLT
    backend.release_reset().unwrap();
    backend
}

#[test]
fn two_nop_fetches_have_exact_raw_address_and_status_duty() {
    let mut cycle = prepare_cycle();

    cycle.run().unwrap();
    cycle.service_execution(8).unwrap();

    assert_eq!(cycle.cpu().registers().pc, 0x0002);
    assert_eq!(cycle.cpu().total_t_states(), 8);

    cycle.commit_panel_activity(Duration::from_millis(16)).unwrap();
    let duty = cycle.machine().bus.raw_panel_lamp_duty();

    assert_close(duty.address[0], 0.5, "A0 duty");
    for bit in 1..16 {
        assert_close(duty.address[bit], 0.0, &format!("A{bit} duty"));
    }
    assert_close(duty.memr, 1.0, "MEMR duty");
    assert_close(duty.m1, 1.0, "M1 duty");
    assert_close(duty.wo, 1.0, "W/O duty");
    assert_close(duty.inp, 0.0, "INP duty");
    assert_close(duty.out, 0.0, "OUT duty");
}

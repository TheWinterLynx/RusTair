use super::*;
use crate::adaptive_metrics;
use crate::backend::MachineBackend;
use crate::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig, SioHardwareConfig,
    SioInterruptTarget, SioInterruptWiring,
};
use crate::cpu8080_cycle::{MachineCycle, Registers};
use crate::s100_chassis::S100ChassisConfig;
use crate::s100_memory::{S100RamBoardModel, S100RamCardConfig};

fn static_4k_hardware() -> S100HardwareConfig {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::Ram(
                S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0),
            )),
        )
        .unwrap();
    hardware.validate().unwrap();
    hardware
}

fn static_4k_with_pint_sio() -> (S100HardwareConfig, SioHardwareConfig) {
    let mut hardware = static_4k_hardware();
    let mut sio = SioHardwareConfig::default();
    sio.interrupt_wiring = SioInterruptWiring {
        input: SioInterruptTarget::Pint,
        output: SioInterruptTarget::Disconnected,
    };
    hardware
        .set_slot(3, Some(S100InstalledCardConfig::Mits88Sio(sio)))
        .unwrap();
    hardware.validate().unwrap();
    (hardware, sio)
}

fn prepare_backend(hardware: S100HardwareConfig, program: &[u8]) -> CycleAccurateMachineBackend {
    let mut backend = CycleAccurateMachineBackend::default();
    backend
        .machine
        .bus
        .configure_s100_hardware_memory(hardware, RamInit::Zeroed)
        .unwrap();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.load_bytes(0, program).unwrap();
    backend.load_bytes(0x0010, &[0x5a, 0xa5]).unwrap();
    backend.release_reset().unwrap();
    backend.run().unwrap();
    let registers = Registers {
        a: 0x11,
        b: 0x22,
        c: 0x33,
        d: 0x44,
        e: 0x55,
        h: 0x66,
        l: 0x77,
        f: 0xd6,
        sp: 0x0800,
        pc: 0,
    };
    backend.cpu.set_registers(registers);
    backend
}

#[test]
fn compiled_full_ei_lhld_keeps_delay_and_continues_same_window_when_pint_is_low() {
    const BUDGET: u32 = 40;
    // EI (4T), LHLD 0010h (16T) and the required guard NOP (4T) stay in one
    // Full window. The remaining 16T fall below Full's conservative 18T reserve
    // and therefore rejoin exact Partial before the following JMP.
    let program = [0xfb, 0x2a, 0x10, 0x00, 0x00, 0xc3, 0x04, 0x00];
    let hardware = static_4k_hardware();
    let mut compiled = prepare_backend(hardware, &program);
    let mut partial = prepare_backend(hardware, &program);

    adaptive_metrics::begin_measurement();
    compiled.service_execution_compiled(BUDGET).unwrap();
    let stats = adaptive_metrics::end_measurement();

    for _ in 0..BUDGET {
        let ready = partial.machine.bus.cycle_front_panel_ready_input();
        let trace = partial.tick_once(ready);
        assert!(trace.fault.is_none());
    }

    assert_eq!(stats.full_windows, 1, "EI->LHLD with PINT low must not fragment Full");
    assert_eq!(stats.full_t_states, 24);
    assert_eq!(stats.partial_t_states, 16);
    assert_eq!(stats.fallbacks.opcode_barrier, 0);
    assert!(compiled.cpu.interrupts_enabled());
    assert_eq!(compiled.cpu.total_t_states(), partial.cpu.total_t_states());
    assert_eq!(compiled.cpu.registers(), partial.cpu.registers());
    assert_eq!(
        compiled.machine.bus.raw_panel_lamp_duty(),
        partial.machine.bus.raw_panel_lamp_duty(),
        "EI delayed INTE transition must have exact T-state lamp duty",
    );
    assert_eq!(compiled.machine.bus.raw_s100_inte(), partial.machine.bus.raw_s100_inte());
    assert_eq!(compiled.machine.bus.raw_s100_status_word(), partial.machine.bus.raw_s100_status_word());
    assert_eq!(compiled.machine.bus.raw_panel_data(), partial.machine.bus.raw_panel_data());
}

#[test]
fn pending_pint_keeps_ei_lhld_entirely_on_exact_partial() {
    const BUDGET: u32 = 21;
    let program = [0xfb, 0x2a, 0x10, 0x00, 0x00, 0xc3, 0x04, 0x00];
    let (hardware, sio) = static_4k_with_pint_sio();
    let mut compiled = prepare_backend(hardware, &program);
    let mut partial = prepare_backend(hardware, &program);

    // The 88-SIO IRQ pad is only a physical route; software must also enable
    // the board's input interrupt source. Do that through the real control/status
    // register before making RDA active with an injected receive character.
    for backend in [&mut compiled, &mut partial] {
        backend
            .machine
            .bus
            .debugger_output_port(sio.address.status(), 0x01);
        assert!(!backend.machine.bus.cpu_control_lines().interrupt);
        assert!(backend
            .machine
            .bus
            .debugger_inject_serial_rx(sio.address.data(), b'I'));
        assert!(backend.machine.bus.cpu_control_lines().interrupt);
        assert!(!backend.cpu.interrupts_enabled());
    }

    adaptive_metrics::begin_measurement();
    compiled.service_execution_compiled(BUDGET).unwrap();
    let stats = adaptive_metrics::end_measurement();

    for _ in 0..BUDGET {
        let ready = partial.machine.bus.cycle_front_panel_ready_input();
        let trace = partial.tick_once(ready);
        assert!(trace.fault.is_none());
    }

    assert_eq!(stats.full_t_states, 0, "pending PINT must conservatively reject Full EI");
    assert_eq!(stats.partial_t_states, u64::from(BUDGET));
    assert_eq!(stats.fallbacks.opcode_barrier, 1);
    assert_eq!(compiled.cpu.machine_cycle(), MachineCycle::InterruptAck);
    assert_eq!(compiled.cpu.machine_cycle(), partial.cpu.machine_cycle());
    assert_eq!(compiled.cpu.t_state(), partial.cpu.t_state());
    assert_eq!(compiled.cpu.total_t_states(), partial.cpu.total_t_states());
    assert_eq!(compiled.cpu.registers(), partial.cpu.registers());
    assert_eq!(
        compiled.machine.bus.raw_panel_lamp_duty(),
        partial.machine.bus.raw_panel_lamp_duty(),
        "pending-PINT Partial ownership must preserve exact panel duty through INTA T1",
    );
    assert_eq!(compiled.machine.bus.raw_s100_status_word(), partial.machine.bus.raw_s100_status_word());
    assert_eq!(compiled.machine.bus.raw_panel_data(), partial.machine.bus.raw_panel_data());
}

#[test]
fn unpaired_ei_remains_on_partial_and_preserves_ei_then_di_cancellation() {
    const BUDGET: u32 = 8;
    let program = [0xfb, 0xf3, 0x00, 0x00];
    let mut backend = prepare_backend(static_4k_hardware(), &program);

    adaptive_metrics::begin_measurement();
    backend.service_execution_compiled(BUDGET).unwrap();
    let stats = adaptive_metrics::end_measurement();

    assert_eq!(stats.full_t_states, 0);
    assert_eq!(stats.partial_t_states, u64::from(BUDGET));
    assert!(!backend.cpu.interrupts_enabled(), "DI immediately after EI must still win");
    assert_eq!(backend.cpu.registers().pc, 2);
}

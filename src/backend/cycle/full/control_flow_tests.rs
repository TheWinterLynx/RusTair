use super::*;
use crate::adaptive_metrics;
use crate::backend::MachineBackend;
use crate::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
use crate::cpu8080_cycle::Registers;
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
    hardware.validate().unwrap()
}

fn prepare_static_backend(program: &[u8]) -> CycleAccurateMachineBackend {
    let mut backend = CycleAccurateMachineBackend::default();
    backend
        .machine
        .bus
        .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
        .unwrap();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.load_bytes(0, program).unwrap();
    backend.release_reset().unwrap();
    backend.run().unwrap();
    backend
}

#[test]
fn compiled_full_control_flow_t5_families_match_forced_partial_exactly() {
    const BUDGET: u32 = 18;
    const STACK_LO: u16 = 0x07fe;
    const STACK_HI: u16 = 0x07ff;

    // Representative taken/not-taken paths plus all undocumented unconditional
    // CALL aliases. Every admitted family below has an internal M1 T5 that Full
    // must place before the following operand/stack transfer.
    for (opcode, flags, expected_full_t, name) in [
        (0xcd, 0x02, 17u64, "CALL"),
        (0xdd, 0x02, 17, "CALL alias DD"),
        (0xed, 0x02, 17, "CALL alias ED"),
        (0xfd, 0x02, 17, "CALL alias FD"),
        (0xdc, 0x03, 17, "CC taken"),
        (0xdc, 0x02, 11, "CC not taken"),
        (0xc4, 0x02, 17, "CNZ taken"),
        (0xc4, 0x42, 11, "CNZ not taken"),
        (0xc8, 0x42, 11, "RZ taken"),
        (0xc8, 0x02, 5, "RZ not taken"),
        (0xcf, 0x02, 11, "RST 1"),
    ] {
        let mut program = [0x00; 16];
        program[0] = opcode;
        if opcode & 0xc7 == 0xc4 || opcode & 0xcf == 0xcd {
            program[1] = 0x08;
            program[2] = 0x00;
        }

        let mut compiled = prepare_static_backend(&program);
        let mut partial = prepare_static_backend(&program);
        let registers = Registers {
            a: 0x5a,
            b: 0x12,
            c: 0x34,
            d: 0x56,
            e: 0x78,
            h: 0x9a,
            l: 0xbc,
            f: flags,
            sp: 0x0800,
            pc: 0,
        };
        compiled.cpu.set_registers(registers);
        partial.cpu.set_registers(registers);

        // A taken conditional RET consumes this prepared return address.
        for backend in [&mut compiled, &mut partial] {
            backend
                .machine
                .bus
                .debugger_write_memory(0x0800, 0x08, false);
            backend
                .machine
                .bus
                .debugger_write_memory(0x0801, 0x00, false);
        }

        adaptive_metrics::begin_measurement();
        compiled.service_execution_compiled(BUDGET).unwrap();
        let stats = adaptive_metrics::end_measurement();
        assert_eq!(stats.full_t_states, expected_full_t, "{name} Full T-states");
        assert_eq!(
            stats.partial_t_states,
            u64::from(BUDGET) - expected_full_t,
            "{name} budget tail"
        );
        assert_eq!(
            stats.fallbacks.opcode_barrier,
            0,
            "{name} must not be a Full barrier"
        );

        for _ in 0..BUDGET {
            let ready = partial.machine.bus.cycle_front_panel_ready_input();
            let trace = partial.tick_once(ready);
            assert!(trace.fault.is_none(), "{name} Partial oracle faulted");
        }

        assert_eq!(
            compiled.cpu.total_t_states(),
            partial.cpu.total_t_states(),
            "{name} T-states"
        );
        assert_eq!(compiled.cpu.registers(), partial.cpu.registers(), "{name} registers");
        for address in [STACK_LO, STACK_HI, 0x0800, 0x0801] {
            assert_eq!(
                compiled.machine.bus.peek_memory(address),
                partial.machine.bus.peek_memory(address),
                "{name} memory at {address:04x}"
            );
        }
        assert_eq!(
            compiled.machine.bus.raw_panel_lamp_duty(),
            partial.machine.bus.raw_panel_lamp_duty(),
            "{name} Full must preserve exact front-panel duty"
        );
        assert_eq!(
            compiled.machine.bus.raw_s100_status_word(),
            partial.machine.bus.raw_s100_status_word(),
            "{name} final 8212 status"
        );
        assert_eq!(
            compiled.machine.bus.raw_panel_data(),
            partial.machine.bus.raw_panel_data(),
            "{name} final front-panel DATA"
        );
    }
}

#[test]
fn stale_memr_t1_uses_new_address_data_before_next_status_latches() {
    // CALL deliberately gives us M1 -> operand read -> operand read -> stack
    // write transitions. The first three cycles keep stale sMEMR asserted during
    // the next T1, so the DATA lamps must already see the byte at the new address.
    let program = [0xcd, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let mut compiled = prepare_static_backend(&program);
    let mut partial = prepare_static_backend(&program);
    let registers = Registers {
        f: 0x02,
        sp: 0x0800,
        pc: 0,
        ..Registers::default()
    };
    compiled.cpu.set_registers(registers);
    partial.cpu.set_registers(registers);

    compiled.service_execution_compiled(18).unwrap();
    for _ in 0..18 {
        let ready = partial.machine.bus.cycle_front_panel_ready_input();
        assert!(partial.tick_once(ready).fault.is_none());
    }

    assert_eq!(
        compiled.machine.bus.raw_panel_lamp_duty(),
        partial.machine.bus.raw_panel_lamp_duty()
    );
    assert_eq!(compiled.machine.bus.raw_panel_data(), partial.machine.bus.raw_panel_data());
}

#[test]
#[ignore = "diagnostic trace for Full-to-Partial Rcc rejoin"]
fn trace_rz_not_taken_full_to_partial_panel_rejoin() {
    let program = [0xc8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let registers = Registers {
        f: 0x02,
        sp: 0x0800,
        pc: 0,
        ..Registers::default()
    };

    let mut partial = prepare_static_backend(&program);
    partial.cpu.set_registers(registers);
    println!("PARTIAL initial pc={:04x} panel={:02x} status={:02x}", partial.cpu.registers().pc, partial.machine.bus.raw_panel_data(), partial.machine.bus.raw_s100_status_word());
    for n in 1..=6 {
        let ready = partial.machine.bus.cycle_front_panel_ready_input();
        let trace = partial.tick_once(ready);
        println!("PARTIAL t{n} trace={:?}/M{} {:?} pc={:04x} addr={:?} panel={:02x} status={:02x} di={:?}", trace.machine_cycle, trace.machine_cycle_index, trace.t_state, partial.cpu.registers().pc, trace.pins.address, partial.machine.bus.raw_panel_data(), partial.machine.bus.raw_s100_status_word(), partial.machine.bus.raw_s100_data_in());
    }

    let mut compiled = prepare_static_backend(&program);
    compiled.cpu.set_registers(registers);
    let opcode = compiled.compiled_full_opcode(FULL_EXECUTION_MAX_T_STATES, true).unwrap();
    let elapsed = compiled.execute_compiled_full_instruction(opcode).unwrap();
    println!("FULL after instruction elapsed={elapsed} pc={:04x} panel={:02x} status={:02x} pins={:?}", compiled.cpu.registers().pc, compiled.machine.bus.raw_panel_data(), compiled.machine.bus.raw_s100_status_word(), compiled.cpu.pins());
    let ready = compiled.machine.bus.cycle_front_panel_ready_input();
    let trace = compiled.tick_once(ready);
    println!("FULL->PARTIAL next trace={:?}/M{} {:?} pc={:04x} addr={:?} panel={:02x} status={:02x} di={:?}", trace.machine_cycle, trace.machine_cycle_index, trace.t_state, compiled.cpu.registers().pc, trace.pins.address, compiled.machine.bus.raw_panel_data(), compiled.machine.bus.raw_s100_status_word(), compiled.machine.bus.raw_s100_data_in());
}

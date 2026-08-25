use std::time::Duration;

use crate::config::SerialBoard;
use crate::cpu8080::{Bus, Cpu8080};
use crate::cpu8080_cycle::{
    Cpu8080Cycle, Cpu8080Inputs, MachineCycle, Registers, TState, TickTrace,
};
use crate::machine::AltairMachine;

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, CpuState,
    EmulationEngine, FrontPanelState, Intel8080State, MachineBackend,
};

/// Host-driven machine backend around the validated T-state Intel 8080 core.
///
/// The CPU timing is exact at T-state granularity. This first integration stage
/// deliberately reuses `AltairBus`'s existing instruction/machine-cycle bridge
/// for RAM, I/O and front-panel side effects; exact per-T-state S-100 lamp
/// sampling is added separately rather than overstating `exact_bus_activity`.
pub struct CycleAccurateMachineBackend {
    machine: AltairMachine,
    cpu: Cpu8080Cycle,
    hold_requested: bool,
    instruction_address: u16,
}

impl Default for CycleAccurateMachineBackend {
    fn default() -> Self {
        let machine = AltairMachine::default();
        let mut backend = Self {
            machine,
            cpu: Cpu8080Cycle::new(),
            hold_requested: false,
            instruction_address: 0,
        };
        backend.sync_machine_cpu();
        backend
    }
}

impl CycleAccurateMachineBackend {
    pub fn machine(&self) -> &AltairMachine { &self.machine }
    pub fn machine_mut(&mut self) -> &mut AltairMachine { &mut self.machine }
    pub fn cpu(&self) -> &Cpu8080Cycle { &self.cpu }

    fn snapshot_cpu(&self) -> CpuState {
        let r = self.cpu.registers();
        CpuState::Intel8080(Intel8080State {
            a: r.a,
            b: r.b,
            c: r.c,
            d: r.d,
            e: r.e,
            h: r.h,
            l: r.l,
            flags: r.f,
            pc: r.pc,
            sp: r.sp,
            inte: self.cpu.interrupts_enabled(),
            halted: Some(self.cpu.is_halted()),
            total_t_states: Some(self.cpu.total_t_states()),
        })
    }

    fn snapshot_panel(&self) -> FrontPanelState {
        FrontPanelState {
            powered: self.machine.powered,
            running: self.machine.running,
            switches: self.machine.panel_switches(),
            address: self.machine.address_leds(),
            data: self.machine.data_leds(),
            lamps: self.machine.panel_lamps(),
            current_board_protected: self.machine.current_board_protected(),
            ext_clear_asserted: self.machine.ext_clear_asserted(),
        }
    }

    fn cycle_registers_from_fast(cpu: &Cpu8080) -> Registers {
        Registers {
            a: cpu.a,
            b: cpu.b,
            c: cpu.c,
            d: cpu.d,
            e: cpu.e,
            h: cpu.h,
            l: cpu.l,
            f: cpu.f,
            sp: cpu.sp,
            pc: cpu.pc,
        }
    }

    /// `AltairMachine` still owns the common chassis/front-panel helpers. Keep
    /// its legacy CPU field as a passive mirror so those helpers see the real
    /// cycle-core PC/HALT state without ever executing the fast CPU.
    fn sync_machine_cpu(&mut self) {
        let r = self.cpu.registers();
        let cpu = &mut self.machine.cpu;
        cpu.a = r.a;
        cpu.b = r.b;
        cpu.c = r.c;
        cpu.d = r.d;
        cpu.e = r.e;
        cpu.h = r.h;
        cpu.l = r.l;
        cpu.f = r.f;
        cpu.pc = r.pc;
        cpu.sp = r.sp;
        cpu.inte = self.cpu.interrupts_enabled();
        cpu.halted = self.cpu.is_halted();
        cpu.cycles = self.cpu.total_t_states();
    }

    fn sync_cycle_pc_from_machine(&mut self) {
        let pc = self.machine.cpu.pc;
        let mut r = self.cpu.registers();
        r.pc = pc;
        self.cpu.set_registers(r);
        self.sync_machine_cpu();
    }

    fn data_in_for_current_t_state(&mut self) -> u8 {
        if self.cpu.t_state() != TState::T3 {
            return 0;
        }
        let address = self.cpu.pins().address.unwrap_or(0);
        match self.cpu.machine_cycle() {
            MachineCycle::InstructionFetch => self.machine.bus.opcode_fetch(address),
            MachineCycle::MemoryRead => self.machine.bus.read(address),
            MachineCycle::StackRead => self.machine.bus.stack_read(address),
            MachineCycle::InputRead => self.machine.bus.input(address as u8),
            MachineCycle::InterruptAck | MachineCycle::InterruptAckWhileHalt => 0xff,
            _ => 0,
        }
    }

    fn apply_trace_side_effects(&mut self, trace: &TickTrace) {
        if trace.t_state == TState::T1 {
            match trace.machine_cycle {
                MachineCycle::HaltAck => {
                    self.machine.bus.halt_ack(
                        trace.pins.address.unwrap_or(self.cpu.registers().pc),
                        trace.opcode.unwrap_or(0x76),
                    );
                }
                MachineCycle::InterruptAck | MachineCycle::InterruptAckWhileHalt => {
                    self.machine.bus.interrupt_ack(
                        trace.pins.address.unwrap_or(self.cpu.registers().pc),
                        trace.opcode.unwrap_or(0xff),
                        trace.machine_cycle == MachineCycle::InterruptAckWhileHalt,
                    );
                }
                _ => {}
            }
        }

        if trace.t_state == TState::T3 {
            let address = trace.pins.address.unwrap_or(0);
            match trace.machine_cycle {
                MachineCycle::MemoryWrite => {
                    if let Some(value) = trace.pins.data_out {
                        self.machine.bus.write(address, value);
                    }
                }
                MachineCycle::StackWrite => {
                    if let Some(value) = trace.pins.data_out {
                        self.machine.bus.stack_write(address, value);
                    }
                }
                MachineCycle::OutputWrite => {
                    if let Some(value) = trace.pins.data_out {
                        self.machine.bus.output(address as u8, value);
                    }
                }
                _ => {}
            }
        }

        if trace.instruction_complete {
            self.machine.bus.instruction_complete(
                self.instruction_address,
                trace.opcode.unwrap_or(0),
                trace.instruction_t_states,
            );
        }
    }

    fn tick_once(&mut self, ready: bool) -> TickTrace {
        if self.cpu.machine_cycle() == MachineCycle::InstructionFetch
            && self.cpu.t_state() == TState::T1
        {
            self.instruction_address = self.cpu.registers().pc;
        }

        let data_in = self.data_in_for_current_t_state();
        let trace = self.cpu.tick(Cpu8080Inputs {
            data_in,
            ready,
            interrupt: false,
            hold: self.hold_requested,
            reset: false,
        });
        self.apply_trace_side_effects(&trace);
        self.sync_machine_cpu();
        trace
    }

    fn run_one_instruction(&mut self) {
        for _ in 0..128 {
            let trace = self.tick_once(true);
            if trace.instruction_complete || trace.fault.is_some() || self.cpu.is_halted() {
                break;
            }
        }
    }

    fn reset_cycle_core(&mut self) {
        let _ = self.cpu.tick(Cpu8080Inputs {
            reset: true,
            ..Cpu8080Inputs::default()
        });
        self.sync_machine_cpu();
    }
}

impl MachineBackend for CycleAccurateMachineBackend {
    fn engine(&self) -> EmulationEngine { EmulationEngine::RustCycleAccurate8080 }
    fn name(&self) -> &'static str { "RusTair cycle-accurate 8080" }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            front_panel: true,
            exact_bus_activity: false,
            exact_t_state_timing: true,
            memory_protection: true,
            hold_hlda: false,
            direct_memory_access: true,
            serial_routing: true,
            disk_mount: false,
        }
    }

    fn execution_model(&self) -> BackendExecutionModel { BackendExecutionModel::HostDriven }
    fn cpu_state(&mut self) -> BackendResult<CpuState> { Ok(self.snapshot_cpu()) }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { Ok(self.snapshot_panel()) }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        self.power_with_historical_run_latch(on, false)
    }

    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> {
        self.machine.power_with_historical_run_latch(on, historical);
        self.hold_requested = false;
        if on {
            self.cpu = Cpu8080Cycle::new();
            self.cpu.set_registers(Self::cycle_registers_from_fast(&self.machine.cpu));
        } else {
            self.cpu = Cpu8080Cycle::new();
        }
        self.sync_machine_cpu();
        Ok(())
    }

    fn run(&mut self) -> BackendResult<()> { self.machine.set_running(true); Ok(()) }
    fn halt(&mut self) -> BackendResult<()> { self.machine.set_running(false); Ok(()) }

    fn step(&mut self) -> BackendResult<()> {
        if self.machine.powered && !self.machine.running {
            self.run_one_instruction();
        }
        Ok(())
    }

    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        if self.machine.powered && self.machine.running {
            for _ in 0..t_state_budget {
                let trace = self.tick_once(true);
                if trace.fault.is_some() {
                    break;
                }
            }
        }
        Ok(())
    }

    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {
        self.sync_machine_cpu();
        self.machine.commit_panel_activity(dt);
        Ok(())
    }

    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> {
        self.sync_machine_cpu();
        self.machine.assert_run_stop(run);
        Ok(())
    }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> {
        self.machine.release_run_stop(run);
        Ok(())
    }

    fn assert_reset(&mut self) -> BackendResult<()> {
        self.machine.assert_front_panel_reset();
        self.reset_cycle_core();
        Ok(())
    }
    fn release_reset(&mut self) -> BackendResult<()> {
        self.machine.release_front_panel_reset();
        self.sync_machine_cpu();
        Ok(())
    }
    fn assert_clear(&mut self) -> BackendResult<()> { self.machine.assert_front_panel_clear(); Ok(()) }
    fn release_clear(&mut self) -> BackendResult<()> { self.machine.release_front_panel_clear(); Ok(()) }

    fn request_hold(&mut self, hold: bool) -> BackendResult<()> {
        self.hold_requested = hold;
        self.machine.request_hold(hold);
        Ok(())
    }

    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.sync_machine_cpu();
        self.machine.examine(next);
        self.sync_cycle_pc_from_machine();
        Ok(())
    }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        self.sync_machine_cpu();
        self.machine.deposit(next);
        self.sync_cycle_pc_from_machine();
        Ok(())
    }
    fn protect_current_board(&mut self, protected: bool) -> BackendResult<()> {
        self.machine.protect_current_board(protected);
        Ok(())
    }
    fn switch_register(&mut self) -> BackendResult<u16> { Ok(self.machine.panel_switches()) }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        let changed = self.machine.panel_switches() ^ value;
        for bit in 0..16 {
            if changed & (1u16 << bit) != 0 {
                self.machine.toggle_sense_switch(bit);
            }
        }
        Ok(())
    }

    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        self.machine.configure_serial_board(board);
        Ok(())
    }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> { Ok(self.machine.serial_board()) }
    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_receive(byte),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_receive(byte),
        }
        Ok(())
    }
    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_rx_empty(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_rx_empty(),
        })
    }
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_rx_len(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_rx_len(),
        })
    }
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.tx_busy(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_busy(),
        })
    }
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_tx_front(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_front(),
        })
    }
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_tx_complete(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_complete(),
        })
    }
    fn clear_serial(&mut self) -> BackendResult<()> { self.machine.bus.clear_serial(); Ok(()) }

    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        Ok(self.machine.bus.peek_memory(address))
    }
    fn write_memory(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> BackendResult<bool> {
        Ok(self.machine.bus.debugger_write_memory(address, value, respect_protection))
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.machine.bus.load(address, bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_backend_executes_real_t_states_without_fast_cpu_execution() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00]).unwrap();
        backend.step().unwrap();

        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else {
            panic!("expected Intel 8080 state")
        };
        assert_eq!(state.pc, 1);
        assert_eq!(state.total_t_states, Some(4));
        assert_eq!(backend.machine().cpu.pc, 1, "legacy field must only mirror the cycle core");
    }

    #[test]
    fn cycle_backend_advertises_timing_without_overclaiming_panel_bus_accuracy() {
        let backend = CycleAccurateMachineBackend::default();
        let capabilities = backend.capabilities();
        assert!(capabilities.exact_t_state_timing);
        assert!(!capabilities.exact_bus_activity);
        assert!(!capabilities.hold_hlda);
    }

    #[test]
    fn cycle_backend_memory_and_io_path_runs_through_altair_bus() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        // MVI A,5Ah ; STA 2000h
        backend.load_bytes(0, &[0x3e, 0x5a, 0x32, 0x00, 0x20]).unwrap();
        backend.step().unwrap();
        backend.step().unwrap();
        assert_eq!(backend.peek_memory(0x2000).unwrap(), Some(0x5a));
        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(state.pc, 5);
        assert_eq!(state.total_t_states, Some(20));
    }
}

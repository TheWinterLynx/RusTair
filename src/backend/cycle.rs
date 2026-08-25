use std::time::Duration;

use crate::config::SerialBoard;
use crate::cpu8080::{Bus, Cpu8080};
use crate::cpu8080_cycle::{
    Cpu8080Cycle, Cpu8080Inputs, MachineCycle, Registers, TState, TickTrace,
};
use crate::machine::{AltairMachine, Cycle8080S100Adapter};

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, CpuState,
    EmulationEngine, FrontPanelState, Intel8080State, MachineBackend,
};

/// Host-driven machine backend around the validated T-state Intel 8080 core.
///
/// RAM/I/O side effects are performed through a raw machine path that does not
/// synthesize legacy aggregate bus cycles. Every `Cpu8080Cycle::tick()` is then
/// converted by the cycle CPU-board adapter into the same S-100 sample contract
/// consumed by the fast CPU-board adapter and the front-panel bus model.
pub struct CycleAccurateMachineBackend {
    machine: AltairMachine,
    cpu: Cpu8080Cycle,
    instruction_address: u16,
}

impl Default for CycleAccurateMachineBackend {
    fn default() -> Self {
        let machine = AltairMachine::default();
        let mut backend = Self {
            machine,
            cpu: Cpu8080Cycle::new(),
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

    /// Supply guest-visible input only at the T3 sampling point. This is the
    /// destructive/functional read path: serial RX is consumed exactly once and
    /// BASIC's transient memory guard is touched only when the CPU really reads.
    fn data_in_for_current_t_state(&mut self) -> u8 {
        if self.cpu.t_state() != TState::T3 {
            return 0;
        }
        let address = self.cpu.pins().address.unwrap_or(0);
        match self.cpu.machine_cycle() {
            MachineCycle::InstructionFetch | MachineCycle::MemoryRead | MachineCycle::StackRead => {
                self.machine.bus.cycle_read_memory(address)
            }
            MachineCycle::InputRead => self.machine.bus.cycle_input_port(address as u8),
            // No interrupt controller is wired into the machine abstraction yet.
            // 0xFF preserves the existing benign external-opcode default.
            MachineCycle::InterruptAck | MachineCycle::InterruptAckWhileHalt => 0xff,
            _ => 0,
        }
    }

    fn apply_trace_side_effects(&mut self, trace: &TickTrace) {
        if trace.t_state == TState::T3 {
            let address = trace.pins.address.unwrap_or(0);
            match trace.machine_cycle {
                MachineCycle::MemoryWrite | MachineCycle::StackWrite => {
                    if let Some(value) = trace.pins.data_out {
                        self.machine.bus.cycle_write_memory(address, value);
                    }
                }
                MachineCycle::OutputWrite => {
                    if let Some(value) = trace.pins.data_out {
                        self.machine.bus.cycle_output_port(address as u8, value);
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

    /// Data electrically visible on the S-100 data bus for this whole T-state.
    /// CPU-driven status/write data comes directly from the pins. During reads,
    /// T2/TW use a non-destructive preview and T3 uses the exact value consumed
    /// by the core, so displaying the panel can never dequeue serial input.
    fn visible_bus_data(&self, trace: &TickTrace, sampled_data_in: u8) -> Option<u8> {
        if let Some(data) = trace.pins.data_out {
            return Some(data);
        }

        if !matches!(trace.t_state, TState::T2 | TState::Tw | TState::T3) {
            return None;
        }

        let address = trace.pins.address?;
        match trace.machine_cycle {
            MachineCycle::InstructionFetch | MachineCycle::MemoryRead | MachineCycle::StackRead => {
                if trace.t_state == TState::T3 {
                    Some(sampled_data_in)
                } else {
                    Some(self.machine.bus.cycle_peek_memory(address))
                }
            }
            MachineCycle::InputRead => {
                if trace.t_state == TState::T3 {
                    Some(sampled_data_in)
                } else {
                    Some(self.machine.bus.peek_io_port(address as u8))
                }
            }
            MachineCycle::InterruptAck | MachineCycle::InterruptAckWhileHalt => Some(0xff),
            _ => None,
        }
    }

    fn drive_s100_t_state(&mut self, trace: &TickTrace, sampled_data_in: u8, ready: bool) {
        let visible_data = self.visible_bus_data(trace, sampled_data_in);
        let sample = Cycle8080S100Adapter::sample(trace, visible_data, ready);
        self.machine.bus.drive_cpu_board_sample(sample);
    }

    fn tick_once(&mut self, ready: bool) -> TickTrace {
        if self.cpu.machine_cycle() == MachineCycle::InstructionFetch
            && self.cpu.t_state() == TState::T1
        {
            self.instruction_address = self.cpu.registers().pc;
        }

        let data_in = self.data_in_for_current_t_state();
        let lines = self.machine.bus.cpu_control_lines();
        let trace = self.cpu.tick(Cpu8080Inputs {
            data_in,
            // SINGLE STEP may momentarily override the stopped READY line for
            // exactly one machine cycle. HOLD and RESET always arrive through
            // the shared S-100 control-line contract.
            ready,
            interrupt: false,
            hold: lines.hold,
            reset: lines.reset,
        });
        self.apply_trace_side_effects(&trace);
        self.drive_s100_t_state(&trace, data_in, ready);
        self.sync_machine_cpu();
        trace
    }

    fn machine_cycle_finished_since(
        &self,
        start_cycle: MachineCycle,
        start_index: u8,
        trace: &TickTrace,
    ) -> bool {
        self.cpu.machine_cycle() != start_cycle
            || self.cpu.machine_cycle_index() != start_index
            || (self.cpu.t_state() == TState::T1 && trace.t_state != TState::T1)
            || self.cpu.is_halted()
            || self.cpu.is_holding()
    }

    /// The original 8800 SINGLE STEP circuit releases READY for one machine
    /// cycle and removes it again at the following PSYNC. Unlike the fast core,
    /// this backend can reproduce that boundary directly.
    fn run_one_machine_cycle(&mut self) {
        let start_cycle = self.cpu.machine_cycle();
        let start_index = self.cpu.machine_cycle_index();
        for _ in 0..32 {
            let trace = self.tick_once(true);
            if trace.fault.is_some()
                || self.machine_cycle_finished_since(start_cycle, start_index, &trace)
            {
                break;
            }
        }
    }

    /// STOP is latched by the display/control hardware at PSYNC. If the switch
    /// is actuated in the middle of a machine cycle, let that cycle reach the
    /// next externally visible SYNC before dropping the RUN latch/READY line.
    /// A processor already dwelling in HLT produces no useful new PSYNC, so the
    /// historical STOP+RESET recovery behavior remains intact.
    fn advance_to_stop_sync(&mut self) {
        if self.cpu.is_halted() {
            return;
        }
        for _ in 0..64 {
            let ready = self.machine.bus.cpu_control_lines().ready;
            let trace = self.tick_once(ready);
            if trace.fault.is_some() || self.cpu.is_halted() || self.cpu.is_holding() {
                break;
            }
            if trace.t_state == TState::T1 && trace.pins.sync {
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
            exact_bus_activity: true,
            exact_t_state_timing: true,
            memory_protection: true,
            hold_hlda: true,
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
            self.run_one_machine_cycle();
            // SINGLE STEP releases READY only for the selected machine cycle.
            // Reassert STOP afterwards without replacing the last real bus
            // address/data/status sample produced by that cycle.
            self.machine.set_running(false);
        }
        Ok(())
    }

    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        if self.machine.powered && self.machine.running {
            for _ in 0..t_state_budget {
                let ready = self.machine.bus.cpu_control_lines().ready;
                let trace = self.tick_once(ready);
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
        if !run && self.machine.powered && self.machine.running && !self.cpu.is_halted() {
            self.advance_to_stop_sync();
        }
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
        assert!(backend.machine().wait_led(), "STEP must return to the stopped WAIT state");
    }

    #[test]
    fn cycle_backend_single_step_advances_one_machine_cycle_not_whole_instruction() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x5a]).unwrap(); // MVI A,5Ah
        let CpuState::Intel8080(before) = backend.cpu_state().unwrap() else { unreachable!() };

        backend.step().unwrap();
        let CpuState::Intel8080(after_fetch) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(after_fetch.pc, 1);
        assert_eq!(after_fetch.a, before.a, "MVI operand cycle must not have executed yet");
        assert_eq!(after_fetch.total_t_states, Some(4));
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::MemoryRead);
        assert_eq!(backend.cpu().machine_cycle_index(), 2);

        backend.step().unwrap();
        let CpuState::Intel8080(after_operand) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(after_operand.pc, 2);
        assert_eq!(after_operand.a, 0x5a);
        assert_eq!(after_operand.total_t_states, Some(7));
        assert!(backend.machine().wait_led());
    }

    #[test]
    fn cycle_backend_advertises_exact_t_state_and_bus_capabilities() {
        let backend = CycleAccurateMachineBackend::default();
        let capabilities = backend.capabilities();
        assert!(capabilities.exact_t_state_timing);
        assert!(capabilities.exact_bus_activity);
        assert!(capabilities.hold_hlda);
    }

    #[test]
    fn cycle_backend_memory_and_io_path_runs_through_altair_bus() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        // MVI A,5Ah is two machine cycles; STA 1F00h is four. SINGLE STEP on
        // the original 8800 advances one machine cycle at a time.
        backend.load_bytes(0, &[0x3e, 0x5a, 0x32, 0x00, 0x1f]).unwrap();
        for _ in 0..6 {
            backend.step().unwrap();
        }
        assert_eq!(backend.peek_memory(0x1f00).unwrap(), Some(0x5a));
        assert_eq!(backend.peek_memory(0x2000).unwrap(), None);
        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(state.pc, 5);
        assert_eq!(state.total_t_states, Some(20));

        backend.commit_panel_activity(Duration::from_millis(16)).unwrap();
        let panel = backend.front_panel_state().unwrap();
        assert_eq!(panel.address, 0x1f00);
        assert_eq!(panel.data, 0x5a);
        assert_eq!(panel.lamps.memr, 0.0);
        assert_eq!(panel.lamps.wo, 0.0);
        assert_eq!(panel.lamps.wait, 1.0);
    }

    #[test]
    fn physical_stop_waits_for_next_psync_boundary() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x5a, 0x00]).unwrap(); // MVI A,5Ah; NOP
        let CpuState::Intel8080(before) = backend.cpu_state().unwrap() else { unreachable!() };
        backend.run().unwrap();

        // Stop is requested after fetch T2. The current M1 is allowed to finish,
        // then the M2 PSYNC/status T1 is exposed before READY is withdrawn.
        backend.service_execution(2).unwrap();
        assert_eq!(backend.cpu().t_state(), TState::T3);
        backend.assert_run_stop(false).unwrap();

        assert!(!backend.machine().running);
        assert!(backend.machine().wait_led());
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::MemoryRead);
        assert_eq!(backend.cpu().machine_cycle_index(), 2);
        assert_eq!(backend.cpu().t_state(), TState::T2);
        let CpuState::Intel8080(stopped) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(stopped.pc, 1);
        assert_eq!(stopped.a, before.a, "STOP at PSYNC must not execute the MVI operand read");
        assert_eq!(stopped.total_t_states, Some(5));

        backend.release_run_stop(false).unwrap();
        backend.assert_run_stop(true).unwrap();
        backend.release_run_stop(true).unwrap();
        backend.service_execution(2).unwrap();
        let CpuState::Intel8080(resumed) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(resumed.pc, 2);
        assert_eq!(resumed.a, 0x5a);
        assert_eq!(resumed.total_t_states, Some(7));
    }

    #[test]
    fn cycle_backend_hold_reaches_real_hlda_and_relinquishes_cpu() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00, 0x00]).unwrap();
        backend.run().unwrap();
        backend.request_hold(true).unwrap();
        assert!(backend.machine().bus.cpu_control_lines().hold);

        // T1, T2(sample HOLD), T3, T4(boundary), THOLD.
        backend.service_execution(5).unwrap();
        assert!(backend.cpu().is_holding());
        backend.commit_panel_activity(Duration::from_millis(16)).unwrap();
        let held = backend.front_panel_state().unwrap();
        assert_eq!(held.lamps.hlda, 1.0);

        backend.request_hold(false).unwrap();
        backend.service_execution(1).unwrap();
        assert!(!backend.cpu().is_holding());
    }

    #[test]
    fn cycle_backend_io_preview_does_not_consume_serial_before_t3() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        // IN 01h on the default 88-SIO data port.
        backend.load_bytes(0, &[0xdb, 0x01]).unwrap();
        backend.serial_receive(BackendSerialPort::Port0, b'R').unwrap();
        backend.run().unwrap();

        // 4T fetch + 3T immediate read + M3 T1/T2 = 9T. The T2 panel sample
        // peeks at the UART but must leave the byte queued for the actual T3.
        backend.service_execution(9).unwrap();
        assert_eq!(backend.serial_rx_len(BackendSerialPort::Port0).unwrap(), 1);

        backend.service_execution(1).unwrap();
        assert_eq!(backend.serial_rx_len(BackendSerialPort::Port0).unwrap(), 0);
        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(state.a, b'R');
        assert_eq!(state.pc, 2);
        assert_eq!(state.total_t_states, Some(10));
    }
}

use std::time::Duration;

use crate::config::SerialBoard;
use crate::cpu8080::{Bus, Cpu8080};
use crate::cpu8080_cycle::{
    Cpu8080Cycle, Cpu8080Inputs, MachineCycle, Registers, TState, TickTrace,
};
use crate::machine::{AltairMachine, Cycle8080S100Adapter};

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, BusCpuPins,
    BusStatusLines, BusTeachingAccuracy, BusTeachingSnapshot, CpuState, EmulationEngine,
    FrontPanelState, Intel8080State, MachineBackend,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CycleExecutionEvent {
    BeforeInstruction,
    InstructionComplete,
}

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
    /// Historical teaching sample only. It is a derived observation of the
    /// canonical CPU/S-100 state, never an authority used to drive the machine.
    last_teaching_snapshot: Option<BusTeachingSnapshot>,
    /// STOP may be asserted while HLDA suppresses PSYNC. Once the first resumed
    /// T1 captures the physical STOP latch, finish the real T2 -> TW handshake
    /// before host execution freezes.
    stop_wait_park_pending: bool,
}

impl Default for CycleAccurateMachineBackend {
    fn default() -> Self {
        let machine = AltairMachine::default();
        let mut backend = Self {
            machine,
            cpu: Cpu8080Cycle::new(),
            instruction_address: 0,
            last_teaching_snapshot: None,
            stop_wait_park_pending: false,
        };
        backend.sync_machine_cpu();
        backend
    }
}

impl CycleAccurateMachineBackend {
    pub fn machine(&self) -> &AltairMachine { &self.machine }
    pub fn machine_mut(&mut self) -> &mut AltairMachine { &mut self.machine }
    pub fn cpu(&self) -> &Cpu8080Cycle { &self.cpu }
    pub(super) fn teaching_snapshot(&self) -> Option<BusTeachingSnapshot> { self.last_teaching_snapshot }

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

    fn cycle_accepts_front_panel_data(&self) -> bool {
        matches!(
            self.cpu.machine_cycle(),
            MachineCycle::InstructionFetch | MachineCycle::MemoryRead | MachineCycle::StackRead
        )
    }

    /// Supply guest-visible input only at the T3 sampling point. During an
    /// EXAMINE/EXAMINE NEXT sequence the Display/Control board disables the RAM
    /// source and drives its jam byte instead. During INTA the selected S-100
    /// interrupt source drives the direct vector opcode onto the data bus.
    fn data_in_for_current_t_state(&mut self, front_panel_data: Option<u8>) -> u8 {
        if self.cpu.t_state() != TState::T3 {
            return 0;
        }
        if self.cycle_accepts_front_panel_data() {
            if let Some(value) = front_panel_data {
                return value;
            }
        }

        let address = self.cpu.pins().address.unwrap_or(0);
        match self.cpu.machine_cycle() {
            MachineCycle::InstructionFetch | MachineCycle::MemoryRead | MachineCycle::StackRead => {
                self.machine.bus.cycle_read_memory(address)
            }
            MachineCycle::InputRead => self.machine.bus.cycle_input_port(address as u8),
            MachineCycle::InterruptAck | MachineCycle::InterruptAckWhileHalt => {
                self.machine.bus.direct_interrupt_opcode()
            }
            _ => 0,
        }
    }

    fn apply_trace_side_effects(&mut self, trace: &TickTrace, record_instruction: bool) {
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

        if record_instruction && trace.instruction_complete {
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
    /// by the core. A front-panel jam byte overrides RAM throughout the same
    /// read window, matching the open-collector injection hardware.
    fn visible_bus_data(
        &self,
        trace: &TickTrace,
        sampled_data_in: u8,
        front_panel_data: Option<u8>,
    ) -> Option<u8> {
        if let Some(data) = trace.pins.data_out {
            return Some(data);
        }

        if !matches!(trace.t_state, TState::T2 | TState::Tw | TState::T3) {
            return None;
        }

        if matches!(
            trace.machine_cycle,
            MachineCycle::InstructionFetch | MachineCycle::MemoryRead | MachineCycle::StackRead
        ) {
            if let Some(value) = front_panel_data {
                return Some(value);
            }
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
            MachineCycle::InterruptAck | MachineCycle::InterruptAckWhileHalt => {
                Some(self.machine.bus.direct_interrupt_opcode())
            }
            _ => None,
        }
    }

    fn drive_s100_t_state(
        &mut self,
        trace: &TickTrace,
        sampled_data_in: u8,
        front_panel_data: Option<u8>,
        ready: bool,
    ) -> Option<u8> {
        let visible_data = self.visible_bus_data(trace, sampled_data_in, front_panel_data);
        let sample = Cycle8080S100Adapter::sample(trace, visible_data, ready);

        self.machine.bus.drive_cpu_board_sample(sample);
        visible_data
    }

    fn capture_teaching_snapshot(&mut self, trace: &TickTrace, visible_data: Option<u8>, ready: bool) {
        let status_word = Some(self.machine.bus.raw_s100_status_word());
        let mut status = BusStatusLines::from_status_word(status_word);
        status.inte = Some(self.machine.bus.raw_s100_inte());
        status.prot = Some(self.machine.bus.raw_s100_prot());
        status.wait = Some(self.machine.bus.raw_s100_wait());
        status.hlda = Some(self.machine.bus.raw_s100_hlda());
        let lines = self.machine.bus.cpu_control_lines();
        self.last_teaching_snapshot = Some(BusTeachingSnapshot {
            accuracy: BusTeachingAccuracy::Exact,
            engine: EmulationEngine::RustCycleAccurate8080,
            instruction_address: Some(self.instruction_address),
            opcode: trace.opcode,
            machine_cycle: trace.machine_cycle.into(),
            machine_cycle_index: Some(trace.machine_cycle_index),
            t_state: trace.t_state.into(),
            address: trace.pins.address,
            data: trace.pins.data_out.or(visible_data),
            status_word,
            pins: BusCpuPins {
                sync: Some(trace.pins.sync),
                dbin: Some(trace.pins.dbin),
                wr_n: Some(trace.pins.wr_n),
                inte: Some(trace.pins.inte),
                wait: Some(trace.pins.wait),
                hlda: Some(trace.pins.hlda),
            },
            status,
            ready: Some(ready),
            interrupt: Some(lines.interrupt),
            hold: Some(lines.hold),
            reset: Some(lines.reset),
            total_t_states: Some(trace.total_t_states),
            instruction_t_states: Some(trace.instruction_t_states),
            instruction_complete: Some(trace.instruction_complete),
            visible_lamps: self.machine.panel_lamps(),
        });
    }

    fn refresh_teaching_visible_lamps(&mut self) {
        if let Some(snapshot) = self.last_teaching_snapshot.as_mut() {
            snapshot.visible_lamps = self.machine.panel_lamps();
        }
    }

    fn tick_once_with_front_panel_data(
        &mut self,
        ready: bool,
        front_panel_data: Option<u8>,
        record_instruction: bool,
    ) -> TickTrace {
        if self.cpu.machine_cycle() == MachineCycle::InstructionFetch
            && self.cpu.t_state() == TState::T1
        {
            self.instruction_address = self.cpu.registers().pc;
        }

        self.machine.bus.refresh_interrupt_request_line();
        let data_in = self.data_in_for_current_t_state(front_panel_data);
        let lines = self.machine.bus.cpu_control_lines();
        let trace = self.cpu.tick(Cpu8080Inputs {
            data_in,
            ready,
            interrupt: lines.interrupt,
            hold: lines.hold,
            reset: lines.reset,
        });
        self.apply_trace_side_effects(&trace, record_instruction);
        let visible_data = self.drive_s100_t_state(&trace, data_in, front_panel_data, ready);
        self.sync_machine_cpu();
        self.capture_teaching_snapshot(&trace, visible_data, ready);

        if trace.t_state == TState::T1
            && trace.pins.sync
            && self.machine.cycle_capture_pending_stop_at_psync()
        {
            self.stop_wait_park_pending = true;
        }
        trace
    }

    fn tick_once(&mut self, ready: bool) -> TickTrace {
        self.tick_once_with_front_panel_data(ready, None, true)
    }

    fn park_physical_stop_at_first_tw(&mut self) {
        self.stop_wait_park_pending = false;
        let lines = self.machine.bus.cpu_control_lines();
        if !self.machine.powered
            || lines.reset
            || lines.ready
            || self.cpu.is_halted()
            || self.cpu.is_holding()
        {
            return;
        }

        for _ in 0..8 {
            let trace = self.tick_once_with_front_panel_data(false, None, false);
            if trace.fault.is_some() || self.cpu.is_halted() || self.cpu.is_holding() {
                break;
            }
            if trace.t_state == TState::Tw {
                break;
            }
        }
        self.refresh_teaching_visible_lamps();
    }

    pub(super) fn debugger_step_t_state_exact(&mut self) -> BackendResult<()> {
        let lines = self.machine.bus.cpu_control_lines();
        if !self.machine.powered
            || self.machine.running
            || lines.reset
            || lines.hold
            || self.cpu.is_halted()
            || self.cpu.is_holding()
        {
            return Ok(());
        }
        let _ = self.tick_once(true);
        self.machine.cycle_set_running(false);
        self.refresh_teaching_visible_lamps();
        Ok(())
    }

    fn at_instruction_boundary(&self) -> bool {
        !self.cpu.is_halted()
            && !self.cpu.is_holding()
            && self.cpu.machine_cycle() == MachineCycle::InstructionFetch
            && self.cpu.t_state() == TState::T1
    }

    pub(super) fn service_execution_with_observer<F>(
        &mut self,
        t_state_budget: u32,
        mut observer: F,
    ) -> BackendResult<()>
    where
        F: FnMut(&mut Self, CycleExecutionEvent) -> bool,
    {
        let lines = self.machine.bus.cpu_control_lines();
        if t_state_budget == 0
            || !self.machine.powered
            || !self.machine.running
            || lines.reset
        {
            return Ok(());
        }

        for _ in 0..t_state_budget {
            if !self.machine.running {
                break;
            }

            if self.at_instruction_boundary() {
                if observer(self, CycleExecutionEvent::BeforeInstruction) {
                    self.machine.cycle_set_running(false);
                    self.refresh_teaching_visible_lamps();
                    break;
                }
                if !self.machine.running {
                    break;
                }
            }

            let ready = self.machine.bus.cpu_control_lines().ready;
            let trace = self.tick_once(ready);
            if trace.fault.is_some() {
                break;
            }
            if self.stop_wait_park_pending {
                self.park_physical_stop_at_first_tw();
                break;
            }

            if trace.instruction_complete
                && observer(self, CycleExecutionEvent::InstructionComplete)
            {
                self.machine.cycle_set_running(false);
                self.refresh_teaching_visible_lamps();
                break;
            }
        }
        Ok(())
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

    fn run_one_machine_cycle_with_front_panel_data(
        &mut self,
        front_panel_data: Option<u8>,
        record_instruction: bool,
    ) {
        let start_cycle = self.cpu.machine_cycle();
        let start_index = self.cpu.machine_cycle_index();
        for _ in 0..32 {
            let trace = self.tick_once_with_front_panel_data(
                true,
                front_panel_data,
                record_instruction,
            );
            if trace.fault.is_some()
                || self.machine_cycle_finished_since(start_cycle, start_index, &trace)
            {
                break;
            }
        }
    }

    fn run_one_machine_cycle(&mut self) {
        self.run_one_machine_cycle_with_front_panel_data(None, true);
    }

    fn park_single_step_at_next_psync_wait(&mut self) {
        let mut saw_psync = false;
        for _ in 0..64 {
            if self.cpu.is_halted() || self.cpu.is_holding() {
                break;
            }

            if self.cpu.t_state() == TState::T1
                && self.cpu.machine_cycle().status_word().is_some()
            {
                let psync = self.tick_once_with_front_panel_data(true, None, true);
                if psync.fault.is_some() {
                    break;
                }
                debug_assert!(psync.pins.sync);
                saw_psync = true;

                self.machine.cycle_set_running(false);
                for _ in 0..8 {
                    let trace = self.tick_once_with_front_panel_data(false, None, false);
                    if trace.fault.is_some() || self.cpu.is_halted() || self.cpu.is_holding() {
                        break;
                    }
                    if trace.t_state == TState::Tw {
                        break;
                    }
                }
                break;
            }

            let trace = self.tick_once_with_front_panel_data(true, None, true);
            if trace.fault.is_some() {
                break;
            }
        }

        if !saw_psync {
            self.machine.cycle_set_running(false);
        }
        self.refresh_teaching_visible_lamps();
    }

    fn park_at_waiting_fetch(&mut self) {
        self.machine.cycle_set_running(false);
        if self.cpu.machine_cycle() == MachineCycle::InstructionFetch {
            for _ in 0..8 {
                let trace = self.tick_once_with_front_panel_data(false, None, false);
                if trace.fault.is_some() || self.cpu.is_halted() || self.cpu.is_holding() {
                    break;
                }
                if trace.t_state == TState::Tw {
                    break;
                }
            }
        }
        self.refresh_teaching_visible_lamps();
    }

    fn front_panel_controls_available(&self) -> bool {
        let lines = self.machine.bus.cpu_control_lines();
        self.machine.powered
            && !self.machine.running
            && !lines.reset
            && !lines.hold
            && !self.cpu.is_halted()
            && !self.cpu.is_holding()
    }

    fn execute_front_panel_examine(&mut self, next: bool) {
        if !self.front_panel_controls_available() {
            return;
        }

        if next {
            self.run_one_machine_cycle_with_front_panel_data(Some(0x00), false);
        } else {
            let address = self.machine.panel_switches();
            for byte in [0xc3, address as u8, (address >> 8) as u8] {
                self.run_one_machine_cycle_with_front_panel_data(Some(byte), false);
            }
        }
        self.park_at_waiting_fetch();
        self.sync_machine_cpu();
    }

    fn execute_front_panel_deposit(&mut self, next: bool) {
        if !self.front_panel_controls_available() {
            return;
        }
        if next {
            self.execute_front_panel_examine(true);
            if !self.front_panel_controls_available() {
                return;
            }
        }
        let address = self.machine.address_leds();
        let value = self.machine.panel_switches() as u8;
        self.machine
            .bus
            .cpu_board_front_panel_deposit(address, value);
        self.sync_machine_cpu();
    }

    fn advance_to_stop_sync(&mut self) {
        if self.cpu.is_halted() || self.cpu.is_holding() {
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

    fn reset_cycle_core_from_s100(&mut self) {
        self.machine.bus.refresh_interrupt_request_line();
        let lines = self.machine.bus.cpu_control_lines();
        let _ = self.cpu.tick(Cpu8080Inputs {
            ready: lines.ready,
            interrupt: lines.interrupt,
            hold: lines.hold,
            reset: lines.reset,
            ..Cpu8080Inputs::default()
        });
        self.stop_wait_park_pending = false;
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
            let registers = Self::cycle_registers_from_fast(&self.machine.cpu);
            let inte = self.machine.cpu.inte;
            self.cpu = Cpu8080Cycle::new();
            self.cpu.initialize_power_on_state(registers, inte);
        } else {
            self.cpu = Cpu8080Cycle::new();
        }
        self.last_teaching_snapshot = None;
        self.stop_wait_park_pending = false;
        self.sync_machine_cpu();
        Ok(())
    }

    fn run(&mut self) -> BackendResult<()> {
        self.machine.cycle_set_running(true);
        Ok(())
    }
    fn halt(&mut self) -> BackendResult<()> {
        self.machine.cycle_set_running(false);
        self.refresh_teaching_visible_lamps();
        Ok(())
    }

    fn step(&mut self) -> BackendResult<()> {
        let lines = self.machine.bus.cpu_control_lines();
        if self.machine.powered && !self.machine.running && !lines.reset && !lines.hold {
            self.run_one_machine_cycle();
            self.park_single_step_at_next_psync_wait();
        }
        Ok(())
    }

    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        let lines = self.machine.bus.cpu_control_lines();
        if self.machine.powered && self.machine.running && !lines.reset {
            for _ in 0..t_state_budget {
                if !self.machine.running {
                    break;
                }
                let ready = self.machine.bus.cpu_control_lines().ready;
                let trace = self.tick_once(ready);
                if trace.fault.is_some() {
                    break;
                }
                if self.stop_wait_park_pending {
                    self.park_physical_stop_at_first_tw();
                    break;
                }
            }
        }
        Ok(())
    }

    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {
        self.sync_machine_cpu();
        self.machine.commit_panel_activity(dt);
        self.refresh_teaching_visible_lamps();
        Ok(())
    }

    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> {
        self.sync_machine_cpu();
        let was_running = self.machine.running;
        if !run
            && self.machine.powered
            && self.machine.running
            && !self.cpu.is_halted()
            && !self.cpu.is_holding()
        {
            self.advance_to_stop_sync();
        }
        self.machine
            .cycle_assert_run_stop(run, self.cpu.is_halted(), self.cpu.is_holding());

        if !run
            && was_running
            && !self.machine.running
            && !self.cpu.is_halted()
            && !self.cpu.is_holding()
        {
            self.park_physical_stop_at_first_tw();
        }
        if !run {
            self.refresh_teaching_visible_lamps();
        }
        Ok(())
    }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> {
        self.machine.release_run_stop(run);
        Ok(())
    }

    fn assert_reset(&mut self) -> BackendResult<()> {
        self.last_teaching_snapshot = None;
        self.stop_wait_park_pending = false;
        self.machine.assert_front_panel_reset();
        self.reset_cycle_core_from_s100();
        Ok(())
    }
    fn release_reset(&mut self) -> BackendResult<()> {
        self.machine.release_front_panel_reset();
        self.stop_wait_park_pending = false;
        self.sync_machine_cpu();
        Ok(())
    }
    fn assert_clear(&mut self) -> BackendResult<()> { self.machine.assert_front_panel_clear(); Ok(()) }
    fn release_clear(&mut self) -> BackendResult<()> { self.machine.release_front_panel_clear(); Ok(()) }

    fn request_hold(&mut self, hold: bool) -> BackendResult<()> {
        self.machine.bus.cycle_set_hold_request(hold);
        Ok(())
    }

    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.execute_front_panel_examine(next);
        Ok(())
    }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        self.execute_front_panel_deposit(next);
        Ok(())
    }
    fn protect_current_board(&mut self, protected: bool) -> BackendResult<()> {
        self.machine.front_panel_set_memory_protection_via_s100(protected);
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
        self.machine.bus.refresh_interrupt_request_line();
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
        let completed = match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_tx_complete(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_complete(),
        };
        self.machine.bus.refresh_interrupt_request_line();
        Ok(completed)
    }
    fn clear_serial(&mut self) -> BackendResult<()> {
        self.machine.bus.clear_serial();
        self.machine.bus.refresh_interrupt_request_line();
        Ok(())
    }

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
        assert_eq!(state.total_t_states, Some(7));
        assert_eq!(backend.machine().cpu.pc, 1, "legacy field must only mirror the cycle core");
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(backend.cpu().t_state(), TState::Tw);
        assert!(backend.machine().wait_led(), "STEP must end on a CPU-generated TW/WAIT sample");
        let teaching = backend.teaching_snapshot().expect("SINGLE STEP must publish its exact TW");
        assert_eq!(teaching.accuracy, BusTeachingAccuracy::Exact);
        assert_eq!(teaching.t_state, TState::Tw.into());
        assert_eq!(teaching.ready, Some(false));
        assert_eq!(teaching.pins.wait, Some(true));
    }

    #[test]
    fn cycle_backend_single_step_advances_one_machine_cycle_not_whole_instruction() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x5a]).unwrap();
        let CpuState::Intel8080(before) = backend.cpu_state().unwrap() else { unreachable!() };

        backend.step().unwrap();
        let CpuState::Intel8080(after_fetch) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(after_fetch.pc, 1);
        assert_eq!(after_fetch.a, before.a, "MVI operand cycle must not have executed yet");
        assert_eq!(after_fetch.total_t_states, Some(7));
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::MemoryRead);
        assert_eq!(backend.cpu().machine_cycle_index(), 2);
        assert_eq!(backend.cpu().t_state(), TState::Tw);

        backend.step().unwrap();
        let CpuState::Intel8080(after_operand) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(after_operand.pc, 2);
        assert_eq!(after_operand.a, 0x5a);
        assert_eq!(after_operand.total_t_states, Some(12));
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(backend.cpu().t_state(), TState::Tw);
        assert!(backend.machine().wait_led());
    }

    #[test]
    fn cycle_backend_single_step_waits_for_real_psync_after_internal_timing() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x09, 0x00]).unwrap();

        backend.step().unwrap();

        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(state.pc, 1);
        assert_eq!(state.total_t_states, Some(13));
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(backend.cpu().t_state(), TState::Tw);
        let teaching = backend.teaching_snapshot().expect("internal timing must end at next exact PSYNC wait");
        assert_eq!(teaching.t_state, TState::Tw.into());
        assert_eq!(teaching.ready, Some(false));
        assert_eq!(teaching.pins.wait, Some(true));
    }

    #[test]
    fn observed_execution_reports_semantic_boundaries_inside_cycle_backend() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00, 0x00, 0x76]).unwrap();
        backend.run().unwrap();

        let mut before_count = 0usize;
        let mut complete_count = 0usize;
        backend
            .service_execution_with_observer(64, |_backend, event| {
                match event {
                    CycleExecutionEvent::BeforeInstruction => before_count += 1,
                    CycleExecutionEvent::InstructionComplete => complete_count += 1,
                }
                false
            })
            .unwrap();

        assert_eq!(before_count, 3);
        assert_eq!(complete_count, 3);
    }

    #[test]
    fn observed_execution_can_stop_before_next_opcode_without_consuming_it() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00, 0x00, 0x76]).unwrap();
        backend.run().unwrap();

        let mut starts = 0usize;
        backend
            .service_execution_with_observer(64, |_backend, event| {
                if event == CycleExecutionEvent::BeforeInstruction {
                    starts += 1;
                    return starts == 2;
                }
                false
            })
            .unwrap();

        assert_eq!(backend.cpu().registers().pc, 0x0001);
        assert!(!backend.machine().running);
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(backend.cpu().t_state(), TState::T1);
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
        backend
            .load_bytes(0, &[0x3e, 0x5a, 0x32, 0x00, 0x1f, 0x00])
            .unwrap();
        for _ in 0..6 {
            backend.step().unwrap();
        }
        assert_eq!(backend.peek_memory(0x1f00).unwrap(), Some(0x5a));
        assert_eq!(backend.peek_memory(0x2000).unwrap(), None);
        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(state.pc, 5);
        assert_eq!(state.total_t_states, Some(33));

        backend.commit_panel_activity(Duration::from_millis(16)).unwrap();
        let panel = backend.front_panel_state().unwrap();
        assert_eq!(panel.address, 0x0005);
        assert_eq!(panel.data, 0x00);
        assert_eq!(panel.lamps.memr, 1.0);
        assert_eq!(panel.lamps.m1, 1.0);
        assert_eq!(panel.lamps.wo, 1.0);
        assert_eq!(panel.lamps.wait, 1.0);
    }

    #[test]
    fn physical_stop_waits_for_next_psync_then_enters_real_tw() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x5a, 0x00]).unwrap();
        let CpuState::Intel8080(before) = backend.cpu_state().unwrap() else { unreachable!() };
        backend.run().unwrap();

        backend.service_execution(2).unwrap();
        assert_eq!(backend.cpu().t_state(), TState::T3);
        backend.assert_run_stop(false).unwrap();

        assert!(!backend.machine().running);
        assert!(!backend.machine().bus.cpu_control_lines().ready);
        assert!(backend.machine().wait_led());
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::MemoryRead);
        assert_eq!(backend.cpu().machine_cycle_index(), 2);
        assert_eq!(backend.cpu().t_state(), TState::Tw);
        let teaching = backend.teaching_snapshot().expect("physical STOP must end on an exact TW");
        assert_eq!(teaching.t_state, TState::Tw.into());
        assert_eq!(teaching.pins.wait, Some(true));
        assert_eq!(teaching.ready, Some(false));

        let CpuState::Intel8080(stopped) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(stopped.pc, 1);
        assert_eq!(stopped.a, before.a);
        assert_eq!(stopped.total_t_states, Some(7));

        backend.release_run_stop(false).unwrap();
        backend.assert_run_stop(true).unwrap();
        assert!(backend.machine().bus.cpu_control_lines().ready);
        assert!(backend.machine().wait_led());
        backend.release_run_stop(true).unwrap();
        backend.service_execution(2).unwrap();
        let CpuState::Intel8080(resumed) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(resumed.pc, 2);
        assert_eq!(resumed.a, 0x5a);
        assert_eq!(resumed.total_t_states, Some(9));
    }

    #[test]
    fn cycle_backend_reset_is_driven_from_shared_s100_control_line() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.load_bytes(0, &[0x00]).unwrap();
        backend.run().unwrap();
        backend.service_execution(4).unwrap();
        assert_ne!(backend.cpu().registers().pc, 0);

        backend.assert_reset().unwrap();
        assert!(backend.machine().bus.cpu_control_lines().reset);
        assert_eq!(backend.cpu().registers().pc, 0);
        assert_eq!(backend.machine().address_leds(), 0xffff);
        assert_eq!(backend.machine().data_leds(), 0xff);

        backend.release_reset().unwrap();
        assert!(!backend.machine().bus.cpu_control_lines().reset);
        assert_eq!(backend.cpu().registers().pc, 0);
    }

    #[test]
    fn cycle_backend_examine_jams_real_jump_and_stops_in_target_fetch_wait() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0x0123, &[0xa5, 0x5a]).unwrap();
        backend.set_switch_register(0x0123).unwrap();

        let before = backend.cpu().total_t_states();
        backend.panel_examine(false).unwrap();
        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(state.pc, 0x0123);
        assert_eq!(state.total_t_states, Some(before + 13));
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(backend.cpu().t_state(), TState::Tw);
        let teaching = backend.teaching_snapshot().expect("EXAMINE must end on a clocked TW sample");
        assert_eq!(teaching.accuracy, BusTeachingAccuracy::Exact);
        assert_eq!(teaching.t_state, TState::Tw.into());
        assert_eq!(teaching.ready, Some(false));
        assert_eq!(teaching.pins.wait, Some(true));

        let panel = backend.front_panel_state().unwrap();
        assert_eq!(panel.address, 0x0123);
        assert_eq!(panel.data, 0xa5);
        assert_eq!(panel.lamps.memr, 1.0);
        assert_eq!(panel.lamps.m1, 1.0);
        assert_eq!(panel.lamps.wo, 1.0);
        assert_eq!(panel.lamps.wait, 1.0);

        backend.panel_examine(true).unwrap();
        let CpuState::Intel8080(next) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(next.pc, 0x0124);
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(backend.cpu().t_state(), TState::Tw);
        let teaching = backend.teaching_snapshot().expect("EXAMINE NEXT must end on a clocked TW sample");
        assert_eq!(teaching.t_state, TState::Tw.into());
        assert_eq!(teaching.ready, Some(false));
        assert_eq!(teaching.pins.wait, Some(true));
        let panel = backend.front_panel_state().unwrap();
        assert_eq!(panel.address, 0x0124);
        assert_eq!(panel.data, 0x5a);
    }

    #[test]
    fn cycle_backend_deposit_uses_front_panel_write_without_assigning_pc() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.set_switch_register(0x0100).unwrap();
        backend.panel_examine(false).unwrap();
        assert_eq!(backend.cpu().registers().pc, 0x0100);

        backend.set_switch_register(0x005a).unwrap();
        backend.panel_deposit(false).unwrap();
        assert_eq!(backend.peek_memory(0x0100).unwrap(), Some(0x5a));
        assert_eq!(backend.cpu().registers().pc, 0x0100);
        assert_eq!(backend.front_panel_state().unwrap().lamps.wo, 0.0);

        backend.panel_deposit(true).unwrap();
        assert_eq!(backend.peek_memory(0x0101).unwrap(), Some(0x5a));
        assert_eq!(backend.cpu().registers().pc, 0x0101);
        assert_eq!(backend.front_panel_state().unwrap().address, 0x0101);
    }

    #[test]
    fn cycle_backend_protect_targets_live_s100_board_and_blocks_deposit() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0x0456, &[0x11]).unwrap();
        backend.set_switch_register(0x0456).unwrap();
        backend.panel_examine(false).unwrap();
        assert_eq!(backend.front_panel_state().unwrap().address, 0x0456);

        backend.set_switch_register(0x0c56).unwrap();
        backend.protect_current_board(true).unwrap();
        assert!(backend.front_panel_state().unwrap().current_board_protected);
        assert!(backend.machine().bus.is_protected(0x0400));
        assert!(!backend.machine().bus.is_protected(0x0c00));

        backend.panel_deposit(false).unwrap();
        assert_eq!(backend.peek_memory(0x0456).unwrap(), Some(0x11));

        backend.protect_current_board(false).unwrap();
        assert!(!backend.front_panel_state().unwrap().current_board_protected);
        backend.panel_deposit(false).unwrap();
        assert_eq!(backend.peek_memory(0x0456).unwrap(), Some(0x56));
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

        backend.service_execution(5).unwrap();
        assert!(backend.cpu().is_holding());
        assert!(backend.machine().bus.raw_s100_hlda());
        backend.commit_panel_activity(Duration::from_millis(16)).unwrap();
        let held = backend.front_panel_state().unwrap();
        assert_eq!(held.lamps.hlda, 1.0);

        backend.request_hold(false).unwrap();
        assert!(!backend.machine().bus.cpu_control_lines().hold);
        assert!(backend.machine().bus.raw_s100_hlda());
        backend.service_execution(1).unwrap();
        assert!(!backend.cpu().is_holding());
        assert!(!backend.machine().bus.raw_s100_hlda());
    }

    #[test]
    fn stop_pressed_during_hlda_waits_for_first_psync_then_real_tw() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00, 0x00]).unwrap();
        backend.run().unwrap();
        backend.request_hold(true).unwrap();
        backend.service_execution(5).unwrap();
        assert!(backend.cpu().is_holding());
        assert!(backend.machine().bus.raw_s100_hlda());

        backend.assert_run_stop(false).unwrap();
        assert!(backend.machine().running);
        assert!(backend.cpu().is_holding());
        assert!(backend.machine().bus.raw_s100_hlda());

        backend.request_hold(false).unwrap();
        assert!(backend.machine().bus.raw_s100_hlda());
        backend.service_execution(1).unwrap();

        assert!(!backend.cpu().is_holding());
        assert!(!backend.machine().running);
        assert!(!backend.machine().bus.raw_s100_hlda());
        assert!(backend.machine().wait_led());
        assert_eq!(backend.cpu().t_state(), TState::Tw);
        let teaching = backend.teaching_snapshot().expect("pending STOP must park on exact TW");
        assert_eq!(teaching.pins.wait, Some(true));
        assert_eq!(teaching.ready, Some(false));

        backend.release_run_stop(false).unwrap();
    }

    #[test]
    fn cycle_backend_io_preview_does_not_consume_serial_before_t3() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0xdb, 0x01]).unwrap();
        backend.serial_receive(BackendSerialPort::Port0, b'R').unwrap();
        backend.run().unwrap();

        backend.service_execution(9).unwrap();
        assert_eq!(backend.serial_rx_len(BackendSerialPort::Port0).unwrap(), 1);

        backend.service_execution(1).unwrap();
        assert_eq!(backend.serial_rx_len(BackendSerialPort::Port0).unwrap(), 0);
        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(state.a, b'R');
        assert_eq!(state.pc, 2);
        assert_eq!(state.total_t_states, Some(10));
    }

    #[test]
    fn cycle_pint_enters_real_inta_t1_and_executes_direct_rst7() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        let mut registers = backend.cpu.registers();
        registers.sp = 0x0400;
        backend.cpu.set_registers(registers);
        backend.sync_machine_cpu();
        backend.load_bytes(0, &[0xfb, 0x00, 0x00]).unwrap();
        backend.machine.bus.debugger_output_port(0x00, 0x01);
        backend.run().unwrap();

        backend.service_execution(8).unwrap();
        assert_eq!(backend.cpu().registers().pc, 0x0002);
        assert!(backend.cpu().interrupts_enabled());

        backend.serial_receive(BackendSerialPort::Port0, b'I').unwrap();
        assert!(backend.machine().bus.cpu_control_lines().interrupt);
        backend.service_execution(1).unwrap();

        let sample = backend.teaching_snapshot().expect("INTA T1 must be visible to the Teacher");
        assert_eq!(sample.machine_cycle, MachineCycle::InterruptAck.into());
        assert_eq!(sample.t_state, TState::T1.into());
        assert_eq!(sample.status_word, Some(0x23));
        assert_eq!(sample.status.int_ack, Some(true));
        assert_eq!(sample.interrupt, Some(true));
        assert_eq!(sample.pins.inte, Some(false));
        assert!(!backend.cpu().interrupts_enabled());
        assert_eq!(backend.cpu().machine_cycle(), MachineCycle::InterruptAck);

        backend.service_execution(10).unwrap();
        assert_eq!(backend.cpu().registers().pc, 0x0038);
        assert_eq!(backend.cpu().registers().sp, 0x03fe);
        assert_eq!(backend.peek_memory(0x03fe).unwrap(), Some(0x02));
        assert_eq!(backend.peek_memory(0x03ff).unwrap(), Some(0x00));
        assert!(backend.machine().bus.cpu_control_lines().interrupt);
    }

    #[test]
    fn cycle_pint_wakes_halt_with_2b_interrupt_ack_status() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        let mut registers = backend.cpu.registers();
        registers.sp = 0x0400;
        backend.cpu.set_registers(registers);
        backend.sync_machine_cpu();
        backend.load_bytes(0, &[0xfb, 0x76]).unwrap();
        backend.machine.bus.debugger_output_port(0x00, 0x01);
        backend.run().unwrap();

        backend.service_execution(11).unwrap();
        assert!(backend.cpu().is_halted());
        assert!(backend.cpu().interrupts_enabled());
        assert_eq!(backend.cpu().registers().pc, 0x0002);

        backend.serial_receive(BackendSerialPort::Port0, b'H').unwrap();
        backend.service_execution(1).unwrap();

        let sample = backend.teaching_snapshot().expect("halt interrupt acknowledge must be sampled");
        assert_eq!(sample.machine_cycle, MachineCycle::InterruptAckWhileHalt.into());
        assert_eq!(sample.t_state, TState::T1.into());
        assert_eq!(sample.status_word, Some(0x2b));
        assert_eq!(sample.status.int_ack, Some(true));
        assert_eq!(sample.interrupt, Some(true));
        assert_eq!(sample.pins.inte, Some(false));
        assert!(!backend.cpu().is_halted());
        assert!(!backend.cpu().interrupts_enabled());

        backend.service_execution(10).unwrap();
        assert_eq!(backend.cpu().registers().pc, 0x0038);
        assert_eq!(backend.cpu().registers().sp, 0x03fe);
    }

    #[test]
    fn exact_teacher_reads_the_same_raw_s100_latch_as_the_panel_bus() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00]).unwrap();

        let _ = backend.tick_once(true);

        let teaching = backend.teaching_snapshot().expect("exact teaching sample");
        assert_eq!(teaching.status_word, Some(backend.machine().bus.raw_s100_status_word()));
        assert_eq!(teaching.status.inte, Some(backend.machine().bus.raw_s100_inte()));
        assert_eq!(teaching.status.prot, Some(backend.machine().bus.raw_s100_prot()));
        assert_eq!(teaching.status.wait, Some(backend.machine().bus.raw_s100_wait()));
        assert_eq!(teaching.status.hlda, Some(backend.machine().bus.raw_s100_hlda()));
        assert_eq!(teaching.interrupt, Some(backend.machine().bus.cpu_control_lines().interrupt));
    }
}

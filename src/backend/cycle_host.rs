use std::time::Duration;

use crate::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};
use crate::cpu8080_cycle::{MachineCycle, TState};
use crate::debugger_control::DebugExecutionControl;
use crate::machine::CpuDiagnosticResult;
use crate::trace8080::{
    collect_post_instruction_effects, collect_pre_instruction_effects, CpuSnapshot8080,
    InstructionEffect8080, InstructionTraceBuffer, InstructionTraceMetadata,
};

use super::cycle::CycleExecutionEvent;
use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, BusChassisSnapshot,
    BusCpuPins, BusMachineCycle, BusStatusLines, BusTeachingAccuracy, BusTeachingSnapshot, BusTState, CpuState,
    CycleAccurateMachineBackend, DebugStopReason, EmulationEngine, FrontPanelState,
    InstructionTraceSnapshot, IoPortActivity, IoTraceSnapshot, MachineBackend, MemoryWatchAccess,
};

#[derive(Clone, Debug)]
struct PendingInstructionTrace {
    address: u16,
    bytes: [u8; 3],
    before: CpuSnapshot8080,
    start_t_states: u64,
    start_completed_instructions: u64,
    effects: Vec<InstructionEffect8080>,
}

pub(super) struct CycleHostBackend {
    inner: CycleAccurateMachineBackend,
    instruction_trace: InstructionTraceBuffer,
    pending_instruction_trace: Option<PendingInstructionTrace>,
    debug_control: DebugExecutionControl,
    teaching_reset_seen: bool,
}

impl Default for CycleHostBackend {
    fn default() -> Self {
        Self {
            inner: CycleAccurateMachineBackend::default(),
            instruction_trace: InstructionTraceBuffer::default(),
            pending_instruction_trace: None,
            debug_control: DebugExecutionControl::default(),
            teaching_reset_seen: false,
        }
    }
}

impl CycleHostBackend {
    fn trace_cpu_snapshot_from(inner: &CycleAccurateMachineBackend) -> CpuSnapshot8080 {
        let r = inner.cpu().registers();
        CpuSnapshot8080 {
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
            inte: inner.cpu().interrupts_enabled(),
            halted: inner.cpu().is_halted(),
        }
    }

    fn trace_bytes_from(inner: &CycleAccurateMachineBackend, address: u16) -> [u8; 3] {
        [
            inner.machine().bus.preview_guest_memory(address),
            inner
                .machine()
                .bus
                .preview_guest_memory(address.wrapping_add(1)),
            inner
                .machine()
                .bus
                .preview_guest_memory(address.wrapping_add(2)),
        ]
    }

    fn at_instruction_boundary(&self) -> bool {
        !self.inner.cpu().is_halted()
            && !self.inner.cpu().is_holding()
            && self.inner.cpu().machine_cycle() == MachineCycle::InstructionFetch
            && self.inner.cpu().t_state() == TState::T1
    }

    fn observing_instruction_effects(&self) -> bool {
        self.instruction_trace.enabled() || self.debug_control.has_watchpoints()
    }

    fn begin_pending_trace(
        inner: &CycleAccurateMachineBackend,
        pending: &mut Option<PendingInstructionTrace>,
    ) {
        if pending.is_some() {
            return;
        }
        let before = Self::trace_cpu_snapshot_from(inner);
        let bytes = Self::trace_bytes_from(inner, before.pc);
        let effects = collect_pre_instruction_effects(bytes, before, |address| {
            inner.machine().bus.preview_guest_memory(address)
        });
        *pending = Some(PendingInstructionTrace {
            address: before.pc,
            bytes,
            before,
            start_t_states: inner.cpu().total_t_states(),
            start_completed_instructions: inner.cpu().completed_instructions(),
            effects,
        });
    }

    fn finalize_pending_trace(
        inner: &CycleAccurateMachineBackend,
        pending: &mut Option<PendingInstructionTrace>,
        instruction_trace: &mut InstructionTraceBuffer,
        debug_control: &mut DebugExecutionControl,
    ) -> bool {
        let Some(mut pending_trace) = pending.take() else {
            return false;
        };
        let after = Self::trace_cpu_snapshot_from(inner);
        let post = collect_post_instruction_effects(
            pending_trace.bytes,
            pending_trace.before,
            after,
            &pending_trace.effects,
        );
        pending_trace.effects.extend(post);
        let elapsed = inner
            .cpu()
            .total_t_states()
            .saturating_sub(pending_trace.start_t_states) as u32;
        let instruction_completed =
            inner.cpu().completed_instructions() > pending_trace.start_completed_instructions;
        let delta = if instruction_completed {
            inner.cpu().last_instruction_t_states().unwrap_or(elapsed)
        } else {
            elapsed
        };
        if delta == 0 {
            return false;
        }

        let watch_stop = debug_control
            .stop_after_effects(pending_trace.address, &pending_trace.effects)
            .is_some();
        if instruction_trace.enabled() {
            instruction_trace.push_with_effects(
                pending_trace.address,
                pending_trace.bytes,
                pending_trace.before,
                after,
                delta,
                pending_trace.effects,
            );
        }
        watch_stop
    }

    fn begin_instruction_trace_if_needed(&mut self) {
        if !self.observing_instruction_effects() || !self.at_instruction_boundary() {
            return;
        }
        Self::begin_pending_trace(&self.inner, &mut self.pending_instruction_trace);
    }

    fn finish_instruction_trace_if_complete(&mut self) -> bool {
        let Some(pending) = self.pending_instruction_trace.as_ref() else {
            return false;
        };
        let instruction_completed =
            self.inner.cpu().completed_instructions() > pending.start_completed_instructions;
        if !instruction_completed && !self.inner.cpu().is_halted() {
            return false;
        }

        Self::finalize_pending_trace(
            &self.inner,
            &mut self.pending_instruction_trace,
            &mut self.instruction_trace,
            &mut self.debug_control,
        )
    }

    fn clear_pending_instruction_trace(&mut self) {
        self.pending_instruction_trace = None;
    }

    /// A trace generation represents one continuous execution context. RESET,
    /// front-panel PC injection, serial-board reset and bulk program replacement
    /// break that continuity. Clearing here prevents Call Stack / Loop Inspector
    /// from treating pre-discontinuity observations as live state afterwards.
    fn reset_debugger_epoch(&mut self) {
        self.clear_pending_instruction_trace();
        self.instruction_trace.clear();
        self.debug_control.clear_transient();
    }

    /// Cycle Accurate can be stopped between machine cycles. If debugger RAM is
    /// patched after an instruction has already been snapshotted, its operands
    /// and predicted data reads may no longer describe what the CPU will really
    /// consume. Drop that partial observation rather than publish a false trace.
    fn invalidate_partial_trace_for_external_memory_change(&mut self) {
        if self.pending_instruction_trace.is_some() {
            self.reset_debugger_epoch();
        }
    }

    fn control_teaching_snapshot(&self) -> BusTeachingSnapshot {
        let machine = self.inner.machine();
        let lines = machine.bus.cpu_control_lines();
        let lamps = machine.panel_lamps();
        let powered = machine.powered;

        let phase = if !powered {
            BusMachineCycle::PowerOff
        } else if lines.reset {
            BusMachineCycle::ResetAsserted
        } else if !self.teaching_reset_seen {
            BusMachineCycle::PowerOnUndefined
        } else if machine.running {
            BusMachineCycle::ResetReleasedRunning
        } else {
            BusMachineCycle::ResetReleasedStopped
        };

        // RAW status always comes from the canonical S100BusState. The lamp
        // snapshot below is optical persistence only and is never reverse-
        // engineered back into an electrical truth value.
        let status_word = powered.then(|| machine.bus.raw_s100_status_word());
        let mut status = BusStatusLines::from_status_word(status_word);
        if powered {
            status.inte = Some(machine.bus.raw_s100_inte());
            status.prot = Some(machine.bus.raw_s100_prot());
            status.wait = Some(machine.bus.raw_s100_wait());
            status.hlda = Some(machine.bus.raw_s100_hlda());
        }

        // A lifecycle/control snapshot must never reuse stale Cpu8080Cycle pin
        // values from a previous T-state. Publish only levels that are physically
        // determined by the current chassis state. RESET RELEASED / STOP-WAIT is
        // a stable read wait: READY is low, WAIT is high, DBIN remains asserted,
        // /WR is inactive and SYNC is low after T1. Other lifecycle phases keep
        // T-state-specific CPU outputs unknown until an actual tick is sampled.
        let pins = match phase {
            BusMachineCycle::PowerOff | BusMachineCycle::PowerOnUndefined => {
                BusCpuPins::default()
            }
            BusMachineCycle::ResetReleasedStopped => BusCpuPins {
                sync: Some(false),
                dbin: Some(true),
                wr_n: Some(true),
                inte: status.inte,
                wait: status.wait,
                hlda: status.hlda,
            },
            _ => BusCpuPins {
                sync: None,
                dbin: None,
                wr_n: None,
                inte: status.inte,
                wait: status.wait,
                hlda: status.hlda,
            },
        };

        let (raw_cpu_data, s100_di, s100_do, panel_data) = if powered {
            (
                machine.bus.raw_cpu_data(),
                machine.bus.raw_s100_data_in(),
                machine.bus.raw_s100_data_out(),
                Some(machine.bus.raw_panel_data()),
            )
        } else {
            (None, None, None, None)
        };
        // Only STOP-WAIT gives us a stable, non-numbered CPU D0-D7 truth. In
        // POWER ON, RESET-held and RESET-released RUN states the chassis may have
        // meaningful DI/DO/display levels while the package D pins remain unknown
        // until a real cycle-core sample exists.
        let cpu_data = if phase == BusMachineCycle::ResetReleasedStopped {
            raw_cpu_data
        } else {
            None
        };

        let r = self.inner.cpu().registers();
        BusTeachingSnapshot {
            accuracy: BusTeachingAccuracy::ControlState,
            engine: EmulationEngine::RustCycleAccurate8080,
            instruction_address: if powered { Some(r.pc) } else { None },
            opcode: None,
            machine_cycle: phase,
            machine_cycle_index: None,
            t_state: BusTState::Unknown,
            address: if powered { Some(machine.address_leds()) } else { None },
            // Keep the legacy display byte until older Teacher call sites have
            // migrated; the four fields below are the new electrical contract.
            data: panel_data,
            cpu_data,
            s100_di,
            s100_do,
            panel_data,
            status_word,
            pins,
            status,
            ready: if powered { Some(lines.ready) } else { None },
            interrupt: if powered { Some(lines.interrupt) } else { None },
            hold: if powered { Some(lines.hold) } else { None },
            reset: if powered { Some(lines.reset) } else { None },
            total_t_states: if powered { Some(self.inner.cpu().total_t_states()) } else { None },
            instruction_t_states: None,
            instruction_complete: None,
            visible_lamps: lamps,
            current_chassis: None,
        }
    }

    fn debugger_step_one_t_state(&mut self) -> BackendResult<()> {
        let lines = self.inner.machine().bus.cpu_control_lines();
        if !self.inner.machine().powered
            || self.inner.machine().running
            || lines.reset
            || lines.hold
            || self.inner.cpu().is_halted()
            || self.inner.cpu().is_holding()
        {
            return Ok(());
        }

        self.debug_control.prepare_manual_step();
        self.begin_instruction_trace_if_needed();
        self.inner.debugger_step_t_state_exact()?;
        self.finish_instruction_trace_if_complete();
        Ok(())
    }

    fn debugger_step_one_machine_cycle(&mut self) -> BackendResult<()> {
        let lines = self.inner.machine().bus.cpu_control_lines();
        if !self.inner.machine().powered
            || self.inner.machine().running
            || lines.reset
            || lines.hold
            || self.inner.cpu().is_halted()
            || self.inner.cpu().is_holding()
        {
            return Ok(());
        }

        self.debug_control.prepare_manual_step();
        self.begin_instruction_trace_if_needed();
        let start_cycle = self.inner.cpu().machine_cycle();
        let start_index = self.inner.cpu().machine_cycle_index();
        let start_t_states = self.inner.cpu().total_t_states();
        for _ in 0..32 {
            self.inner.debugger_step_t_state_exact()?;
            if self.inner.cpu().is_halted() || self.inner.cpu().is_holding() {
                break;
            }
            if self.inner.cpu().machine_cycle() != start_cycle
                || self.inner.cpu().machine_cycle_index() != start_index
                || (self.inner.cpu().t_state() == TState::T1
                    && self.inner.cpu().total_t_states() > start_t_states)
            {
                break;
            }
        }
        self.finish_instruction_trace_if_complete();
        Ok(())
    }

    fn debugger_step_one_instruction(&mut self) -> BackendResult<()> {
        let lines = self.inner.machine().bus.cpu_control_lines();
        if !self.inner.machine().powered
            || self.inner.machine().running
            || lines.reset
            || lines.hold
            || self.inner.cpu().is_halted()
            || self.inner.cpu().is_holding()
        {
            return Ok(());
        }

        self.debug_control.prepare_manual_step();
        self.begin_instruction_trace_if_needed();
        let start_completed = self.inner.cpu().completed_instructions();
        for _ in 0..128 {
            self.inner.debugger_step_t_state_exact()?;
            if self.inner.cpu().completed_instructions() > start_completed
                || self.inner.cpu().is_halted()
                || self.inner.cpu().is_holding()
            {
                break;
            }
        }
        self.finish_instruction_trace_if_complete();
        Ok(())
    }
}

impl MachineBackend for CycleHostBackend {
    fn engine(&self) -> EmulationEngine { self.inner.engine() }
    fn name(&self) -> &'static str { self.inner.name() }
    fn capabilities(&self) -> BackendCapabilities { self.inner.capabilities() }
    fn execution_model(&self) -> BackendExecutionModel { self.inner.execution_model() }

    fn cpu_state(&mut self) -> BackendResult<CpuState> { self.inner.cpu_state() }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { self.inner.front_panel_state() }

    fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {
        let powered = self.inner.machine().powered;
        if powered {
            // Host pause first so RAM reconfiguration cannot leave the physical
            // RUN latch requesting execution while storage is being replaced.
            self.inner.halt()?;
        }

        // Configure the shared RAM object directly. The generic
        // AltairMachine::configure_memory path is intentionally Fast-specific:
        // it resets AltairMachine.cpu. Cycle must never read or mutate that
        // dormant Cpu8080 object.
        self.inner.machine_mut().bus.configure_memory(size, init);
        self.inner.machine_mut().bus.clear_serial();

        if powered {
            self.inner.assert_reset()?;
            self.inner.release_reset()?;
            self.teaching_reset_seen = true;
        } else {
            self.teaching_reset_seen = false;
        }
        self.reset_debugger_epoch();
        Ok(())
    }

    fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) -> BackendResult<()> {
        let powered = self.inner.machine().powered;
        self.inner.machine_mut().configure_memory_board_profile(profile);
        if powered {
            self.inner.assert_reset()?;
            self.inner.release_reset()?;
            self.teaching_reset_seen = true;
        }
        self.reset_debugger_epoch();
        Ok(())
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        self.power_with_historical_run_latch(on, false)
    }
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> {
        self.inner.power_with_historical_run_latch(on, historical)?;
        self.teaching_reset_seen = false;
        self.reset_debugger_epoch();
        Ok(())
    }
    fn run(&mut self) -> BackendResult<()> {
        let r = self.inner.cpu().registers();
        self.debug_control.prepare_resume_with_sp(r.pc, r.sp);
        self.inner.run()
    }
    fn halt(&mut self) -> BackendResult<()> {
        self.debug_control.cancel_run_to();
        self.inner.halt()
    }
    fn step(&mut self) -> BackendResult<()> {
        self.debug_control.prepare_manual_step();
        self.begin_instruction_trace_if_needed();
        self.inner.step()?;
        self.finish_instruction_trace_if_complete();
        Ok(())
    }
    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        if !self.instruction_trace.enabled() && !self.debug_control.active() {
            return self.inner.service_execution(t_state_budget);
        }

        let observing_effects = self.observing_instruction_effects();
        let pending = &mut self.pending_instruction_trace;
        let instruction_trace = &mut self.instruction_trace;
        let debug_control = &mut self.debug_control;

        self.inner
            .service_execution_with_observer(t_state_budget, |inner, event| match event {
                CycleExecutionEvent::BeforeInstruction => {
                    let r = inner.cpu().registers();
                    if debug_control.stop_before_with_sp(r.pc, r.sp).is_some() {
                        return true;
                    }
                    if observing_effects {
                        Self::begin_pending_trace(inner, pending);
                    }
                    false
                }
                CycleExecutionEvent::InstructionComplete => Self::finalize_pending_trace(
                    inner,
                    pending,
                    instruction_trace,
                    debug_control,
                ),
            })
    }
    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {
        self.inner.commit_panel_activity(dt)
    }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> {
        if run {
            let r = self.inner.cpu().registers();
            self.debug_control.prepare_resume_with_sp(r.pc, r.sp);
        } else {
            self.debug_control.cancel_run_to();
        }
        self.inner.assert_run_stop(run)
    }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> { self.inner.release_run_stop(run) }
    fn assert_reset(&mut self) -> BackendResult<()> {
        self.reset_debugger_epoch();
        self.teaching_reset_seen = true;
        self.inner.assert_reset()
    }
    fn release_reset(&mut self) -> BackendResult<()> { self.inner.release_reset() }
    fn assert_clear(&mut self) -> BackendResult<()> { self.inner.assert_clear() }
    fn release_clear(&mut self) -> BackendResult<()> { self.inner.release_clear() }
    fn request_hold(&mut self, hold: bool) -> BackendResult<()> { self.inner.request_hold(hold) }
    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.reset_debugger_epoch();
        self.inner.panel_examine(next)
    }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        if next {
            self.reset_debugger_epoch();
        } else {
            self.invalidate_partial_trace_for_external_memory_change();
        }
        self.inner.panel_deposit(next)
    }
    fn protect_current_board(&mut self, protected: bool) -> BackendResult<()> {
        self.inner.protect_current_board(protected)
    }
    fn switch_register(&mut self) -> BackendResult<u16> { self.inner.switch_register() }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        self.inner.set_switch_register(value)
    }
    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        if self.inner.machine().serial_board() == board {
            return Ok(());
        }
        let powered = self.inner.machine().powered;
        self.reset_debugger_epoch();
        self.inner.machine_mut().configure_serial_board(board);
        if powered {
            self.inner.assert_reset()?;
            self.inner.release_reset()?;
            self.teaching_reset_seen = true;
        }
        Ok(())
    }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> { self.inner.serial_board() }
    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        self.inner.serial_receive(port, byte)
    }
    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        self.inner.serial_rx_empty(port)
    }
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> {
        self.inner.serial_rx_len(port)
    }
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        self.inner.serial_tx_busy(port)
    }
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        self.inner.serial_tx_front(port)
    }
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        self.inner.serial_tx_complete(port)
    }
    fn clear_serial(&mut self) -> BackendResult<()> { self.inner.clear_serial() }
    fn installed_ram_bytes(&mut self) -> BackendResult<usize> { Ok(self.inner.machine().installed_ram_bytes()) }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        self.inner.peek_memory(address)
    }
    fn write_memory(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> BackendResult<bool> {
        self.invalidate_partial_trace_for_external_memory_change();
        self.inner.write_memory(address, value, respect_protection)
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.inner.load_bytes(address, bytes)?;
        self.reset_debugger_epoch();
        Ok(())
    }
    fn memory_is_protected(&mut self, address: u16) -> BackendResult<bool> {
        Ok(self.inner.machine().bus.is_protected(address))
    }
    fn clear_memory_protection(&mut self) -> BackendResult<()> {
        self.inner.machine_mut().bus.clear_protection();
        Ok(())
    }
    fn clear_transient_memory_guards(&mut self) -> BackendResult<()> {
        self.invalidate_partial_trace_for_external_memory_change();
        self.inner.machine_mut().bus.clear_transient_memory_guards();
        Ok(())
    }
    fn arm_basic32_full_memory_probe_guard(&mut self) -> BackendResult<bool> {
        self.invalidate_partial_trace_for_external_memory_change();
        Ok(self.inner.machine_mut().arm_basic32_full_memory_probe_guard())
    }
    fn begin_cpu_diagnostic_meter(
        &mut self,
        name: String,
        bdos_start: u16,
        bdos_len: usize,
        expected_instructions: Option<u64>,
        expected_t_states: Option<u64>,
    ) -> BackendResult<()> {
        self.inner.machine_mut().begin_cpu_diagnostic_meter(
            name,
            bdos_start,
            bdos_len,
            expected_instructions,
            expected_t_states,
        );
        Ok(())
    }
    fn cancel_cpu_diagnostic_meter(&mut self) -> BackendResult<()> {
        self.inner.machine_mut().bus.cancel_cpu_diagnostic_meter();
        Ok(())
    }
    fn take_cpu_diagnostic_result(&mut self) -> BackendResult<Option<CpuDiagnosticResult>> {
        Ok(self.inner.machine_mut().take_cpu_diagnostic_result())
    }
    fn peek_io_port(&mut self, port: u8) -> BackendResult<u8> { Ok(self.inner.machine().bus.peek_io_port(port)) }
    fn io_port_activity(&mut self, port: u8) -> BackendResult<IoPortActivity> {
        Ok(self.inner.machine().bus.io_port_activity(port))
    }
    fn io_trace_snapshot(&mut self) -> BackendResult<IoTraceSnapshot> {
        Ok(self.inner.machine().bus.io_trace_snapshot())
    }
    fn io_trace_enabled(&mut self) -> BackendResult<bool> {
        Ok(self.inner.machine().bus.io_trace_enabled())
    }
    fn set_io_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> {
        self.inner.machine_mut().bus.set_io_trace_enabled(enabled);
        Ok(())
    }
    fn clear_io_trace(&mut self) -> BackendResult<()> {
        self.inner.machine_mut().bus.clear_io_trace();
        Ok(())
    }
    fn instruction_trace_snapshot(&mut self) -> BackendResult<InstructionTraceSnapshot> {
        Ok(self.instruction_trace.snapshot())
    }
    fn instruction_trace_enabled(&mut self) -> BackendResult<bool> { Ok(self.instruction_trace.enabled()) }
    fn instruction_trace_metadata(&mut self) -> BackendResult<InstructionTraceMetadata> {
        Ok(self.instruction_trace.metadata())
    }
    fn set_instruction_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> {
        self.instruction_trace.set_enabled(enabled);
        if !enabled {
            self.clear_pending_instruction_trace();
        }
        Ok(())
    }
    fn clear_instruction_trace(&mut self) -> BackendResult<()> {
        self.clear_pending_instruction_trace();
        self.instruction_trace.clear();
        Ok(())
    }
    fn bus_teaching_snapshot(&mut self) -> BackendResult<Option<BusTeachingSnapshot>> {
        let mut snapshot = self
            .inner
            .teaching_snapshot()
            .unwrap_or_else(|| self.control_teaching_snapshot());
        snapshot.current_chassis = Some(BusChassisSnapshot::from_altair_machine(
            EmulationEngine::RustCycleAccurate8080,
            self.inner.machine(),
        ));
        Ok(Some(snapshot))
    }
    fn debugger_step_t_state(&mut self) -> BackendResult<()> { self.debugger_step_one_t_state() }
    fn debugger_step_machine_cycle(&mut self) -> BackendResult<()> { self.debugger_step_one_machine_cycle() }
    fn debugger_step_instruction(&mut self) -> BackendResult<()> { self.debugger_step_one_instruction() }
    fn debugger_breakpoints(&mut self) -> BackendResult<Vec<u16>> { Ok(self.debug_control.breakpoints()) }
    fn debugger_set_breakpoint(&mut self, address: u16, enabled: bool) -> BackendResult<()> {
        self.debug_control.set_breakpoint(address, enabled);
        Ok(())
    }
    fn debugger_clear_breakpoints(&mut self) -> BackendResult<()> {
        self.debug_control.clear_breakpoints();
        Ok(())
    }
    fn debugger_watchpoints(&mut self) -> BackendResult<Vec<(u16, MemoryWatchAccess)>> {
        Ok(self.debug_control.watchpoints())
    }
    fn debugger_set_watchpoint(
        &mut self,
        address: u16,
        access: Option<MemoryWatchAccess>,
    ) -> BackendResult<()> {
        self.debug_control.set_watchpoint(address, access);
        Ok(())
    }
    fn debugger_clear_watchpoints(&mut self) -> BackendResult<()> {
        self.debug_control.clear_watchpoints();
        Ok(())
    }
    fn debugger_run_to(&mut self, address: u16) -> BackendResult<()> {
        self.debug_control.set_run_to(address);
        self.run()
    }
    fn debugger_run_to_with_sp(&mut self, address: u16, required_sp: u16) -> BackendResult<()> {
        self.debug_control.set_run_to_with_sp(address, required_sp);
        self.run()
    }
    fn debugger_cancel_run_to(&mut self) -> BackendResult<()> {
        self.debug_control.cancel_run_to();
        Ok(())
    }
    fn debugger_run_to_target(&mut self) -> BackendResult<Option<u16>> {
        Ok(self.debug_control.run_to())
    }
    fn debugger_stop_reason(&mut self) -> BackendResult<Option<DebugStopReason>> {
        Ok(self.debug_control.stop_reason())
    }
    fn debugger_input_port(&mut self, port: u8) -> BackendResult<u8> {
        Ok(self.inner.machine_mut().bus.debugger_input_port(port))
    }
    fn debugger_output_port(&mut self, port: u8, value: u8) -> BackendResult<()> {
        self.inner.machine_mut().bus.debugger_output_port(port, value);
        Ok(())
    }
    fn debugger_inject_serial_rx(&mut self, port: u8, byte: u8) -> BackendResult<bool> {
        Ok(self.inner.machine_mut().bus.debugger_inject_serial_rx(port, byte))
    }
    fn debugger_clear_serial_rx(&mut self, port: u8) -> BackendResult<bool> {
        Ok(self.inner.machine_mut().bus.debugger_clear_serial_rx(port))
    }
    fn debugger_clear_serial_tx(&mut self, port: u8) -> BackendResult<bool> {
        Ok(self.inner.machine_mut().bus.debugger_clear_serial_tx(port))
    }
    fn debugger_complete_serial_tx(&mut self, port: u8) -> BackendResult<Option<u8>> {
        Ok(self.inner.machine_mut().bus.debugger_complete_serial_tx(port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_teacher_exposes_power_and_reset_without_fake_t_states() {
        let mut backend = CycleHostBackend::default();
        backend.power(true).unwrap();
        let on = backend.bus_teaching_snapshot().unwrap().unwrap();
        assert_eq!(on.accuracy, BusTeachingAccuracy::ControlState);
        assert_eq!(on.machine_cycle, BusMachineCycle::PowerOnUndefined);
        assert_eq!(on.t_state, BusTState::Unknown);
        assert_eq!(on.ready, Some(false));
        assert_eq!(on.status.inte, Some(backend.inner.cpu().interrupts_enabled()));
        assert_eq!(on.status.wait, Some(true));
        assert_eq!(on.pins.wait, None);
        assert_eq!(on.status_word, Some(0xa2));
        assert_eq!(on.cpu_data, None);
        assert!(on.s100_di.is_some());
        assert_eq!(on.s100_do, None);
        assert!(on.panel_data.is_some());
        assert_eq!(on.data, on.panel_data);

        backend.assert_reset().unwrap();
        let held = backend.bus_teaching_snapshot().unwrap().unwrap();
        assert_eq!(held.machine_cycle, BusMachineCycle::ResetAsserted);
        assert_eq!(held.t_state, BusTState::Unknown);
        assert_eq!(held.address, Some(0xffff));
        assert_eq!(held.data, Some(0xff));
        assert_eq!(held.cpu_data, None);
        assert_eq!(held.s100_di, Some(0xff));
        assert_eq!(held.s100_do, None);
        assert_eq!(held.panel_data, Some(0xff));
        assert_eq!(held.status.inte, Some(false));
        assert_eq!(held.pins.inte, Some(false));
        assert_eq!(held.status.wait, Some(false));
        assert_eq!(held.pins.wait, Some(false));
        assert_eq!(held.reset, Some(true));

        backend.release_reset().unwrap();
        let stopped = backend.bus_teaching_snapshot().unwrap().unwrap();
        assert_eq!(stopped.machine_cycle, BusMachineCycle::ResetReleasedStopped);
        assert_eq!(stopped.t_state, BusTState::Unknown);
        assert_eq!(stopped.address, Some(0));
        assert_eq!(stopped.ready, Some(false));
        assert_eq!(stopped.status.memr, Some(true));
        assert_eq!(stopped.status.m1, Some(true));
        assert_eq!(stopped.status.wo, Some(true));
        assert_eq!(stopped.status.wait, Some(true));
        assert_eq!(stopped.pins.wait, Some(true));
        assert_eq!(stopped.cpu_data, stopped.s100_di);
        assert_eq!(stopped.s100_do, None);
        assert_eq!(stopped.data, stopped.panel_data);
    }

    #[test]
    fn lifecycle_teacher_raw_status_and_data_are_not_derived_from_optical_lamps() {
        let mut backend = CycleHostBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.inner.machine_mut().bus.load(0, &[0xa5]);
        // Rebuild the stable STOP-WAIT bus state so DI reflects the newly loaded
        // memory byte while the optical snapshot is then deliberately corrupted.
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.inner.machine_mut().bus.debug_set_panel_lamp_snapshot_for_test(
            crate::machine::PanelLampSnapshot::default(),
        );

        let snapshot = backend.bus_teaching_snapshot().unwrap().unwrap();
        assert_eq!(snapshot.status_word, Some(0xa2));
        assert_eq!(snapshot.status.memr, Some(true));
        assert_eq!(snapshot.status.m1, Some(true));
        assert_eq!(snapshot.status.wo, Some(true));
        assert_eq!(snapshot.status.wait, Some(true));
        assert_eq!(snapshot.cpu_data, Some(0xa5));
        assert_eq!(snapshot.s100_di, Some(0xa5));
        assert_eq!(snapshot.s100_do, None);
        assert_eq!(snapshot.panel_data, Some(0xa5));
        assert_eq!(snapshot.visible_lamps.memr, 0.0);
        assert_eq!(snapshot.visible_lamps.m1, 0.0);
        assert_eq!(snapshot.visible_lamps.wait, 0.0);
    }

    #[test]
    fn powered_serial_board_change_resets_real_cycle_core() {
        let mut backend = CycleHostBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.inner.machine_mut().bus.load(0, &[0x00]);
        backend.run().unwrap();
        backend.service_execution(4).unwrap();
        assert_ne!(backend.inner.cpu().registers().pc, 0);

        backend.configure_serial_board(SerialBoard::TwoSio88).unwrap();
        assert_eq!(backend.inner.cpu().registers().pc, 0);
        assert_eq!(backend.inner.machine().serial_board(), SerialBoard::TwoSio88);
    }

    #[test]
    fn wrapper_dispatches_cycle_step_without_exposing_chassis_to_app() {
        let mut backend = CycleHostBackend::default();
        backend.configure_memory(RamSize::K1, RamInit::Zeroed).unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00]).unwrap();
        backend.step().unwrap();
        let CpuState::Intel8080(state) = backend.cpu_state().unwrap();
        assert_eq!(state.pc, 1);
        assert_eq!(state.total_t_states, Some(7));
        assert_eq!(backend.inner.cpu().t_state(), TState::Tw);
    }

    #[test]
    fn debugger_memory_patch_between_machine_cycles_drops_stale_partial_trace() {
        let mut backend = CycleHostBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x11]).unwrap();
        backend.set_instruction_trace_enabled(true).unwrap();

        backend.debugger_step_one_machine_cycle().unwrap();
        assert!(backend.pending_instruction_trace.is_some());
        assert!(backend.write_memory(1, 0x22, false).unwrap());
        assert!(backend.pending_instruction_trace.is_none());
        assert!(backend.instruction_trace.snapshot().is_empty());
    }

    #[test]
    fn front_panel_deposit_between_machine_cycles_drops_stale_partial_trace() {
        let mut backend = CycleHostBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x11]).unwrap();
        backend.set_instruction_trace_enabled(true).unwrap();

        backend.debugger_step_one_machine_cycle().unwrap();
        assert!(backend.pending_instruction_trace.is_some());
        backend.set_switch_register(0x0022).unwrap();
        backend.panel_deposit(false).unwrap();
        assert!(backend.pending_instruction_trace.is_none());
        assert!(backend.instruction_trace.snapshot().is_empty());
    }

    #[test]
    fn cycle_history_records_complete_guest_instruction_boundaries() {
        let mut backend = CycleHostBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x5a, 0x00]).unwrap();
        backend.set_instruction_trace_enabled(true).unwrap();
        backend.run().unwrap();
        backend.service_execution(16).unwrap();
        let history = backend.instruction_trace.snapshot();
        assert!(history.len() >= 2);
        assert_eq!(history[0].address, 0);
        assert_eq!(history[0].bytes[0], 0x3e);
        assert_eq!(history[0].after.a, 0x5a);
        assert_eq!(history[0].t_states, 7);
        assert_eq!(history[1].address, 2);
        assert_eq!(history[1].bytes[0], 0x00);
        assert_eq!(history[1].t_states, 4);
    }
}

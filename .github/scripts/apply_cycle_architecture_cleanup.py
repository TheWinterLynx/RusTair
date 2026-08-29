from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"anchor not found in {path}: {old[:180]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


cycle = "src/backend/cycle.rs"

replace_once(
    cycle,
    '''    Cpu8080Cycle, Cpu8080Inputs, MachineCycle, Registers, TState, TickTrace,\n''',
    '''    Cpu8080Cycle, Cpu8080CycleFault, Cpu8080Inputs, MachineCycle, Registers, TState, TickTrace,\n''',
)
replace_once(
    cycle,
    '''    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, BusCpuPins,\n''',
    '''    BackendCapabilities, BackendError, BackendExecutionModel, BackendResult, BackendSerialPort, BusCpuPins,\n''',
)
replace_once(
    cycle,
    '''    stop_wait_park_pending: bool,\n}\n''',
    '''    stop_wait_park_pending: bool,\n    /// Latched exact-core fault. Unlike a TickTrace observation this persists\n    /// until RESET/power recovery so callers cannot silently resume execution.\n    cpu_fault: Option<Cpu8080CycleFault>,\n}\n''',
)
replace_once(
    cycle,
    '''            stop_wait_park_pending: false,\n        };\n''',
    '''            stop_wait_park_pending: false,\n            cpu_fault: None,\n        };\n''',
)
replace_once(
    cycle,
    '''    fn cycle_registers_from_fast(cpu: &Cpu8080) -> Registers {\n''',
    '''    fn backend_error_for_cycle_fault(\n        operation: &'static str,\n        fault: Cpu8080CycleFault,\n    ) -> BackendError {\n        BackendError::Operation {\n            operation,\n            detail: format!("cycle-accurate 8080 fault: {fault:?}"),\n        }\n    }\n\n    fn fail_if_cpu_fault(&mut self, operation: &'static str) -> BackendResult<()> {\n        if let Some(fault) = self.cpu_fault {\n            // A CPU fault is a stopped/error condition, not a successful short\n            // execution slice. Keep it latched until RESET or power recovery.\n            self.machine.cycle_set_running(false);\n            self.refresh_teaching_visible_lamps();\n            Err(Self::backend_error_for_cycle_fault(operation, fault))\n        } else {\n            Ok(())\n        }\n    }\n\n    fn cycle_registers_from_fast(cpu: &Cpu8080) -> Registers {\n''',
)
replace_once(
    cycle,
    '''        let trace = self.cpu.tick(Cpu8080Inputs {\n            data_in,\n''',
    '''        let trace = self.cpu.tick(Cpu8080Inputs {\n            data_in,\n''',
)
replace_once(
    cycle,
    '''            reset: lines.reset,\n        });\n        self.apply_trace_side_effects(&trace, record_instruction);\n''',
    '''            reset: lines.reset,\n        });\n        if let Some(fault) = trace.fault {\n            self.cpu_fault = Some(fault);\n        }\n        self.apply_trace_side_effects(&trace, record_instruction);\n''',
)
replace_once(
    cycle,
    '''        let _ = self.tick_once(true);\n        // Debugger pause is not a physical STOP transition. Lower READY without\n''',
    '''        let trace = self.tick_once(true);\n        if trace.fault.is_some() {\n            return self.fail_if_cpu_fault("debugger T-state step");\n        }\n        // Debugger pause is not a physical STOP transition. Lower READY without\n''',
)
replace_once(
    cycle,
    '''            let trace = self.tick_once(ready);\n            if trace.fault.is_some() {\n                break;\n            }\n            if self.stop_wait_park_pending {\n''',
    '''            let trace = self.tick_once(ready);\n            if trace.fault.is_some() {\n                return self.fail_if_cpu_fault("service execution");\n            }\n            if self.stop_wait_park_pending {\n''',
)
# There are two execution loops with the same old fault break. Replace the
# remaining MachineBackend service loop occurrence too.
replace_once(
    cycle,
    '''                let trace = self.tick_once(ready);\n                if trace.fault.is_some() {\n                    break;\n                }\n                if self.stop_wait_park_pending {\n''',
    '''                let trace = self.tick_once(ready);\n                if trace.fault.is_some() {\n                    return self.fail_if_cpu_fault("service execution");\n                }\n                if self.stop_wait_park_pending {\n''',
)
replace_once(
    cycle,
    '''    fn reset_cycle_core_from_s100(&mut self) {\n        self.machine.bus.refresh_interrupt_request_line();\n''',
    '''    fn reset_cycle_core_from_s100(&mut self) {\n        // RESET is the recovery boundary for a latched exact-core fault.\n        self.cpu_fault = None;\n        self.machine.bus.refresh_interrupt_request_line();\n''',
)
replace_once(
    cycle,
    '''        self.last_teaching_snapshot = None;\n        self.stop_wait_park_pending = false;\n        self.sync_machine_cpu();\n''',
    '''        self.last_teaching_snapshot = None;\n        self.stop_wait_park_pending = false;\n        self.cpu_fault = None;\n        self.sync_machine_cpu();\n''',
)
replace_once(
    cycle,
    '''    fn run(&mut self) -> BackendResult<()> {\n        self.machine.cycle_set_running(true);\n        Ok(())\n    }\n''',
    '''    fn run(&mut self) -> BackendResult<()> {\n        self.fail_if_cpu_fault("run")?;\n        self.machine.cycle_set_running(true);\n        Ok(())\n    }\n''',
)
replace_once(
    cycle,
    '''    fn step(&mut self) -> BackendResult<()> {\n        let lines = self.machine.bus.cpu_control_lines();\n        if self.machine.powered && !self.machine.running && !lines.reset && !lines.hold {\n            self.run_one_machine_cycle();\n            self.park_single_step_at_next_psync_wait();\n        }\n        Ok(())\n    }\n''',
    '''    fn step(&mut self) -> BackendResult<()> {\n        self.fail_if_cpu_fault("single step")?;\n        let lines = self.machine.bus.cpu_control_lines();\n        if self.machine.powered && !self.machine.running && !lines.reset && !lines.hold {\n            self.run_one_machine_cycle();\n            self.park_single_step_at_next_psync_wait();\n        }\n        self.fail_if_cpu_fault("single step")\n    }\n''',
)
replace_once(
    cycle,
    '''        }\n        Ok(())\n    }\n\n    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {\n''',
    '''        }\n        self.fail_if_cpu_fault("service execution")\n    }\n\n    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {\n''',
)
replace_once(
    cycle,
    '''        if !run\n            && self.machine.powered\n            && self.machine.running\n            && !self.cpu.is_halted()\n            && !self.cpu.is_holding()\n        {\n            self.advance_to_stop_sync();\n        }\n        self.machine\n''',
    '''        if !run\n            && self.machine.powered\n            && self.machine.running\n            && !self.cpu.is_halted()\n            && !self.cpu.is_holding()\n        {\n            self.advance_to_stop_sync();\n            self.fail_if_cpu_fault("RUN/STOP")?;\n        }\n        self.machine\n''',
)
replace_once(
    cycle,
    '''        if !run {\n            self.refresh_teaching_visible_lamps();\n        }\n        Ok(())\n    }\n''',
    '''        if !run {\n            self.refresh_teaching_visible_lamps();\n        }\n        self.fail_if_cpu_fault("RUN/STOP")\n    }\n''',
)
replace_once(
    cycle,
    '''    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {\n        self.execute_front_panel_examine(next);\n        Ok(())\n    }\n    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {\n        self.execute_front_panel_deposit(next);\n        Ok(())\n    }\n''',
    '''    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {\n        self.fail_if_cpu_fault("front-panel EXAMINE")?;\n        self.execute_front_panel_examine(next);\n        self.fail_if_cpu_fault("front-panel EXAMINE")\n    }\n    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {\n        self.fail_if_cpu_fault("front-panel DEPOSIT")?;\n        self.execute_front_panel_deposit(next);\n        self.fail_if_cpu_fault("front-panel DEPOSIT")\n    }\n''',
)
# Add a direct mapping regression even though the current decoder deliberately
# gives all 256 byte values silicon behavior and cannot naturally fault today.
replace_once(
    cycle,
    '''mod tests {\n    use super::*;\n\n''',
    '''mod tests {\n    use super::*;\n\n    #[test]\n    fn cycle_fault_maps_to_explicit_backend_error() {\n        let error = CycleAccurateMachineBackend::backend_error_for_cycle_fault(\n            "test execution",\n            Cpu8080CycleFault::UnsupportedOpcode(0xdd),\n        );\n        assert_eq!(\n            error,\n            BackendError::Operation {\n                operation: "test execution",\n                detail: "cycle-accurate 8080 fault: UnsupportedOpcode(221)".into(),\n            }\n        );\n    }\n\n''',
)

# Unpowered RAM changes must mutate the existing chassis/backend instead of
# replacing it with a new CycleAccurateMachineBackend and losing unrelated state.
replace_once(
    "src/backend/cycle_host.rs",
    '''    fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {\n        let powered = self.inner.machine().powered;\n        let serial_board = self.inner.machine().serial_board();\n        let memory_profile = self\n            .inner\n            .machine()\n            .memory_board_profile(0)\n            .unwrap_or_default();\n\n        if powered {\n            self.inner.machine_mut().configure_memory(size, init);\n            self.inner.assert_reset()?;\n            self.inner.release_reset()?;\n            self.teaching_reset_seen = true;\n        } else {\n            let mut replacement = CycleAccurateMachineBackend::default();\n            replacement.machine_mut().configure_memory(size, init);\n            replacement.machine_mut().configure_memory_board_profile(memory_profile);\n            replacement.machine_mut().configure_serial_board(serial_board);\n            self.inner = replacement;\n            self.teaching_reset_seen = false;\n        }\n        self.reset_debugger_epoch();\n        Ok(())\n    }\n''',
    '''    fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {\n        let powered = self.inner.machine().powered;\n        self.inner.machine_mut().configure_memory(size, init);\n\n        if powered {\n            self.inner.assert_reset()?;\n            self.inner.release_reset()?;\n            self.teaching_reset_seen = true;\n        } else {\n            // Keep the existing chassis, serial-board choice, sense switches and\n            // memory-card timing profile. Replacing the whole backend here made\n            // a RAM-capacity setting an accidental machine factory reset.\n            self.teaching_reset_seen = false;\n        }\n        self.reset_debugger_epoch();\n        Ok(())\n    }\n''',
)

# Add a public-path regression for the state that the old replacement silently
# discarded. The RAM timing profile is preserved by Memory::configure itself.
Path("tests/cycle_memory_reconfigure.rs").write_text('''use rustair::backend::{BackendHost, EmulationEngine};\nuse rustair::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};\n\n#[test]\nfn unpowered_cycle_memory_reconfigure_preserves_unrelated_chassis_configuration() {\n    let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080)\n        .expect("cycle backend must be built in");\n\n    host.configure_serial_board(SerialBoard::TwoSio88);\n    host.configure_memory_board_profile(RamBoardProfile::Mits1KStatic1975);\n    host.set_switch_register(0xa55a);\n\n    host.configure_memory(RamSize::K16, RamInit::Zeroed);\n\n    assert_eq!(host.installed_ram_bytes(), 16 * 1024);\n    assert_eq!(host.serial_board(), SerialBoard::TwoSio88);\n    assert_eq!(host.switch_register(), 0xa55a);\n}\n''', encoding="utf-8")

# Small dead-code cleanup left by completed fidelity migrations.
replace_once(
    "src/machine/mod.rs",
    '''    pub(crate) fn interrupt_requested(&self) -> bool {\n        self.s100.signals().interrupt\n    }\n\n''',
    '''''',
)
replace_once(
    "src/machine/cpu_board.rs",
    '''    /// Compatibility entry used by ordinary Cycle transfers while the exact\n    /// front-panel-direct marker is migrated in the next checkpoint.\n    pub(crate) fn sample(\n        trace: &TickTrace,\n        visible_data: Option<u8>,\n        ready: bool,\n    ) -> S100CpuSample {\n        Self::sample_with_front_panel_direct(trace, visible_data, false, ready)\n    }\n\n''',
    '''''',
)
replace_once(
    "tests/backend_authority.rs",
    '''use rustair::cpu8080_cycle::{MachineCycle, TState};\n''',
    '''''',
)
replace_once(
    "src/app/ui/cpu_pin_diagram.rs",
    '''    Ground,\n    Unmodeled(&'static str),\n}\n''',
    '''    Ground,\n}\n''',
)
replace_once(
    "src/app/ui/cpu_pin_diagram.rs",
    '''        PinKind::Unmodeled(note) => PinState {\n            level: None,\n            asserted: None,\n            state_text: if powered { "NOT WIRED / NOT MODELED".into() } else { "UNPOWERED".into() },\n            note,\n            modeled: false,\n            static_pin: false,\n            released: false,\n        },\n''',
    '''''',
)

# Remove this one-shot helper from the source commit.
Path(__file__).unlink()

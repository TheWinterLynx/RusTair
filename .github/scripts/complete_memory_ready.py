from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"expected migration anchor not found in {path}: {old[:160]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


def append_once(path: str, marker: str, text_to_append: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if marker in text:
        return
    p.write_text(text + text_to_append, encoding="utf-8")


# ---------------------------------------------------------------------------
# Configuration: the physical RAM card type is separate from capacity/content.
# ---------------------------------------------------------------------------
replace_once(
    "src/config/machine.rs",
    '''impl Default for RamInit {
    fn default() -> Self {
        Self::Random
    }
}

/// MITS serial interface installed in the emulated Altair.''',
    '''impl Default for RamInit {
    fn default() -> Self {
        Self::Random
    }
}

/// Electrical/timing profile of the installed S-100 RAM cards.
///
/// Capacity and initial contents are deliberately separate from card timing: an
/// Altair can have the same number of bytes implemented by very different boards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RamBoardProfile {
    /// Compatibility profile for later/fast memory: no PRDY stretching.
    FastNoWait,
    /// Original MITS 1K Static Memory Board using Intel 8101 RAMs. The 1975
    /// Theory of Operation specifies two wait cycles (1 us at 2 MHz) on reads.
    Mits1KStatic1975,
}

impl RamBoardProfile {
    pub const ALL: [Self; 2] = [Self::FastNoWait, Self::Mits1KStatic1975];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FastNoWait => "Fast / no wait states",
            Self::Mits1KStatic1975 => "MITS 1K Static RAM (1975, 2 read waits)",
        }
    }

    pub const fn read_wait_states(self) -> u8 {
        match self {
            Self::FastNoWait => 0,
            Self::Mits1KStatic1975 => 2,
        }
    }
}

impl Default for RamBoardProfile {
    fn default() -> Self {
        Self::FastNoWait
    }
}

/// MITS serial interface installed in the emulated Altair.''',
)
replace_once(
    "src/config/machine.rs",
    '''pub struct MachineConfig {
    pub cpu_model: CpuModel,
    pub ram_size: RamSize,
    pub ram_init: RamInit,
    pub serial_board: SerialBoard,
}''',
    '''pub struct MachineConfig {
    pub cpu_model: CpuModel,
    pub ram_size: RamSize,
    pub ram_init: RamInit,
    pub ram_board_profile: RamBoardProfile,
    pub serial_board: SerialBoard,
}''',
)
append_once(
    "src/config/machine.rs",
    "fn original_mits_1k_profile_has_two_read_wait_states",
    '''\n#[cfg(test)]\nmod memory_board_profile_tests {\n    use super::*;\n\n    #[test]\n    fn original_mits_1k_profile_has_two_read_wait_states() {\n        assert_eq!(RamBoardProfile::Mits1KStatic1975.read_wait_states(), 2);\n        assert_eq!(RamBoardProfile::FastNoWait.read_wait_states(), 0);\n        assert_eq!(AppConfig::default().machine.ram_board_profile, RamBoardProfile::FastNoWait);\n    }\n}\n''',
)
replace_once(
    "src/config/mod.rs",
    '''    PeripheralConfig, PreferencesConfig, RamInit, RamSize, SerialBoard, TerminalSpeed,
};''',
    '''    PeripheralConfig, PreferencesConfig, RamBoardProfile, RamInit, RamSize, SerialBoard,
    TerminalSpeed,
};''',
)

# ---------------------------------------------------------------------------
# S-100 READY is the wired result of front-panel PRDY and memory-card PRDY.
# ---------------------------------------------------------------------------
replace_once(
    "src/machine/panel_bus.rs",
    '''    pub run: bool,
    pub ready: bool,
    pub wait: bool,''',
    '''    pub run: bool,
    /// Effective PRDY level seen by the CPU after all S-100 contributors.
    pub ready: bool,
    /// Display/Control contribution to PRDY (RUN/SINGLE STEP/EXAMINE side).
    pub front_panel_ready: bool,
    /// Selected memory-card contribution to PRDY. Slow RAM may pull this low.
    pub memory_ready: bool,
    pub wait: bool,''',
)
replace_once(
    "src/machine/panel_bus.rs",
    '''            run: false,
            ready: false,
            wait: false,''',
    '''            run: false,
            ready: false,
            front_panel_ready: false,
            memory_ready: true,
            wait: false,''',
)
replace_once(
    "src/machine/panel_bus.rs",
    '''    pub(super) fn set_run(&mut self, run: bool) {
        self.signals.run = run;
    }

    /// Instruction-level/Fast compatibility helper.''',
    '''    pub(super) fn set_run(&mut self, run: bool) {
        self.signals.run = run;
    }

    fn recompute_ready(&mut self) {
        self.signals.ready = self.signals.front_panel_ready && self.signals.memory_ready;
    }

    /// Instruction-level/Fast compatibility helper.''',
)
replace_once(
    "src/machine/panel_bus.rs",
    '''    pub(super) fn set_ready(&mut self, ready: bool) {
        self.signals.ready = ready;
        self.signals.wait = !ready && !self.signals.reset;
    }

    /// Exact CPU-board path: READY is an input to the 8080 and must be mutable
    /// without also fabricating the CPU's WAIT output. Cycle Accurate publishes
    /// WAIT only through a real `Cpu8080Cycle` T-state sample.
    pub(super) fn set_ready_input(&mut self, ready: bool) {
        self.signals.ready = ready;
    }''',
    '''    pub(super) fn set_ready(&mut self, ready: bool) {
        self.signals.front_panel_ready = ready;
        self.signals.memory_ready = true;
        self.recompute_ready();
        self.signals.wait = !self.signals.ready && !self.signals.reset;
    }

    /// Exact CPU-board path: mutate only the Display/Control contribution to
    /// PRDY. Memory cards keep their own wired-AND contribution.
    pub(super) fn set_ready_input(&mut self, ready: bool) {
        self.signals.front_panel_ready = ready;
        self.recompute_ready();
    }

    /// Memory-board PRDY contribution. `true` means the selected card is ready;
    /// `false` means a slow card is actively stretching the read cycle.
    pub(super) fn set_memory_ready_input(&mut self, ready: bool) {
        self.signals.memory_ready = ready;
        self.recompute_ready();
    }''',
)
replace_once(
    "src/machine/panel_bus.rs",
    '''        self.signals.ready = false;
        self.signals.wait = false;''',
    '''        self.signals.front_panel_ready = false;
        self.signals.memory_ready = true;
        self.signals.ready = false;
        self.signals.wait = false;''',
)
replace_once(
    "src/machine/panel_bus.rs",
    '''        self.signals.ready = run;
        self.signals.wait = !run;''',
    '''        self.signals.front_panel_ready = run;
        self.signals.memory_ready = true;
        self.signals.ready = run;
        self.signals.wait = !run;''',
)
replace_once(
    "src/machine/panel_bus.rs",
    '''            self.signals.apply_status_word(STATUS_INSTRUCTION_FETCH);
            self.signals.ready = false;
            self.signals.wait = true;''',
    '''            self.signals.apply_status_word(STATUS_INSTRUCTION_FETCH);
            self.signals.front_panel_ready = false;
            self.signals.memory_ready = true;
            self.signals.ready = false;
            self.signals.wait = true;''',
)
append_once(
    "src/machine/panel_bus.rs",
    "memory_ready_is_wired_with_front_panel_ready",
    '''\n#[cfg(test)]\nmod ready_source_tests {\n    use super::*;\n\n    #[test]\n    fn memory_ready_is_wired_with_front_panel_ready() {\n        let mut bus = S100BusState::default();\n        bus.set_ready_input(true);\n        assert!(bus.signals().ready);\n        bus.set_memory_ready_input(false);\n        assert!(!bus.signals().ready);\n        bus.set_memory_ready_input(true);\n        assert!(bus.signals().ready);\n        bus.set_ready_input(false);\n        assert!(!bus.signals().ready);\n    }\n}\n''',
)

# ---------------------------------------------------------------------------
# Memory board owns its slowdown circuit and tracks read-wait pulse state.
# ---------------------------------------------------------------------------
replace_once(
    "src/machine/memory.rs",
    '''use crate::config::{RamInit, RamSize};''',
    '''use crate::config::{RamBoardProfile, RamInit, RamSize};''',
)
replace_once(
    "src/machine/memory.rs",
    '''pub const MEMORY_BOARD_COUNT: usize = MAX_MEM_SIZE / MEMORY_BOARD_SIZE;

/// Physical RAM backing store plus the front-panel write-protection latches.''',
    '''pub const MEMORY_BOARD_COUNT: usize = MAX_MEM_SIZE / MEMORY_BOARD_SIZE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryReadyPhase {
    T1,
    T2,
    Tw,
    T3,
    Other,
}

/// Physical RAM backing store plus the front-panel write-protection latches.''',
)
replace_once(
    "src/machine/memory.rs",
    '''    init_mode: RamInit,
    basic32_probe_guard: bool,
    basic32_probe_write: Option<u8>,
}''',
    '''    init_mode: RamInit,
    board_profiles: [RamBoardProfile; MEMORY_BOARD_COUNT],
    read_wait_active: bool,
    read_wait_remaining: u8,
    basic32_probe_guard: bool,
    basic32_probe_write: Option<u8>,
}''',
)
replace_once(
    "src/machine/memory.rs",
    '''            installed_size: MEM_SIZE,
            init_mode: RamInit::Random,
            basic32_probe_guard: false,''',
    '''            installed_size: MEM_SIZE,
            init_mode: RamInit::Random,
            board_profiles: [RamBoardProfile::FastNoWait; MEMORY_BOARD_COUNT],
            read_wait_active: false,
            read_wait_remaining: 0,
            basic32_probe_guard: false,''',
)
replace_once(
    "src/machine/memory.rs",
    '''    pub(super) fn configure(&mut self, size: RamSize, init_mode: RamInit) {
        self.installed_size = size.bytes();
        self.init_mode = init_mode;
        self.clear_protection();
        self.initialize();
    }

    pub(super) fn installed_size(&self) -> usize {''',
    '''    pub(super) fn configure(&mut self, size: RamSize, init_mode: RamInit) {
        self.installed_size = size.bytes();
        self.init_mode = init_mode;
        self.clear_protection();
        self.reset_timing();
        self.initialize();
    }

    pub(super) fn configure_board_profile(&mut self, profile: RamBoardProfile) {
        // Storage is per 1 KiB slot even though the current UI applies one
        // profile to all installed slots. This deliberately leaves room for
        // mixed-card Altair configurations without another memory rewrite.
        self.board_profiles.fill(profile);
        self.reset_timing();
    }

    pub(super) fn board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        if address as usize >= self.installed_size {
            return None;
        }
        Self::board_index(address).map(|index| self.board_profiles[index])
    }

    fn read_wait_states(&self, address: u16) -> u8 {
        self.board_profile(address)
            .map(RamBoardProfile::read_wait_states)
            .unwrap_or(0)
    }

    pub(super) fn reset_timing(&mut self) {
        self.read_wait_active = false;
        self.read_wait_remaining = 0;
    }

    /// Return the memory-card PRDY contribution for the current 8080 T-state.
    /// The MITS 1K board starts its slowdown pulse with PSYNC and produces two
    /// actual TW cycles on reads. Writes and uninstalled addresses never wait.
    pub(super) fn ready_for_t_state(
        &mut self,
        address: u16,
        memory_read: bool,
        phase: MemoryReadyPhase,
    ) -> bool {
        if !memory_read {
            self.reset_timing();
            return true;
        }

        match phase {
            MemoryReadyPhase::T1 => {
                self.read_wait_remaining = self.read_wait_states(address);
                self.read_wait_active = self.read_wait_remaining != 0;
                !self.read_wait_active
            }
            MemoryReadyPhase::T2 => !self.read_wait_active,
            MemoryReadyPhase::Tw if self.read_wait_active => {
                if self.read_wait_remaining > 1 {
                    self.read_wait_remaining -= 1;
                    false
                } else {
                    self.reset_timing();
                    true
                }
            }
            MemoryReadyPhase::Tw => true,
            MemoryReadyPhase::T3 | MemoryReadyPhase::Other => {
                self.reset_timing();
                true
            }
        }
    }

    pub(super) fn installed_size(&self) -> usize {''',
)
replace_once(
    "src/machine/memory.rs",
    '''    pub(crate) fn cycle_read_memory(&mut self, address: u16) -> u8 {
        self.memory.read(address)
    }''',
    '''    pub(crate) fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) {
        self.memory.configure_board_profile(profile);
        self.s100.set_memory_ready_input(true);
    }

    pub(crate) fn memory_board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        self.memory.board_profile(address)
    }

    pub(crate) fn cycle_memory_ready(
        &mut self,
        address: u16,
        memory_read: bool,
        phase: MemoryReadyPhase,
    ) -> bool {
        let ready = self.memory.ready_for_t_state(address, memory_read, phase);
        self.s100.set_memory_ready_input(ready);
        ready
    }

    /// Host freezes physical STOP at the first TW instead of burning millions of
    /// identical wait clocks. A real memory-board one-shot would expire during
    /// the operator pause, so settle that transient PRDY source before resume.
    pub(crate) fn cycle_settle_memory_ready_after_panel_freeze(&mut self) {
        self.memory.reset_timing();
        self.s100.set_memory_ready_input(true);
    }

    pub(crate) fn cycle_read_memory(&mut self, address: u16) -> u8 {
        self.memory.read(address)
    }''',
)
append_once(
    "src/machine/memory.rs",
    "mits_1k_read_timing_yields_two_wait_cycles",
    '''\n#[cfg(test)]\nmod timing_tests {\n    use super::*;\n\n    #[test]\n    fn mits_1k_read_timing_yields_two_wait_cycles() {\n        let mut memory = Memory::default();\n        memory.configure(RamSize::K1, RamInit::Zeroed);\n        memory.configure_board_profile(RamBoardProfile::Mits1KStatic1975);\n\n        assert!(!memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T1));\n        assert!(!memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T2));\n        assert!(!memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::Tw));\n        assert!(memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::Tw));\n        assert!(memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T3));\n    }\n\n    #[test]\n    fn mits_1k_write_and_fast_profile_do_not_stretch_ready() {\n        let mut memory = Memory::default();\n        memory.configure(RamSize::K1, RamInit::Zeroed);\n        memory.configure_board_profile(RamBoardProfile::Mits1KStatic1975);\n        assert!(memory.ready_for_t_state(0x0000, false, MemoryReadyPhase::T1));\n        memory.configure_board_profile(RamBoardProfile::FastNoWait);\n        assert!(memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T1));\n        assert!(memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T2));\n    }\n}\n''',
)

# ---------------------------------------------------------------------------
# Chassis API and lifecycle clear transient memory-card wait pulses.
# ---------------------------------------------------------------------------
replace_once(
    "src/machine/mod.rs",
    '''use crate::config::{RamInit, RamSize};''',
    '''use crate::config::{RamBoardProfile, RamInit, RamSize};''',
)
replace_once(
    "src/machine/mod.rs",
    '''pub(crate) use cpu_board::{Cycle8080S100Adapter, S100CpuControlLines, S100CpuSample};
pub use memory::{MAX_MEM_SIZE, MEM_SIZE, MEMORY_BOARD_COUNT, MEMORY_BOARD_SIZE};''',
    '''pub(crate) use cpu_board::{Cycle8080S100Adapter, S100CpuControlLines, S100CpuSample};
pub(crate) use memory::MemoryReadyPhase;
pub use memory::{MAX_MEM_SIZE, MEM_SIZE, MEMORY_BOARD_COUNT, MEMORY_BOARD_SIZE};''',
)
replace_once(
    "src/machine/mod.rs",
    '''    pub fn installed_ram_bytes(&self) -> usize { self.bus.installed_ram_bytes() }
    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool { self.bus.arm_basic32_full_memory_probe_guard() }''',
    '''    pub fn installed_ram_bytes(&self) -> usize { self.bus.installed_ram_bytes() }
    pub fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) {
        self.bus.configure_memory_board_profile(profile);
    }
    pub fn memory_board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        self.bus.memory_board_profile(address)
    }
    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool { self.bus.arm_basic32_full_memory_probe_guard() }''',
)
replace_once(
    "src/machine/mod.rs",
    '''    fn assert_front_panel_reset_bus(&mut self) {
        self.s100.assert_front_panel_reset();
    }''',
    '''    fn assert_front_panel_reset_bus(&mut self) {
        self.memory.reset_timing();
        self.s100.set_memory_ready_input(true);
        self.s100.assert_front_panel_reset();
    }''',
)
replace_once(
    "src/machine/mod.rs",
    '''    fn power_off_s100(&mut self) {
        self.s100.power_off();
        self.cpu_inte = false;
    }''',
    '''    fn power_off_s100(&mut self) {
        self.memory.reset_timing();
        self.s100.power_off();
        self.cpu_inte = false;
    }''',
)

# Expose the cycle address before T1 so a memory card can decode the same address
# that the CPU will put on the bus when it starts PSYNC.
replace_once(
    "src/cpu8080_cycle/mod.rs",
    '''    pub const fn machine_cycle_index(&self) -> u8 {
        self.machine_cycle_index
    }

    pub const fn t_state(&self) -> TState {''',
    '''    pub const fn machine_cycle_index(&self) -> u8 {
        self.machine_cycle_index
    }

    pub const fn cycle_address(&self) -> u16 {
        self.cycle_address
    }

    pub const fn t_state(&self) -> TState {''',
)

# ---------------------------------------------------------------------------
# Backend API: selecting card type is a machine configuration operation.
# ---------------------------------------------------------------------------
replace_once(
    "src/backend/mod.rs",
    '''use crate::config::{RamInit, RamSize, SerialBoard};''',
    '''use crate::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};''',
)
replace_once(
    "src/backend/mod.rs",
    '''    fn configure_memory(&mut self, _size: RamSize, _init: RamInit) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "configure memory", engine: self.engine() })
    }
    fn power(&mut self, on: bool) -> BackendResult<()>;''',
    '''    fn configure_memory(&mut self, _size: RamSize, _init: RamInit) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "configure memory", engine: self.engine() })
    }
    fn configure_memory_board_profile(&mut self, _profile: RamBoardProfile) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "configure memory board profile", engine: self.engine() })
    }
    fn power(&mut self, on: bool) -> BackendResult<()>;''',
)
replace_once(
    "src/backend/mod.rs",
    '''    pub fn configure_memory(&mut self, size: RamSize, init: RamInit) { Self::call(self.backend.configure_memory(size, init)); }
    pub fn configure_serial_board(&mut self, board: SerialBoard) { Self::call(self.backend.configure_serial_board(board)); }''',
    '''    pub fn configure_memory(&mut self, size: RamSize, init: RamInit) { Self::call(self.backend.configure_memory(size, init)); }
    pub fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) { Self::call(self.backend.configure_memory_board_profile(profile)); }
    pub fn configure_serial_board(&mut self, board: SerialBoard) { Self::call(self.backend.configure_serial_board(board)); }''',
)

replace_once(
    "src/backend/native.rs",
    '''use crate::config::{RamInit, RamSize, SerialBoard};''',
    '''use crate::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};''',
)
replace_once(
    "src/backend/native.rs",
    '''    fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {
        self.machine.configure_memory(size, init);
        self.reset_debugger_epoch();
        Ok(())
    }
    fn power(&mut self, on: bool) -> BackendResult<()> {''',
    '''    fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {
        self.machine.configure_memory(size, init);
        self.reset_debugger_epoch();
        Ok(())
    }
    fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) -> BackendResult<()> {
        self.machine.configure_memory_board_profile(profile);
        self.reset_debugger_epoch();
        Ok(())
    }
    fn power(&mut self, on: bool) -> BackendResult<()> {''',
)

replace_once(
    "src/backend/cycle_host.rs",
    '''use crate::config::{RamInit, RamSize, SerialBoard};''',
    '''use crate::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};''',
)
replace_once(
    "src/backend/cycle_host.rs",
    '''        let powered = self.inner.machine().powered;
        let serial_board = self.inner.machine().serial_board();

        if powered {''',
    '''        let powered = self.inner.machine().powered;
        let serial_board = self.inner.machine().serial_board();
        let memory_profile = self
            .inner
            .machine()
            .memory_board_profile(0)
            .unwrap_or_default();

        if powered {''',
)
replace_once(
    "src/backend/cycle_host.rs",
    '''            replacement.machine_mut().configure_memory(size, init);
            replacement.machine_mut().configure_serial_board(serial_board);''',
    '''            replacement.machine_mut().configure_memory(size, init);
            replacement.machine_mut().configure_memory_board_profile(memory_profile);
            replacement.machine_mut().configure_serial_board(serial_board);''',
)
replace_once(
    "src/backend/cycle_host.rs",
    '''        self.reset_debugger_epoch();
        Ok(())
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {''',
    '''        self.reset_debugger_epoch();
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

    fn power(&mut self, on: bool) -> BackendResult<()> {''',
)

# ---------------------------------------------------------------------------
# Cycle backend consumes effective PRDY from the selected RAM board.
# ---------------------------------------------------------------------------
replace_once(
    "src/backend/cycle.rs",
    '''use crate::machine::{AltairMachine, Cycle8080S100Adapter};''',
    '''use crate::machine::{AltairMachine, Cycle8080S100Adapter, MemoryReadyPhase};''',
)
replace_once(
    "src/backend/cycle.rs",
    '''    fn tick_once_with_front_panel_data(
        &mut self,
        ready: bool,
        front_panel_data: Option<u8>,
        record_instruction: bool,
    ) -> TickTrace {''',
    '''    fn memory_ready_for_current_t_state(&mut self) -> bool {
        let memory_read = matches!(
            self.cpu.machine_cycle(),
            MachineCycle::InstructionFetch | MachineCycle::MemoryRead | MachineCycle::StackRead
        );
        let phase = match self.cpu.t_state() {
            TState::T1 => MemoryReadyPhase::T1,
            TState::T2 => MemoryReadyPhase::T2,
            TState::Tw => MemoryReadyPhase::Tw,
            TState::T3 => MemoryReadyPhase::T3,
            _ => MemoryReadyPhase::Other,
        };
        self.machine
            .bus
            .cycle_memory_ready(self.cpu.cycle_address(), memory_read, phase)
    }

    fn tick_once_with_front_panel_data(
        &mut self,
        ready: bool,
        front_panel_data: Option<u8>,
        record_instruction: bool,
    ) -> TickTrace {''',
)
replace_once(
    "src/backend/cycle.rs",
    '''        self.machine.bus.refresh_interrupt_request_line();
        let data_in = self.data_in_for_current_t_state(front_panel_data);
        let lines = self.machine.bus.cpu_control_lines();
        let trace = self.cpu.tick(Cpu8080Inputs {
            data_in,
            // SINGLE STEP and the EXM sequencer may momentarily override the
            // stopped READY line. HOLD, PINT and RESET always arrive through S-100.
            ready,
            interrupt: lines.interrupt,''',
    '''        self.machine.bus.refresh_interrupt_request_line();
        let memory_ready = self.memory_ready_for_current_t_state();
        let effective_ready = ready && memory_ready;
        let data_in = self.data_in_for_current_t_state(front_panel_data);
        let lines = self.machine.bus.cpu_control_lines();
        let trace = self.cpu.tick(Cpu8080Inputs {
            data_in,
            // SINGLE STEP/EXM may override the front-panel contribution, but a
            // selected slow RAM card can still pull the effective PRDY low.
            ready: effective_ready,
            interrupt: lines.interrupt,''',
)
replace_once(
    "src/backend/cycle.rs",
    '''        let visible_data = self.drive_s100_t_state(&trace, data_in, front_panel_data, ready);
        self.sync_machine_cpu();
        self.capture_teaching_snapshot(&trace, visible_data, ready);''',
    '''        let visible_data = self.drive_s100_t_state(
            &trace,
            data_in,
            front_panel_data,
            effective_ready,
        );
        self.sync_machine_cpu();
        self.capture_teaching_snapshot(&trace, visible_data, effective_ready);''',
)
replace_once(
    "src/backend/cycle.rs",
    '''        self.refresh_teaching_visible_lamps();
    }

    pub(super) fn debugger_step_t_state_exact''',
    '''        self.machine.bus.cycle_settle_memory_ready_after_panel_freeze();
        self.refresh_teaching_visible_lamps();
    }

    pub(super) fn debugger_step_t_state_exact''',
)
replace_once(
    "src/backend/cycle.rs",
    '''        if !saw_psync {
            // Restore the stopped READY input even if a fault/HALT prevented a
            // following PSYNC. This path deliberately does not synthesize WAIT.
            self.machine.cycle_set_running(false);
        }
        self.refresh_teaching_visible_lamps();''',
    '''        if !saw_psync {
            // Restore the stopped READY input even if a fault/HALT prevented a
            // following PSYNC. This path deliberately does not synthesize WAIT.
            self.machine.cycle_set_running(false);
        }
        self.machine.bus.cycle_settle_memory_ready_after_panel_freeze();
        self.refresh_teaching_visible_lamps();''',
)
replace_once(
    "src/backend/cycle.rs",
    '''        }
        self.refresh_teaching_visible_lamps();
    }

    fn front_panel_controls_available''',
    '''        }
        self.machine.bus.cycle_settle_memory_ready_after_panel_freeze();
        self.refresh_teaching_visible_lamps();
    }

    fn front_panel_controls_available''',
)

# ---------------------------------------------------------------------------
# Application config applies the selected card profile when engines are swapped.
# UI/persistence selection is wired in the next layer after electrical CI is green.
# ---------------------------------------------------------------------------
replace_once(
    "src/app/mod.rs",
    '''    AppConfig, Asr33Speed, EmulationSpeed, RamInit, RamSize, SerialBoard, TerminalSpeed,
};''',
    '''    AppConfig, Asr33Speed, EmulationSpeed, RamBoardProfile, RamInit, RamSize, SerialBoard,
    TerminalSpeed,
};''',
)
replace_once(
    "src/app/mod.rs",
    '''                self.machine.configure_memory(
                    self.config.machine.ram_size,
                    self.config.machine.ram_init,
                );
                self.machine.configure_serial_board(self.config.machine.serial_board);''',
    '''                self.machine.configure_memory(
                    self.config.machine.ram_size,
                    self.config.machine.ram_init,
                );
                self.machine
                    .configure_memory_board_profile(self.config.machine.ram_board_profile);
                self.machine.configure_serial_board(self.config.machine.serial_board);''',
)
replace_once(
    "src/app/mod.rs",
    '''    fn apply_serial_board_configuration(&mut self, serial_board: SerialBoard) {''',
    '''    fn apply_memory_board_profile(&mut self, profile: RamBoardProfile) {
        if self.config.machine.ram_board_profile == profile { return; }
        self.config.machine.ram_board_profile = profile;
        self.machine.configure_memory_board_profile(profile);
        self.last_tick = Instant::now();
        self.status = format!("Memory card timing: {}", profile.label());
    }

    fn apply_serial_board_configuration(&mut self, serial_board: SerialBoard) {''',
)

# ---------------------------------------------------------------------------
# End-to-end regressions: actual Cycle core emits two TW states and +2T per read.
# ---------------------------------------------------------------------------
test_path = Path("tests/memory_wait_timing.rs")
if not test_path.exists():
    test_path.write_text(r'''use rustair::backend::{BackendHost, BusTState, EmulationEngine};
use rustair::config::{RamBoardProfile, RamInit, RamSize};

fn prepared(profile: RamBoardProfile, program: &[u8]) -> BackendHost {
    let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080)
        .expect("built-in Cycle backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.configure_memory_board_profile(profile);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host
}

#[test]
fn mits_1k_opcode_fetch_emits_exactly_two_tw_states() {
    let mut host = prepared(RamBoardProfile::Mits1KStatic1975, &[0x00, 0x00]);
    let mut states = Vec::new();
    let mut ready = Vec::new();
    let mut wait = Vec::new();

    for _ in 0..6 {
        host.debugger_step_t_state();
        let sample = host.bus_teaching_snapshot().expect("exact sample");
        states.push(sample.t_state);
        ready.push(sample.ready);
        wait.push(sample.pins.wait);
    }

    assert_eq!(states, vec![
        BusTState::T1,
        BusTState::T2,
        BusTState::Tw,
        BusTState::Tw,
        BusTState::T3,
        BusTState::T4,
    ]);
    assert_eq!(ready, vec![
        Some(false),
        Some(false),
        Some(false),
        Some(true),
        Some(true),
        Some(true),
    ]);
    assert_eq!(wait[2], Some(true));
    assert_eq!(wait[3], Some(true));
    assert_eq!(host.intel8080_state().total_t_states, Some(6));
}

#[test]
fn fast_memory_profile_keeps_standard_nop_at_four_t_states() {
    let mut host = prepared(RamBoardProfile::FastNoWait, &[0x00, 0x00]);
    host.debugger_step_instruction();
    assert_eq!(host.intel8080_state().total_t_states, Some(4));
}

#[test]
fn mits_1k_mvi_has_two_slow_reads_not_a_global_instruction_penalty() {
    let mut host = prepared(RamBoardProfile::Mits1KStatic1975, &[0x3e, 0x42, 0x00]);
    host.debugger_step_instruction();
    let cpu = host.intel8080_state();
    assert_eq!(cpu.a, 0x42);
    // MVI A,imm is 7T normally: M1 fetch + operand memory read. Each addressed
    // MITS 1K read contributes exactly two TW states, therefore 7 + 2 + 2 = 11.
    assert_eq!(cpu.total_t_states, Some(11));
}
''', encoding="utf-8")

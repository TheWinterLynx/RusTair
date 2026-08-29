from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"expected completion anchor not found in {path}: {old[:160]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


def replace_all(path: str, old: str, new: str, expected: int) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if old not in text:
        if new in text:
            return
        raise SystemExit(f"completion anchor not found in {path}: {old!r}")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"expected {expected} occurrences in {path}, found {count}: {old!r}")
    p.write_text(text.replace(old, new), encoding="utf-8")


def append_once(path: str, marker: str, addition: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if marker in text:
        return
    p.write_text(text + addition, encoding="utf-8")


# ---------------------------------------------------------------------------
# Cycle: keep the front-panel PRDY contributor separate from effective READY.
# Otherwise a slow RAM card that held READY low on the previous TW could never
# release the running CPU because the old effective level would feed itself.
# ---------------------------------------------------------------------------
replace_once(
    "src/machine/cpu_board.rs",
    '''    pub(crate) fn cycle_set_ready_input(&mut self, ready: bool) {
        self.s100.set_ready_input(ready);
    }

    /// Change only the external HOLD request seen by the cycle-accurate CPU.''',
    '''    pub(crate) fn cycle_set_ready_input(&mut self, ready: bool) {
        self.s100.set_ready_input(ready);
    }

    /// Display/Control-board PRDY contribution before RAM/device wait sources
    /// are wired into the effective S-100 READY level.
    pub(crate) fn cycle_front_panel_ready_input(&self) -> bool {
        self.s100.signals().front_panel_ready
    }

    /// Change only the external HOLD request seen by the cycle-accurate CPU.''',
)
replace_all(
    "src/backend/cycle.rs",
    "            let ready = self.machine.bus.cpu_control_lines().ready;",
    "            let ready = self.machine.bus.cycle_front_panel_ready_input();",
    2,
)

# ---------------------------------------------------------------------------
# Fast backend: it cannot expose individual TW samples, but guest elapsed time
# must still include wait T-states contributed by the selected physical RAM card.
# ---------------------------------------------------------------------------
replace_once(
    "src/cpu8080.rs",
    '''    fn interrupt_ack(&mut self, _address: u16, _opcode: u8, _while_halted: bool) {}
    fn instruction_complete(&mut self, _address: u16, _opcode: u8, _t_states: u32) {}
}''',
    '''    fn interrupt_ack(&mut self, _address: u16, _opcode: u8, _while_halted: bool) {}
    /// Extra external wait T-states accumulated by instruction-level bus
    /// devices during the current instruction. Exact Cycle mode does not use
    /// this approximation because it clocks every TW explicitly.
    fn take_wait_states(&mut self) -> u32 { 0 }
    fn instruction_complete(&mut self, _address: u16, _opcode: u8, _t_states: u32) {}
}''',
)
replace_once(
    "src/cpu8080.rs",
    '''        let opcode = bus.opcode_fetch(opcode_address);
        self.pc = self.pc.wrapping_add(1);
        let t = self.execute(bus, opcode);
        // EI enables interrupts only after the following instruction.''',
    '''        let opcode = bus.opcode_fetch(opcode_address);
        self.pc = self.pc.wrapping_add(1);
        let base_t = self.execute(bus, opcode);
        // EI enables interrupts only after the following instruction.''',
)
replace_once(
    "src/cpu8080.rs",
    '''        self.f = (self.f & 0xd5) | FLAG_1;
        self.cycles += t as u64;
        bus.instruction_complete(opcode_address, opcode, t);
        t
    }''',
    '''        self.f = (self.f & 0xd5) | FLAG_1;
        let t = base_t.saturating_add(bus.take_wait_states());
        self.cycles += t as u64;
        bus.instruction_complete(opcode_address, opcode, t);
        t
    }''',
)
replace_once(
    "src/cpu8080.rs",
    '''        self.halted = false;
        let t = self.execute(bus, opcode);
        self.cycles += t as u64;
        true''',
    '''        self.halted = false;
        let t = self.execute(bus, opcode).saturating_add(bus.take_wait_states());
        self.cycles += t as u64;
        true''',
)

replace_once(
    "src/machine/mod.rs",
    '''    s100: S100BusState,
    cpu_inte: bool,
    diagnostic_meter: Option<CpuDiagnosticMeter>,''',
    '''    s100: S100BusState,
    cpu_inte: bool,
    fast_wait_t_states: u32,
    diagnostic_meter: Option<CpuDiagnosticMeter>,''',
)
replace_once(
    "src/machine/mod.rs",
    '''            s100: S100BusState::default(),
            cpu_inte: false,
            diagnostic_meter: None,''',
    '''            s100: S100BusState::default(),
            cpu_inte: false,
            fast_wait_t_states: 0,
            diagnostic_meter: None,''',
)
replace_once(
    "src/machine/mod.rs",
    '''        self.memory.configure(size, init_mode);
        self.refresh_protect_line();''',
    '''        self.memory.configure(size, init_mode);
        self.fast_wait_t_states = 0;
        self.refresh_protect_line();''',
)
replace_once(
    "src/machine/memory.rs",
    '''    fn read_wait_states(&self, address: u16) -> u8 {''',
    '''    pub(super) fn read_wait_states(&self, address: u16) -> u8 {''',
)
replace_once(
    "src/machine/memory.rs",
    '''    pub(crate) fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) {
        self.memory.configure_board_profile(profile);
        self.s100.set_memory_ready_input(true);
    }''',
    '''    pub(crate) fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) {
        self.memory.configure_board_profile(profile);
        self.fast_wait_t_states = 0;
        self.s100.set_memory_ready_input(true);
    }

    pub(crate) fn fast_account_memory_read_wait(&mut self, address: u16) {
        self.fast_wait_t_states = self
            .fast_wait_t_states
            .saturating_add(u32::from(self.memory.read_wait_states(address)));
    }

    pub(crate) fn take_fast_memory_wait_t_states(&mut self) -> u32 {
        std::mem::take(&mut self.fast_wait_t_states)
    }''',
)
replace_once(
    "src/machine/mod.rs",
    '''    fn read(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::MemoryRead);''',
    '''    fn read(&mut self, address: u16) -> u8 {
        self.fast_account_memory_read_wait(address);
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::MemoryRead);''',
)
replace_once(
    "src/machine/mod.rs",
    '''    fn opcode_fetch(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::InstructionFetch);''',
    '''    fn opcode_fetch(&mut self, address: u16) -> u8 {
        self.fast_account_memory_read_wait(address);
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::InstructionFetch);''',
)
replace_once(
    "src/machine/mod.rs",
    '''    fn stack_read(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::StackRead);''',
    '''    fn stack_read(&mut self, address: u16) -> u8 {
        self.fast_account_memory_read_wait(address);
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::StackRead);''',
)
replace_once(
    "src/machine/mod.rs",
    '''    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        let cycle = if while_halted {''',
    '''    fn take_wait_states(&mut self) -> u32 {
        self.take_fast_memory_wait_t_states()
    }

    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        let cycle = if while_halted {''',
)

# ---------------------------------------------------------------------------
# Configuration UI: card timing is physical hardware, so swap it only POWER OFF.
# ---------------------------------------------------------------------------
replace_once(
    "src/app/mod.rs",
    '''    fn apply_memory_board_profile(&mut self, profile: RamBoardProfile) {
        if self.config.machine.ram_board_profile == profile { return; }
        self.config.machine.ram_board_profile = profile;
        self.machine.configure_memory_board_profile(profile);
        self.last_tick = Instant::now();
        self.status = format!("Memory card timing: {}", profile.label());
    }''',
    '''    fn apply_memory_board_profile(&mut self, profile: RamBoardProfile) {
        if self.config.machine.ram_board_profile == profile { return; }
        if self.machine.powered() {
            self.status = "Power OFF the Altair before changing the installed RAM card timing".into();
            return;
        }
        self.config.machine.ram_board_profile = profile;
        self.machine.configure_memory_board_profile(profile);
        self.last_tick = Instant::now();
        self.status = format!("Memory card timing: {}", profile.label());
    }''',
)
replace_once(
    "src/app/runtime.rs",
    '''                        ui.separator();
                        ui.menu_button("Power-on contents", |ui| {''',
    '''                        ui.separator();
                        ui.menu_button("RAM board timing", |ui| {
                            let current = self.config.machine.ram_board_profile;
                            ui.label(format!("Installed timing profile: {}", current.label()));
                            ui.separator();
                            for profile in RamBoardProfile::ALL {
                                if ui.selectable_label(current == profile, profile.label()).clicked() {
                                    self.apply_memory_board_profile(profile);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            ui.small("The original MITS 1K static board uses its Processor Slow Down circuit to pull PRDY low for two wait states on each addressed memory read.");
                            ui.small("Cycle Accurate clocks both TW states explicitly; Fast 8080 adds the same wait T-states to guest elapsed time but cannot expose sub-instruction TW pin samples.");
                            if self.machine.powered() {
                                ui.small("Power OFF is required to swap the installed RAM-card timing profile.");
                            }
                        });
                        ui.separator();
                        ui.menu_button("Power-on contents", |ui| {''',
)

# ---------------------------------------------------------------------------
# Persistence and startup application of the physical RAM-card selection.
# ---------------------------------------------------------------------------
replace_once(
    "src/app/persistence.rs",
    '''                "machine.ram_init" => if let Some(v) = parse_ram_init(value) { saved.config.machine.ram_init = v; },
                "machine.serial_board" =>''',
    '''                "machine.ram_init" => if let Some(v) = parse_ram_init(value) { saved.config.machine.ram_init = v; },
                "machine.ram_board_profile" => if let Some(v) = parse_ram_board_profile(value) { saved.config.machine.ram_board_profile = v; },
                "machine.serial_board" =>''',
)
replace_once(
    "src/app/persistence.rs",
    '''        let _ = writeln!(out, "machine.ram_init={}", ram_init_key(self.config.machine.ram_init));
        let _ = writeln!(out, "machine.serial_board={}", serial_board_key(self.config.machine.serial_board));''',
    '''        let _ = writeln!(out, "machine.ram_init={}", ram_init_key(self.config.machine.ram_init));
        let _ = writeln!(out, "machine.ram_board_profile={}", ram_board_profile_key(self.config.machine.ram_board_profile));
        let _ = writeln!(out, "machine.serial_board={}", serial_board_key(self.config.machine.serial_board));''',
)
replace_once(
    "src/app/persistence.rs",
    '''        self.machine.configure_memory(self.config.machine.ram_size, self.config.machine.ram_init);
        self.machine.configure_serial_board(self.config.machine.serial_board);''',
    '''        self.machine.configure_memory(self.config.machine.ram_size, self.config.machine.ram_init);
        self.machine.configure_memory_board_profile(self.config.machine.ram_board_profile);
        self.machine.configure_serial_board(self.config.machine.serial_board);''',
)
replace_once(
    "src/app/persistence.rs",
    '''fn ram_init_key(v: RamInit) -> &'static str { match v { RamInit::Random => "random", RamInit::Zeroed => "zeroed" } }
fn parse_ram_init(v: &str) -> Option<RamInit> { Some(match v { "random" => RamInit::Random, "zeroed" => RamInit::Zeroed, _ => return None }) }
fn serial_board_key''',
    '''fn ram_init_key(v: RamInit) -> &'static str { match v { RamInit::Random => "random", RamInit::Zeroed => "zeroed" } }
fn parse_ram_init(v: &str) -> Option<RamInit> { Some(match v { "random" => RamInit::Random, "zeroed" => RamInit::Zeroed, _ => return None }) }
fn ram_board_profile_key(v: RamBoardProfile) -> &'static str { match v { RamBoardProfile::FastNoWait => "fast-no-wait", RamBoardProfile::Mits1KStatic1975 => "mits-1k-static-1975" } }
fn parse_ram_board_profile(v: &str) -> Option<RamBoardProfile> { Some(match v { "fast-no-wait" => RamBoardProfile::FastNoWait, "mits-1k-static-1975" => RamBoardProfile::Mits1KStatic1975, _ => return None }) }
fn serial_board_key''',
)
replace_once(
    "src/app/persistence.rs",
    '''        saved.config.machine.ram_size = RamSize::K48;
        saved.config.machine.serial_board = SerialBoard::TwoSio88;''',
    '''        saved.config.machine.ram_size = RamSize::K48;
        saved.config.machine.ram_board_profile = RamBoardProfile::Mits1KStatic1975;
        saved.config.machine.serial_board = SerialBoard::TwoSio88;''',
)
replace_once(
    "src/app/persistence.rs",
    '''        assert!(text.contains("machine.ram_size=48k"));
        assert!(text.contains("asr33.reader_speed=1x"));''',
    '''        assert!(text.contains("machine.ram_size=48k"));
        assert!(text.contains("machine.ram_board_profile=fast-no-wait"));
        assert!(text.contains("asr33.reader_speed=1x"));''',
)

# ---------------------------------------------------------------------------
# Regressions for continuous Cycle execution and Fast elapsed-time accounting.
# ---------------------------------------------------------------------------
append_once(
    "tests/memory_wait_timing.rs",
    "running_cycle_backend_recovers_when_memory_ready_returns_high",
    r'''

#[test]
fn running_cycle_backend_recovers_when_memory_ready_returns_high() {
    let mut host = prepared(RamBoardProfile::Mits1KStatic1975, &[0x00, 0x00]);
    host.set_running(true);
    host.run_cycles(6);
    let cpu = host.intel8080_state();
    assert_eq!(cpu.pc, 0x0001, "continuous RUN must leave TW when the card releases PRDY");
    assert_eq!(cpu.total_t_states, Some(6));
}

#[test]
fn fast_backend_accounts_for_mits_1k_wait_t_states_at_instruction_level() {
    let mut host = BackendHost::from_engine(EmulationEngine::RustFast8080)
        .expect("built-in Fast backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.configure_memory_board_profile(RamBoardProfile::Mits1KStatic1975);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, &[0x3e, 0x42, 0x00]);
    host.step();
    let cpu = host.intel8080_state();
    assert_eq!(cpu.a, 0x42);
    assert_eq!(cpu.total_t_states, Some(11));
}
''',
)

from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"expected INTE cleanup anchor not found in {path}: {old[:180]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


# S100Signals.inte is already the canonical raw chassis output. Remove the
# second AltairBus boolean that merely copied the same value.
replace_once(
    "src/machine/mod.rs",
    '''    s100: S100BusState,
    cpu_inte: bool,
    fast_wait_t_states: u32,''',
    '''    s100: S100BusState,
    fast_wait_t_states: u32,''',
)
replace_once(
    "src/machine/mod.rs",
    '''            s100: S100BusState::default(),
            cpu_inte: false,
            fast_wait_t_states: 0,''',
    '''            s100: S100BusState::default(),
            fast_wait_t_states: 0,''',
)
replace_once(
    "src/machine/mod.rs",
    '''    fn sync_cpu_inte(&mut self, enabled: bool) {
        self.cpu_inte = enabled;
        self.s100.set_inte(enabled);
    }''',
    '''    fn sync_cpu_inte(&mut self, enabled: bool) {
        self.s100.set_inte(enabled);
    }''',
)
replace_once(
    "src/machine/mod.rs",
    '''        let signals = self.s100.signals();
        let inte = self.cpu_inte;
        Fast8080S100Adapter::for_each_sample(''',
    '''        let signals = self.s100.signals();
        let inte = signals.inte;
        Fast8080S100Adapter::for_each_sample(''',
)
replace_once(
    "src/machine/mod.rs",
    '''        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        self.s100
            .drive_power_on_state(address, data, protected, self.cpu_inte, run);''',
    '''        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100
            .drive_power_on_state(address, data, protected, inte, run);''',
)
replace_once(
    "src/machine/mod.rs",
    '''        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        self.s100
            .release_front_panel_reset(address, data, protected, self.cpu_inte, run);''',
    '''        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100
            .release_front_panel_reset(address, data, protected, inte, run);''',
)
replace_once(
    "src/machine/mod.rs",
    '''        let protected = self.memory.is_protected(address);
        self.s100.drive_front_panel_deposit(address, value, protected, self.cpu_inte);''',
    '''        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100.drive_front_panel_deposit(address, value, protected, inte);''',
)
replace_once(
    "src/machine/mod.rs",
    '''    fn power_off_s100(&mut self) {
        self.memory.reset_timing();
        self.s100.power_off();
        self.cpu_inte = false;
    }''',
    '''    fn power_off_s100(&mut self) {
        self.memory.reset_timing();
        self.s100.power_off();
    }''',
)
replace_once(
    "src/machine/cpu_board.rs",
    '''        let signals = self.s100.signals();
        let inte = self.cpu_inte;
        Fast8080S100Adapter::for_each_front_panel_jam_sample(''',
    '''        let signals = self.s100.signals();
        let inte = signals.inte;
        Fast8080S100Adapter::for_each_front_panel_jam_sample(''',
)
replace_once(
    "src/machine/memory.rs",
    '''        self.cpu_inte = inte;
        self.s100.drive_cpu_t_state(''',
    '''        self.s100.drive_cpu_t_state(''',
)

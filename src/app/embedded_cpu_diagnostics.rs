use super::*;
use crate::cpu8080::{Bus, Cpu8080};
use crate::machine::CpuDiagnosticResult;

const CPM_COM_LOAD_ADDRESS: u16 = 0x0100;
const CPM_PAGE_ZERO_SIZE: usize = 0x0100;
const BOOT_ADDRESS: usize = 0x0080;
const CPM_BDOS_PAGE_BYTES: usize = 0x0100;
const CPM_STACK_GUARD_BYTES: usize = 0x0100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiagnosticRunSpeed {
    Unlimited,
    Authentic2MHz,
}

impl DiagnosticRunSpeed {
    const ALL: [Self; 2] = [Self::Unlimited, Self::Authentic2MHz];

    const fn label(self) -> &'static str {
        match self {
            Self::Unlimited => "Unlimited",
            Self::Authentic2MHz => "Authentic 2 MHz",
        }
    }

    const fn emulation_speed(self) -> EmulationSpeed {
        match self {
            Self::Unlimited => EmulationSpeed::Unlimited,
            Self::Authentic2MHz => EmulationSpeed::Authentic,
        }
    }
}

impl Default for DiagnosticRunSpeed {
    fn default() -> Self { Self::Unlimited }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassicDiagnostic {
    Preliminary,
    Tst8080,
    CpuTest,
    ExerciserModified,
}

impl ClassicDiagnostic {
    const SUITE: [Self; 4] = [
        Self::Preliminary,
        Self::Tst8080,
        Self::CpuTest,
        Self::ExerciserModified,
    ];

    const fn filename(self) -> &'static str {
        match self {
            Self::Preliminary => "8080PRE.COM",
            Self::Tst8080 => "TST8080.COM",
            Self::CpuTest => "CPUTEST.COM",
            Self::ExerciserModified => "8080EXM.COM",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Preliminary => "8080PRE — preliminary tests",
            Self::Tst8080 => "TST8080 — Microcosm diagnostic",
            Self::CpuTest => "CPUTEST — Supersoft diagnostic",
            Self::ExerciserModified => "8080EXM — full instruction exerciser",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Preliminary => include_bytes!("../../assets/cpu-tests/8080PRE.COM"),
            Self::Tst8080 => include_bytes!("../../assets/cpu-tests/TST8080.COM"),
            Self::CpuTest => include_bytes!("../../assets/cpu-tests/CPUTEST.COM"),
            Self::ExerciserModified => include_bytes!("../../assets/cpu-tests/8080EXM.COM"),
        }
    }

    const fn expected_instructions(self) -> u64 {
        match self {
            Self::Preliminary => 1_061,
            Self::Tst8080 => 651,
            Self::CpuTest => 33_971_311,
            Self::ExerciserModified => 2_919_050_698,
        }
    }

    const fn expected_t_states(self) -> u64 {
        match self {
            Self::Preliminary => 7_817,
            Self::Tst8080 => 4_924,
            Self::CpuTest => 255_653_383,
            Self::ExerciserModified => 23_803_381_171,
        }
    }
}

#[derive(Clone, Debug)]
struct ControlCheck {
    name: &'static str,
    passed: bool,
    detail: String,
}

#[derive(Clone, Debug)]
struct ControlLineReport {
    checks: Vec<ControlCheck>,
}

impl ControlLineReport {
    fn passed(&self) -> bool { self.checks.iter().all(|check| check.passed) }
}

#[derive(Clone, Debug)]
struct SuiteRun {
    next_index: usize,
    control: ControlLineReport,
    results: Vec<CpuDiagnosticResult>,
}

#[derive(Clone, Debug)]
struct SuiteReport {
    control: ControlLineReport,
    results: Vec<CpuDiagnosticResult>,
}

impl SuiteReport {
    fn passed(&self) -> bool {
        self.control.passed()
            && self.results.len() == ClassicDiagnostic::SUITE.len()
            && self.results.iter().all(reference_match)
    }
}

pub(in crate::app) struct EmbeddedDiagnosticsState {
    speed: DiagnosticRunSpeed,
    port: cpu_diagnostics::DiagnosticSerialPort,
    active_test: Option<ClassicDiagnostic>,
    suite: Option<SuiteRun>,
    individual_result: Option<CpuDiagnosticResult>,
    control_report: Option<ControlLineReport>,
    suite_report: Option<SuiteReport>,
}

impl Default for EmbeddedDiagnosticsState {
    fn default() -> Self {
        Self {
            speed: DiagnosticRunSpeed::default(),
            port: cpu_diagnostics::DiagnosticSerialPort::Port0,
            active_test: None,
            suite: None,
            individual_result: None,
            control_report: None,
            suite_report: None,
        }
    }
}

struct CpmDiagnosticEnvironment {
    page_zero: [u8; CPM_PAGE_ZERO_SIZE],
    bdos_base: u16,
    bdos: Vec<u8>,
}

fn append_abs(code: &mut Vec<u8>, opcode: u8, address: u16) {
    let [lo, hi] = address.to_le_bytes();
    code.extend_from_slice(&[opcode, lo, hi]);
}

fn build_cpm_environment(
    board: SerialBoard,
    port: cpu_diagnostics::DiagnosticSerialPort,
    bdos_base: u16,
) -> Option<CpmDiagnosticEnvironment> {
    let (status_port, data_port, ready_mask, wait_branch) = match (board, port) {
        (SerialBoard::Sio88, cpu_diagnostics::DiagnosticSerialPort::Port0) => {
            (0x00, 0x01, 0xc0, 0xc2)
        }
        (SerialBoard::Sio88, cpu_diagnostics::DiagnosticSerialPort::Port1) => return None,
        (SerialBoard::TwoSio88, cpu_diagnostics::DiagnosticSerialPort::Port0) => {
            (0x10, 0x11, 0x02, 0xca)
        }
        (SerialBoard::TwoSio88, cpu_diagnostics::DiagnosticSerialPort::Port1) => {
            (0x12, 0x13, 0x02, 0xca)
        }
    };

    let mut page_zero = [0u8; CPM_PAGE_ZERO_SIZE];
    page_zero[0..3].copy_from_slice(&[0xc3, 0x80, 0x00]); // JMP 0080h
    let [bdos_lo, bdos_hi] = bdos_base.to_le_bytes();
    page_zero[5..8].copy_from_slice(&[0xc3, bdos_lo, bdos_hi]); // JMP BDOS

    let boot = [
        0x31, bdos_lo, bdos_hi, // LXI SP,bdos
        0x3e, 0x76,             // MVI A,HLT
        0x32, 0x00, 0x00,       // STA 0000h
        0xc3, 0x00, 0x01,       // JMP 0100h
    ];
    page_zero[BOOT_ADDRESS..BOOT_ADDRESS + boot.len()].copy_from_slice(&boot);

    const CHAR_OFFSET: u16 = 0x0012;
    const STRING_OFFSET: u16 = 0x0019;
    const DONE_OFFSET: u16 = 0x0026;
    const PUTC_OFFSET: u16 = 0x002b;
    const POLL_OFFSET: u16 = 0x002c;

    let char_addr = bdos_base.wrapping_add(CHAR_OFFSET);
    let string_addr = bdos_base.wrapping_add(STRING_OFFSET);
    let done_addr = bdos_base.wrapping_add(DONE_OFFSET);
    let putc_addr = bdos_base.wrapping_add(PUTC_OFFSET);
    let poll_addr = bdos_base.wrapping_add(POLL_OFFSET);

    let mut bdos = Vec::with_capacity(0x37);
    bdos.extend_from_slice(&[0xf5, 0xc5, 0xd5, 0xe5]); // PUSH PSW/B/D/H
    bdos.push(0x79); // MOV A,C
    bdos.extend_from_slice(&[0xfe, 0x02]); // CPI 2
    append_abs(&mut bdos, 0xca, char_addr);
    bdos.extend_from_slice(&[0xfe, 0x09]); // CPI 9
    append_abs(&mut bdos, 0xca, string_addr);
    append_abs(&mut bdos, 0xc3, done_addr);

    bdos.push(0x7b); // char: MOV A,E
    append_abs(&mut bdos, 0xcd, putc_addr);
    append_abs(&mut bdos, 0xc3, done_addr);

    bdos.push(0x1a); // string: LDAX D
    bdos.extend_from_slice(&[0xfe, 0x24]); // CPI '$'
    append_abs(&mut bdos, 0xca, done_addr);
    append_abs(&mut bdos, 0xcd, putc_addr);
    bdos.push(0x13); // INX D
    append_abs(&mut bdos, 0xc3, string_addr);

    bdos.extend_from_slice(&[0xe1, 0xd1, 0xc1, 0xf1, 0xc9]); // POP H/D/B/PSW; RET

    bdos.push(0x47); // putc: MOV B,A
    bdos.extend_from_slice(&[0xdb, status_port, 0xe6, ready_mask]);
    append_abs(&mut bdos, wait_branch, poll_addr);
    bdos.extend_from_slice(&[0x78, 0xd3, data_port, 0xc9]);
    debug_assert_eq!(bdos.len(), 0x37);

    Some(CpmDiagnosticEnvironment { page_zero, bdos_base, bdos })
}

fn port_connection(port: cpu_diagnostics::DiagnosticSerialPort) -> SerialConnection {
    match port {
        cpu_diagnostics::DiagnosticSerialPort::Port0 => SerialConnection::Port0,
        cpu_diagnostics::DiagnosticSerialPort::Port1 => SerialConnection::Port1,
    }
}

fn port_label(board: SerialBoard, port: cpu_diagnostics::DiagnosticSerialPort) -> &'static str {
    match (board, port) {
        (SerialBoard::Sio88, cpu_diagnostics::DiagnosticSerialPort::Port0) => {
            "88-SIO Port 0 [00h/01h]"
        }
        (SerialBoard::Sio88, cpu_diagnostics::DiagnosticSerialPort::Port1) => "Unavailable",
        (SerialBoard::TwoSio88, cpu_diagnostics::DiagnosticSerialPort::Port0) => {
            "88-2SIO Port 0 [10h/11h]"
        }
        (SerialBoard::TwoSio88, cpu_diagnostics::DiagnosticSerialPort::Port1) => {
            "88-2SIO Port 1 [12h/13h]"
        }
    }
}

fn reference_match(result: &CpuDiagnosticResult) -> bool {
    match (result.expected_instructions, result.expected_t_states) {
        (Some(i), Some(t)) => result.instructions == i && result.t_states == t,
        _ => false,
    }
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index != 0 && (digits.len() - index) % 3 == 0 { out.push(','); }
        out.push(ch);
    }
    out
}

fn format_diff(actual: u64, expected: u64) -> String {
    let diff = actual as i128 - expected as i128;
    if diff > 0 { format!("+{diff}") } else { diff.to_string() }
}

fn format_2mhz_duration(t_states: u64) -> String {
    let total_millis = t_states.saturating_mul(1_000) / u64::from(CLOCK_HZ);
    let hours = total_millis / 3_600_000;
    let minutes = (total_millis / 60_000) % 60;
    let seconds = (total_millis / 1_000) % 60;
    let millis = total_millis % 1_000;
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}.{millis:03}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}.{millis:03}s")
    } else {
        format!("{seconds}.{millis:03}s")
    }
}

struct ProbeBus {
    mem: [u8; 65536],
    input_value: u8,
    last_input: Option<u8>,
    last_output: Option<(u8, u8)>,
    halt_ack: bool,
    interrupt_ack: Option<(u16, u8, bool)>,
}

impl Default for ProbeBus {
    fn default() -> Self {
        Self {
            mem: [0; 65536],
            input_value: 0,
            last_input: None,
            last_output: None,
            halt_ack: false,
            interrupt_ack: None,
        }
    }
}

impl Bus for ProbeBus {
    fn read(&mut self, address: u16) -> u8 { self.mem[address as usize] }
    fn write(&mut self, address: u16, value: u8) { self.mem[address as usize] = value; }
    fn input(&mut self, port: u8) -> u8 {
        self.last_input = Some(port);
        self.input_value
    }
    fn output(&mut self, port: u8, value: u8) { self.last_output = Some((port, value)); }
    fn halt_ack(&mut self, _address: u16, _opcode: u8) { self.halt_ack = true; }
    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        self.interrupt_ack = Some((address, opcode, while_halted));
    }
}

fn run_control_line_baseline() -> ControlLineReport {
    let mut checks = Vec::new();

    // EI enables only after the following instruction, while DI must then clear INTE.
    let mut bus = ProbeBus::default();
    bus.mem[0..3].copy_from_slice(&[0xfb, 0x00, 0xf3]); // EI; NOP; DI
    let mut cpu = Cpu8080::new();
    let ei_t = cpu.step(&mut bus);
    let inte_after_ei = cpu.inte;
    let nop_t = cpu.step(&mut bus);
    let inte_after_nop = cpu.inte;
    let di_t = cpu.step(&mut bus);
    let inte_after_di = cpu.inte;
    let ei_di_ok = ei_t == 4
        && nop_t == 4
        && di_t == 4
        && !inte_after_ei
        && inte_after_nop
        && !inte_after_di;
    checks.push(ControlCheck {
        name: "EI delay / DI",
        passed: ei_di_ok,
        detail: format!(
            "INTE after EI={} · after NOP={} · after DI={}",
            inte_after_ei, inte_after_nop, inte_after_di
        ),
    });

    // DI immediately following EI must win over the delayed enable.
    let mut bus = ProbeBus::default();
    bus.mem[0..2].copy_from_slice(&[0xfb, 0xf3]); // EI; DI
    let mut cpu = Cpu8080::new();
    cpu.step(&mut bus);
    let accepted_before_di = cpu.interrupt(&mut bus, 0xcf);
    cpu.step(&mut bus);
    let inte_after_immediate_di = cpu.inte;
    let accepted_after_di = cpu.interrupt(&mut bus, 0xcf);
    checks.push(ControlCheck {
        name: "EI immediately followed by DI",
        passed: !accepted_before_di && !inte_after_immediate_di && !accepted_after_di,
        detail: format!(
            "INT accepted before DI={} · INTE after DI={} · INT accepted after DI={}",
            accepted_before_di, inte_after_immediate_di, accepted_after_di
        ),
    });

    // HLT must acknowledge HALT. An accepted interrupt wakes it, clears INTE,
    // acknowledges INTA and executes the supplied RST opcode with a real stack push.
    let mut bus = ProbeBus::default();
    bus.mem[0] = 0x76;
    let mut cpu = Cpu8080::new();
    cpu.sp = 0x2000;
    cpu.inte = true;
    let hlt_t = cpu.step(&mut bus);
    let halted = cpu.halted && bus.halt_ack && hlt_t == 7 && cpu.pc == 1;
    let accepted = cpu.interrupt(&mut bus, 0xcf); // RST 1
    let return_address = u16::from_le_bytes([bus.mem[0x1ffe], bus.mem[0x1fff]]);
    let interrupt_ok = accepted
        && !cpu.halted
        && !cpu.inte
        && cpu.pc == 0x0008
        && cpu.sp == 0x1ffe
        && return_address == 0x0001
        && bus.interrupt_ack == Some((0x0001, 0xcf, true));
    checks.push(ControlCheck {
        name: "HLT wake / INTA / RST stack",
        passed: halted && interrupt_ok,
        detail: format!(
            "HALT={} · accepted={} · PC={:04X} · SP={:04X} · return={:04X}",
            halted, accepted, cpu.pc, cpu.sp, return_address
        ),
    });

    // IN and OUT must use the abstract CPU bus contract.
    let mut bus = ProbeBus::default();
    bus.input_value = 0xa5;
    bus.mem[0..4].copy_from_slice(&[0xdb, 0x42, 0xd3, 0x43]);
    let mut cpu = Cpu8080::new();
    let in_t = cpu.step(&mut bus);
    let in_ok = in_t == 10 && cpu.a == 0xa5 && bus.last_input == Some(0x42);
    let out_t = cpu.step(&mut bus);
    let out_ok = out_t == 10 && bus.last_output == Some((0x43, 0xa5));
    checks.push(ControlCheck {
        name: "IN / OUT bus contract",
        passed: in_ok && out_ok,
        detail: format!(
            "IN port={:?} → A={:02X} · OUT={:?}",
            bus.last_input, cpu.a, bus.last_output
        ),
    });

    // The current core is instruction-granular: this freezes the present
    // front-panel READY/WAIT and HOLD/HLDA contract, not true TW insertion.
    let mut machine = AltairMachine::default();
    machine.power(true);
    machine.set_running(false);
    let wait_when_stopped = machine.wait_led();
    machine.set_running(true);
    let ready_when_running = !machine.wait_led();
    let pc_before_hold = machine.cpu.pc;
    machine.request_hold(true);
    machine.run_cycles(16);
    machine.commit_panel_activity(Duration::from_secs(1));
    let hlda_asserted = machine.panel_lamps().hlda == 1.0;
    let cpu_frozen = machine.cpu.pc == pc_before_hold;
    machine.request_hold(false);
    machine.set_running(false);
    machine.commit_panel_activity(Duration::from_secs(1));
    let hlda_released = machine.panel_lamps().hlda == 0.0;
    checks.push(ControlCheck {
        name: "READY/WAIT + HOLD/HLDA baseline",
        passed: wait_when_stopped
            && ready_when_running
            && hlda_asserted
            && cpu_frozen
            && hlda_released,
        detail: format!(
            "WAIT@STOP={} · READY@RUN={} · HLDA={} · CPU frozen={} · HLDA released={}",
            wait_when_stopped, ready_when_running, hlda_asserted, cpu_frozen, hlda_released
        ),
    });

    ControlLineReport { checks }
}

impl RusTairApp {
    pub(in crate::app) fn effective_emulation_speed(&self) -> EmulationSpeed {
        if self.embedded_diagnostics.active_test.is_some()
            || self.embedded_diagnostics.suite.is_some()
        {
            self.embedded_diagnostics.speed.emulation_speed()
        } else {
            self.config.preferences.emulation_speed
        }
    }

    pub(in crate::app) fn draw_cpu_diagnostics_menu(&mut self, ui: &mut egui::Ui) {
        if self.config.machine.serial_board == SerialBoard::Sio88
            && self.embedded_diagnostics.port == cpu_diagnostics::DiagnosticSerialPort::Port1
        {
            self.embedded_diagnostics.port = cpu_diagnostics::DiagnosticSerialPort::Port0;
        }

        let picker_open = self.diagnostic_file_dialog.is_some();
        let running = self.embedded_diagnostics.active_test.is_some()
            || self.embedded_diagnostics.suite.is_some();
        let busy = picker_open || running;

        ui.small(
            "Embedded Intel 8080 tests execute as real guest code through the normal Altair machine. The RusTair baseline covers interrupt/HALT/I-O and bus-arbitration behaviour that the classic CP/M tests largely omit.",
        );
        ui.separator();

        ui.menu_button("Test speed", |ui| {
            ui.add_enabled_ui(!running, |ui| {
                for speed in DiagnosticRunSpeed::ALL {
                    if ui
                        .selectable_label(
                            self.embedded_diagnostics.speed == speed,
                            speed.label(),
                        )
                        .clicked()
                    {
                        self.embedded_diagnostics.speed = speed;
                    }
                }
            });
        });
        ui.small(format!("Selected speed: {}", self.embedded_diagnostics.speed.label()));

        ui.menu_button("Serial output", |ui| {
            let board = self.config.machine.serial_board;
            let p0 = cpu_diagnostics::DiagnosticSerialPort::Port0;
            if ui
                .selectable_label(self.embedded_diagnostics.port == p0, port_label(board, p0))
                .clicked()
            {
                self.embedded_diagnostics.port = p0;
            }
            if board == SerialBoard::TwoSio88 {
                let p1 = cpu_diagnostics::DiagnosticSerialPort::Port1;
                if ui
                    .selectable_label(self.embedded_diagnostics.port == p1, port_label(board, p1))
                    .clicked()
                {
                    self.embedded_diagnostics.port = p1;
                }
            }
        });

        ui.separator();
        if ui
            .add_enabled(!busy, egui::Button::new("Run full CPU diagnostic suite"))
            .clicked()
        {
            self.start_embedded_cpu_suite();
            ui.close();
        }
        ui.small(
            "Suite: RusTair baseline → 8080PRE → TST8080 → CPUTEST → 8080EXM. Requires at least 32 KiB RAM.",
        );

        ui.menu_button("Run individual test", |ui| {
            let enabled = !busy;
            if ui
                .add_enabled(enabled, egui::Button::new("RusTair control-line baseline"))
                .clicked()
            {
                let report = run_control_line_baseline();
                let passed = report.passed();
                self.embedded_diagnostics.control_report = Some(report);
                self.status = if passed {
                    "RusTair control-line baseline: PASS".into()
                } else {
                    "RusTair control-line baseline: FAIL — inspect report".into()
                };
                ui.close();
            }
            ui.separator();
            for test in ClassicDiagnostic::SUITE {
                if ui
                    .add_enabled(enabled, egui::Button::new(test.label()))
                    .clicked()
                {
                    self.start_embedded_classic_test(test, false);
                    ui.close();
                }
            }
        });

        ui.separator();
        if ui
            .add_enabled(!busy, egui::Button::new("Load external .COM…"))
            .clicked()
        {
            self.start_cpu_diagnostic_dialog(self.embedded_diagnostics.port);
            ui.close();
        }
        ui.small("External .COM files continue to use the normal emulator speed preference and the existing generic result reporter.");

        if running && !picker_open {
            ui.separator();
            if ui.button("Abort running diagnostic / suite").clicked() {
                self.abort_embedded_cpu_diagnostics();
                ui.close();
            }
        }
        if picker_open {
            ui.small("The Windows diagnostic picker is open; guest execution is paused until it closes.");
        }
    }

    pub(in crate::app) fn poll_embedded_cpu_diagnostics(&mut self, ctx: &egui::Context) {
        if self.embedded_diagnostics.active_test.is_some()
            || self.embedded_diagnostics.suite.is_some()
        {
            if let Some(result) = self.machine.take_cpu_diagnostic_result() {
                self.handle_embedded_cpu_result(result);
            }
        }

        self.draw_embedded_individual_result(ctx);
        self.draw_control_line_report(ctx);
        self.draw_suite_report(ctx);
    }

    fn start_embedded_cpu_suite(&mut self) {
        if self.machine.installed_ram_bytes() < 32 * 1024 {
            self.report_load_error(format!(
                "The full embedded CPU diagnostic suite includes CPUTEST.COM and requires at least 32 KiB RAM. The current machine has {}. Configure 32, 48 or 64 KiB and run the suite again.",
                self.config.machine.ram_size.label()
            ));
            return;
        }

        let control = run_control_line_baseline();
        self.embedded_diagnostics.individual_result = None;
        self.embedded_diagnostics.control_report = None;
        self.embedded_diagnostics.suite_report = None;
        self.embedded_diagnostics.suite = Some(SuiteRun {
            next_index: 1,
            control,
            results: Vec::with_capacity(ClassicDiagnostic::SUITE.len()),
        });

        if !self.start_embedded_classic_test(ClassicDiagnostic::SUITE[0], true) {
            self.embedded_diagnostics.suite = None;
        }
    }

    fn start_embedded_classic_test(&mut self, test: ClassicDiagnostic, suite_member: bool) -> bool {
        let port = self.embedded_diagnostics.port;
        self.embedded_diagnostics.active_test = Some(test);
        if !suite_member {
            self.embedded_diagnostics.suite = None;
            self.embedded_diagnostics.suite_report = None;
        }

        if self.load_embedded_classic_test(test, port) {
            true
        } else {
            self.embedded_diagnostics.active_test = None;
            false
        }
    }

    fn load_embedded_classic_test(
        &mut self,
        test: ClassicDiagnostic,
        port: cpu_diagnostics::DiagnosticSerialPort,
    ) -> bool {
        let bytes = test.bytes();
        let board = self.config.machine.serial_board;
        let connection = port_connection(port);

        if board == SerialBoard::Sio88 && port == cpu_diagnostics::DiagnosticSerialPort::Port1 {
            self.report_load_error(
                "CPU diagnostic cannot use Port 1 because the installed MITS 88-SIO only provides Port 0.",
            );
            return false;
        }

        let installed = self.machine.installed_ram_bytes();
        let image_end = CPM_COM_LOAD_ADDRESS as usize + bytes.len();
        let minimum_bytes = image_end
            .saturating_add(CPM_STACK_GUARD_BYTES)
            .saturating_add(CPM_BDOS_PAGE_BYTES);
        let Some(bdos_base_usize) = installed.checked_sub(CPM_BDOS_PAGE_BYTES) else {
            self.report_load_error(format!(
                "{} cannot start because {} RAM is too small for the CP/M diagnostic environment.",
                test.filename(), self.config.machine.ram_size.label()
            ));
            return false;
        };
        let Some(tpa_limit) = bdos_base_usize.checked_sub(CPM_STACK_GUARD_BYTES) else {
            self.report_load_error(format!("{} has no stack area below BDOS.", test.filename()));
            return false;
        };
        if image_end > tpa_limit {
            self.report_load_error(format!(
                "{} is {} bytes and needs at least {} KiB including the CP/M stack/BDOS reserve. The current machine has {}.",
                test.filename(), bytes.len(), minimum_bytes.div_ceil(1024), self.config.machine.ram_size.label()
            ));
            return false;
        }

        let bdos_base = bdos_base_usize as u16;
        let Some(environment) = build_cpm_environment(board, port, bdos_base) else {
            self.report_load_error(format!(
                "{} is unavailable on {}.",
                port_label(board, port), board.label()
            ));
            return false;
        };

        // Convenience loader: deterministic power/reset/RAM setup, then let the
        // real emulated 8080 execute the complete test and mini-BDOS itself.
        if !self.machine.powered { self.set_altair_power(true); }
        self.machine.set_running(false);
        self.machine.reset();
        self.asr33.tx_started = None;
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
        self.machine.bus.clear_protection();
        self.machine.bus.clear_transient_memory_guards();
        let clean_ram = vec![0u8; installed];
        self.machine.bus.load(0, &clean_ram);
        self.machine.bus.load(0, &environment.page_zero);
        self.machine.bus.load(CPM_COM_LOAD_ADDRESS, bytes);
        self.machine.bus.load(environment.bdos_base, &environment.bdos);
        self.machine.cpu.pc = 0;

        self.machine.begin_cpu_diagnostic_meter(
            test.filename().into(),
            environment.bdos_base,
            environment.bdos.len(),
            Some(test.expected_instructions()),
            Some(test.expected_t_states()),
        );

        match self.serial_router.device_on(connection) {
            Some(SerialDevice::InternalAsr33) => self.asr33.window_open = true,
            Some(SerialDevice::TextTerminal) => self.terminal.window_open = true,
            Some(SerialDevice::ExternalTcp) => self.external_serial.window_open = true,
            Some(SerialDevice::ExternalCom) => self.external_com.window_open = true,
            None => {}
        }

        self.machine.set_running(true);
        let endpoint = self
            .serial_router
            .device_on(connection)
            .map(Self::serial_device_name)
            .unwrap_or("no endpoint connected");
        self.status = format!(
            "Embedded CPU diagnostic running: {} — {} — output via {} → {}",
            test.filename(),
            self.embedded_diagnostics.speed.label(),
            port_label(board, port),
            endpoint
        );
        true
    }

    fn handle_embedded_cpu_result(&mut self, result: CpuDiagnosticResult) {
        self.embedded_diagnostics.active_test = None;

        if let Some(mut suite) = self.embedded_diagnostics.suite.take() {
            suite.results.push(result);
            if suite.next_index < ClassicDiagnostic::SUITE.len() {
                let next_test = ClassicDiagnostic::SUITE[suite.next_index];
                suite.next_index += 1;
                self.embedded_diagnostics.suite = Some(suite);
                if !self.start_embedded_classic_test(next_test, true) {
                    self.embedded_diagnostics.suite = None;
                }
                return;
            }

            let report = SuiteReport {
                control: suite.control,
                results: suite.results,
            };
            let passed = report.passed();
            self.embedded_diagnostics.suite_report = Some(report);
            self.status = if passed {
                "Embedded CPU diagnostic suite complete — ALL TESTS PASS / REFERENCE MATCH".into()
            } else {
                "Embedded CPU diagnostic suite complete — FAILURE / REFERENCE MISMATCH".into()
            };
        } else {
            let matched = reference_match(&result);
            self.status = if matched {
                format!("{} complete — REFERENCE MATCH", result.name)
            } else {
                format!("{} complete — REFERENCE MISMATCH", result.name)
            };
            self.embedded_diagnostics.individual_result = Some(result);
        }
    }

    fn abort_embedded_cpu_diagnostics(&mut self) {
        self.machine.set_running(false);
        self.machine.bus.cancel_cpu_diagnostic_meter();
        self.embedded_diagnostics.active_test = None;
        self.embedded_diagnostics.suite = None;
        self.status = "CPU diagnostic / suite aborted; machine left stopped".into();
    }

    fn draw_embedded_individual_result(&mut self, ctx: &egui::Context) {
        let Some(result) = self.embedded_diagnostics.individual_result.as_ref() else { return; };
        let matched = reference_match(result);
        let mut dismiss = false;
        egui::Window::new("CPU diagnostic complete")
            .id(egui::Id::new("embedded-cpu-diagnostic-result"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.heading(&result.name);
                ui.add_space(6.0);
                if matched {
                    ui.strong("REFERENCE MATCH — instruction count and T-state total are exact.");
                } else {
                    ui.strong("REFERENCE MISMATCH — inspect the differences below.");
                }
                ui.add_space(8.0);
                egui::Grid::new("embedded-cpu-result-grid")
                    .num_columns(4)
                    .spacing([18.0, 5.0])
                    .show(ui, |ui| {
                        ui.strong("Metric");
                        ui.strong("Actual");
                        ui.strong("Expected");
                        ui.strong("Diff");
                        ui.end_row();

                        ui.label("Instructions");
                        ui.monospace(format_count(result.instructions));
                        if let Some(expected) = result.expected_instructions {
                            ui.monospace(format_count(expected));
                            ui.monospace(format_diff(result.instructions, expected));
                        } else {
                            ui.label("—");
                            ui.label("—");
                        }
                        ui.end_row();

                        ui.label("T-states");
                        ui.monospace(format_count(result.t_states));
                        if let Some(expected) = result.expected_t_states {
                            ui.monospace(format_count(expected));
                            ui.monospace(format_diff(result.t_states, expected));
                        } else {
                            ui.label("—");
                            ui.label("—");
                        }
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.label(format!(
                    "Equivalent 8080 time at 2 MHz: {}",
                    format_2mhz_duration(result.t_states)
                ));
                if ui.button("OK").clicked() { dismiss = true; }
            });
        if dismiss { self.embedded_diagnostics.individual_result = None; }
    }

    fn draw_control_line_report(&mut self, ctx: &egui::Context) {
        let Some(report) = self.embedded_diagnostics.control_report.as_ref() else { return; };
        let passed = report.passed();
        let mut dismiss = false;
        egui::Window::new("RusTair 8080 control-line baseline")
            .id(egui::Id::new("rustair-control-line-baseline"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(true)
            .default_width(700.0)
            .show(ctx, |ui| {
                if passed {
                    ui.strong("PASS — all baseline checks succeeded.");
                } else {
                    ui.strong("FAIL — at least one baseline check failed.");
                }
                ui.add_space(8.0);
                for check in &report.checks {
                    let line = format!(
                        "{}  {} - {}",
                        if check.passed { "PASS" } else { "FAIL" },
                        check.name,
                        check.detail
                    )
                    .replace('·', "|")
                    .replace('→', "->");
                    ui.label(egui::RichText::new(line).monospace());
                }
                ui.add_space(8.0);
                ui.small(
                    "READY/WAIT freezes the current instruction-level front-panel contract. True T-state TW insertion will be a separate requirement for the cycle-accurate core.",
                );
                if ui.button("OK").clicked() { dismiss = true; }
            });
        if dismiss { self.embedded_diagnostics.control_report = None; }
    }

    fn draw_suite_report(&mut self, ctx: &egui::Context) {
        let Some(report) = self.embedded_diagnostics.suite_report.as_ref() else { return; };
        let passed = report.passed();
        let mut dismiss = false;
        egui::Window::new("CPU diagnostic suite complete")
            .id(egui::Id::new("embedded-cpu-suite-result"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(true)
            .default_width(780.0)
            .show(ctx, |ui| {
                if passed {
                    ui.heading("ALL TESTS PASS");
                    ui.strong("All classic instruction/T-state references match exactly.");
                } else {
                    ui.heading("SUITE FAILURE");
                    ui.strong("Inspect the failing row or control-line check below.");
                }
                ui.add_space(8.0);
                ui.label(format!(
                    "RusTair control-line baseline: {}",
                    if report.control.passed() { "PASS" } else { "FAIL" }
                ));
                for check in &report.control.checks {
                    ui.small(format!(
                        "{}  {} — {}",
                        if check.passed { "PASS" } else { "FAIL" },
                        check.name,
                        check.detail
                    ));
                }
                ui.separator();
                egui::Grid::new("embedded-cpu-suite-grid")
                    .num_columns(5)
                    .spacing([16.0, 5.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Test");
                        ui.strong("Result");
                        ui.strong("Instructions");
                        ui.strong("T-states");
                        ui.strong("T diff");
                        ui.end_row();
                        for result in &report.results {
                            let ok = reference_match(result);
                            ui.monospace(&result.name);
                            ui.monospace(if ok { "PASS" } else { "FAIL" });
                            ui.monospace(format_count(result.instructions));
                            ui.monospace(format_count(result.t_states));
                            if let Some(expected) = result.expected_t_states {
                                ui.monospace(format_diff(result.t_states, expected));
                            } else {
                                ui.label("—");
                            }
                            ui.end_row();
                        }
                    });
                ui.add_space(10.0);
                if ui.button("OK").clicked() { dismiss = true; }
            });
        if dismiss { self.embedded_diagnostics.suite_report = None; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_classic_images_and_reference_totals_are_stable() {
        assert_eq!(ClassicDiagnostic::Preliminary.bytes().len(), 1024);
        assert_eq!(ClassicDiagnostic::Tst8080.bytes().len(), 1536);
        assert_eq!(ClassicDiagnostic::CpuTest.bytes().len(), 19200);
        assert_eq!(ClassicDiagnostic::ExerciserModified.bytes().len(), 4608);
        assert_eq!(ClassicDiagnostic::Preliminary.expected_t_states(), 7_817);
        assert_eq!(ClassicDiagnostic::Tst8080.expected_t_states(), 4_924);
        assert_eq!(ClassicDiagnostic::CpuTest.expected_t_states(), 255_653_383);
        assert_eq!(
            ClassicDiagnostic::ExerciserModified.expected_t_states(),
            23_803_381_171
        );
    }

    #[test]
    fn rustair_control_line_baseline_passes() {
        let report = run_control_line_baseline();
        assert!(report.passed(), "{:#?}", report.checks);
    }

    #[test]
    fn embedded_bdos_remains_high_memory_compatible() {
        let env = build_cpm_environment(
            SerialBoard::TwoSio88,
            cpu_diagnostics::DiagnosticSerialPort::Port0,
            0xff00,
        )
        .unwrap();
        assert_eq!(&env.page_zero[5..8], &[0xc3, 0x00, 0xff]);
        assert_eq!(&env.page_zero[0x80..0x83], &[0x31, 0x00, 0xff]);
        assert_eq!(env.bdos.len(), 0x37);
    }
}
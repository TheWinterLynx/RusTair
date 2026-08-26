use super::*;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

const CPM_COM_LOAD_ADDRESS: u16 = 0x0100;
const CPM_PAGE_ZERO_SIZE: usize = 0x0100;
const BOOT_ADDRESS: usize = 0x0080;
const CPM_BDOS_PAGE_BYTES: usize = 0x0100;
const CPM_STACK_GUARD_BYTES: usize = 0x0100;
const DIAGNOSTIC_RESULT_DIALOG_ID: &str = "rustair-cpu-diagnostic-result-dialog";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum DiagnosticSerialPort { Port0, Port1 }

impl DiagnosticSerialPort {
    fn connection(self) -> SerialConnection {
        match self { Self::Port0 => SerialConnection::Port0, Self::Port1 => SerialConnection::Port1 }
    }
    fn label(self, board: SerialBoard) -> &'static str {
        match (board, self) {
            (SerialBoard::Sio88, Self::Port0) => "88-SIO Port 0 [00h/01h]",
            (SerialBoard::Sio88, Self::Port1) => "unavailable",
            (SerialBoard::TwoSio88, Self::Port0) => "88-2SIO Port 0 [10h/11h]",
            (SerialBoard::TwoSio88, Self::Port1) => "88-2SIO Port 1 [12h/13h]",
        }
    }
}

pub(in crate::app) struct DiagnosticFileDialog {
    receiver: Receiver<Option<std::path::PathBuf>>,
    port: DiagnosticSerialPort,
    resume_on_cancel: bool,
}

struct CpmDiagnosticEnvironment {
    page_zero: [u8; CPM_PAGE_ZERO_SIZE],
    bdos_base: u16,
    bdos: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiagnosticReference { instructions: u64, t_states: u64 }

fn diagnostic_reference(path: &Path) -> Option<DiagnosticReference> {
    let name = path.file_name()?.to_string_lossy().to_ascii_uppercase();
    match name.as_str() {
        "TST8080.COM" => Some(DiagnosticReference { instructions: 651, t_states: 4_924 }),
        "8080PRE.COM" => Some(DiagnosticReference { instructions: 1_061, t_states: 7_817 }),
        "CPUTEST.COM" => Some(DiagnosticReference { instructions: 33_971_311, t_states: 255_653_383 }),
        "8080EXM.COM" => Some(DiagnosticReference { instructions: 2_919_050_698, t_states: 23_803_381_171 }),
        _ => None,
    }
}

fn diagnostic_display_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
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
    if hours > 0 { format!("{hours}h {minutes:02}m {seconds:02}.{millis:03}s") }
    else if minutes > 0 { format!("{minutes}m {seconds:02}.{millis:03}s") }
    else { format!("{seconds}.{millis:03}s") }
}

fn append_abs(code: &mut Vec<u8>, opcode: u8, address: u16) {
    let [lo, hi] = address.to_le_bytes();
    code.extend_from_slice(&[opcode, lo, hi]);
}

fn build_cpm_diagnostic_environment(board: SerialBoard, port: DiagnosticSerialPort, bdos_base: u16) -> Option<CpmDiagnosticEnvironment> {
    let (status_port, data_port, ready_mask, wait_branch) = match (board, port) {
        (SerialBoard::Sio88, DiagnosticSerialPort::Port0) => (0x00, 0x01, 0xc0, 0xc2),
        (SerialBoard::Sio88, DiagnosticSerialPort::Port1) => return None,
        (SerialBoard::TwoSio88, DiagnosticSerialPort::Port0) => (0x10, 0x11, 0x02, 0xca),
        (SerialBoard::TwoSio88, DiagnosticSerialPort::Port1) => (0x12, 0x13, 0x02, 0xca),
    };

    let mut page_zero = [0u8; CPM_PAGE_ZERO_SIZE];
    page_zero[0x0000..0x0003].copy_from_slice(&[0xc3, 0x80, 0x00]);
    let [bdos_lo, bdos_hi] = bdos_base.to_le_bytes();
    page_zero[0x0005..0x0008].copy_from_slice(&[0xc3, bdos_lo, bdos_hi]);
    let boot = [
        0x31, bdos_lo, bdos_hi,
        0x3e, 0x76,
        0x32, 0x00, 0x00,
        0xc3, 0x00, 0x01,
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
    bdos.extend_from_slice(&[0xf5, 0xc5, 0xd5, 0xe5]);
    bdos.push(0x79);
    bdos.extend_from_slice(&[0xfe, 0x02]); append_abs(&mut bdos, 0xca, char_addr);
    bdos.extend_from_slice(&[0xfe, 0x09]); append_abs(&mut bdos, 0xca, string_addr);
    append_abs(&mut bdos, 0xc3, done_addr);
    debug_assert_eq!(bdos.len(), CHAR_OFFSET as usize);
    bdos.push(0x7b); append_abs(&mut bdos, 0xcd, putc_addr); append_abs(&mut bdos, 0xc3, done_addr);
    debug_assert_eq!(bdos.len(), STRING_OFFSET as usize);
    bdos.push(0x1a); bdos.extend_from_slice(&[0xfe, 0x24]); append_abs(&mut bdos, 0xca, done_addr);
    append_abs(&mut bdos, 0xcd, putc_addr); bdos.push(0x13); append_abs(&mut bdos, 0xc3, string_addr);
    debug_assert_eq!(bdos.len(), DONE_OFFSET as usize);
    bdos.extend_from_slice(&[0xe1, 0xd1, 0xc1, 0xf1, 0xc9]);
    debug_assert_eq!(bdos.len(), PUTC_OFFSET as usize);
    bdos.push(0x47); bdos.push(0xdb); bdos.push(status_port); bdos.push(0xe6); bdos.push(ready_mask);
    append_abs(&mut bdos, wait_branch, poll_addr);
    bdos.push(0x78); bdos.push(0xd3); bdos.push(data_port); bdos.push(0xc9);
    debug_assert_eq!(bdos.len(), 0x37);

    Some(CpmDiagnosticEnvironment { page_zero, bdos_base, bdos })
}

impl RusTairApp {
    pub(in crate::app) fn start_cpu_diagnostic_dialog(&mut self, port: DiagnosticSerialPort) {
        if self.diagnostic_file_dialog.is_some() {
            self.status = "A CPU diagnostic file dialog is already open".into();
            return;
        }
        if self.config.machine.serial_board == SerialBoard::Sio88 && port == DiagnosticSerialPort::Port1 {
            self.report_load_error("CPU diagnostic cannot use Port 1 because the installed MITS 88-SIO only provides Port 0. Select Port 0 or install the MITS 88-2SIO.");
            return;
        }

        let resume_on_cancel = self.machine.running();
        if resume_on_cancel { self.machine.set_running(false); }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let selected = rfd::FileDialog::new().add_filter("CP/M 8080 diagnostic", &["com", "bin"]).pick_file();
            let _ = sender.send(selected);
        });
        self.diagnostic_file_dialog = Some(DiagnosticFileDialog { receiver, port, resume_on_cancel });
        self.status = "CPU diagnostic paused — choose a .COM file".into();
    }

    pub(in crate::app) fn poll_cpu_diagnostic_dialog(&mut self, ctx: &egui::Context) {
        self.poll_cpu_diagnostic_result(ctx);
        self.draw_load_error_dialog(ctx);

        let result = match self.diagnostic_file_dialog.as_ref() {
            Some(dialog) => match dialog.receiver.try_recv() {
                Ok(path) => Some(Ok((path, dialog.port, dialog.resume_on_cancel))),
                Err(TryRecvError::Empty) => { ctx.request_repaint_after(Duration::from_millis(50)); None }
                Err(TryRecvError::Disconnected) => Some(Err(())),
            },
            None => None,
        };
        let Some(result) = result else { return; };
        self.diagnostic_file_dialog = None;

        match result {
            Err(()) => self.report_load_error("The Windows CPU diagnostic file picker terminated unexpectedly before returning a file."),
            Ok((None, _, resume_on_cancel)) => {
                if resume_on_cancel { self.machine.set_running(true); }
                self.status = if resume_on_cancel { "CPU diagnostic selection cancelled — previous machine resumed".into() } else { "CPU diagnostic selection cancelled".into() };
            }
            Ok((Some(path), port, _)) => match std::fs::read(&path) {
                Ok(bytes) => self.load_cpu_diagnostic(&path, &bytes, port),
                Err(e) => self.report_load_error(format!("Could not read CPU diagnostic {}: {e}", path.display())),
            },
        }
    }

    fn poll_cpu_diagnostic_result(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new(DIAGNOSTIC_RESULT_DIALOG_ID);
        if let Some(result) = self.machine.take_cpu_diagnostic_result() {
            let instruction_match = result.expected_instructions.map(|expected| expected == result.instructions);
            let timing_match = result.expected_t_states.map(|expected| expected == result.t_states);
            let reference_match = match (instruction_match, timing_match) { (Some(a), Some(b)) => Some(a && b), _ => None };
            self.status = match reference_match {
                Some(true) => format!("CPU diagnostic complete: {} — REFERENCE MATCH — {} instructions — {} T-states", result.name, format_count(result.instructions), format_count(result.t_states)),
                Some(false) => format!("CPU diagnostic complete: {} — REFERENCE MISMATCH — {} instructions — {} T-states", result.name, format_count(result.instructions), format_count(result.t_states)),
                None => format!("CPU diagnostic complete: {} — {} instructions — {} T-states (no registered reference)", result.name, format_count(result.instructions), format_count(result.t_states)),
            };
            ctx.data_mut(|data| data.insert_temp(id, result));
        }

        let Some(result) = ctx.data(|data| data.get_temp::<crate::machine::CpuDiagnosticResult>(id)) else { return; };
        let reference_match = match (result.expected_instructions, result.expected_t_states) {
            (Some(expected_i), Some(expected_t)) => Some(result.instructions == expected_i && result.t_states == expected_t),
            _ => None,
        };
        let mut dismissed = false;
        egui::Window::new("CPU diagnostic complete")
            .id(egui::Id::new("rustair-cpu-diagnostic-result-window"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false).resizable(false).default_width(560.0)
            .show(ctx, |ui| {
                ui.heading(&result.name); ui.add_space(6.0);
                match reference_match {
                    Some(true) => { ui.strong("REFERENCE MATCH — instruction count and T-state total are exact."); }
                    Some(false) => { ui.strong("REFERENCE MISMATCH — inspect the differences below."); }
                    None => { ui.strong("Measurement complete — no reference totals are registered for this file."); }
                }
                ui.add_space(8.0);
                egui::Grid::new("cpu-diagnostic-result-grid").num_columns(4).spacing([18.0, 5.0]).show(ui, |ui| {
                    ui.strong("Metric"); ui.strong("Actual"); ui.strong("Expected"); ui.strong("Diff"); ui.end_row();
                    ui.label("Instructions"); ui.monospace(format_count(result.instructions));
                    if let Some(expected) = result.expected_instructions { ui.monospace(format_count(expected)); ui.monospace(format_diff(result.instructions, expected)); }
                    else { ui.label("—"); ui.label("—"); }
                    ui.end_row();
                    ui.label("T-states"); ui.monospace(format_count(result.t_states));
                    if let Some(expected) = result.expected_t_states { ui.monospace(format_count(expected)); ui.monospace(format_diff(result.t_states, expected)); }
                    else { ui.label("—"); ui.label("—"); }
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.label(format!("Equivalent 8080 time at 2 MHz: {}", format_2mhz_duration(result.t_states)));
                ui.small("For comparison with the classic test harness, accounting starts at 0100h, normalizes each CP/M CALL 0005h to the reference OUT+RET stub, and normalizes the final warm boot at 0000h to OUT 0. RusTair still executes the real high-memory mini-BDOS, UART polling and serial hardware; only the reported comparison counters are normalized.");
                ui.add_space(10.0);
                if ui.button("OK").clicked() { dismissed = true; }
            });
        if dismissed { ctx.data_mut(|data| data.remove::<crate::machine::CpuDiagnosticResult>(id)); }
    }

    fn load_cpu_diagnostic(&mut self, path: &std::path::Path, bytes: &[u8], port: DiagnosticSerialPort) {
        if bytes.is_empty() {
            self.report_load_error(format!("CPU diagnostic {} is empty (0 bytes). Nothing was loaded.", path.display()));
            return;
        }

        let board = self.config.machine.serial_board;
        let connection = port.connection();
        if board == SerialBoard::Sio88 && port == DiagnosticSerialPort::Port1 {
            self.report_load_error("CPU diagnostic cannot use Port 1 because the installed MITS 88-SIO only provides Port 0.");
            return;
        }

        let installed = self.machine.installed_ram_bytes();
        let image_end = CPM_COM_LOAD_ADDRESS as usize + bytes.len();
        let minimum_bytes = image_end.saturating_add(CPM_STACK_GUARD_BYTES).saturating_add(CPM_BDOS_PAGE_BYTES);
        let Some(bdos_base_usize) = installed.checked_sub(CPM_BDOS_PAGE_BYTES) else {
            self.report_load_error(format!("CPU diagnostic {} cannot start because the current {} RAM configuration is too small for a CP/M page-zero and BDOS environment.", path.display(), self.config.machine.ram_size.label()));
            return;
        };
        let Some(tpa_limit) = bdos_base_usize.checked_sub(CPM_STACK_GUARD_BYTES) else {
            self.report_load_error(format!("CPU diagnostic {} cannot start because the current {} RAM configuration leaves no stack area below BDOS.", path.display(), self.config.machine.ram_size.label()));
            return;
        };
        if image_end > tpa_limit {
            self.report_load_error(format!("CPU diagnostic {} is {} bytes and loads at 0100h. Including the CP/M stack/BDOS reserve it needs at least {} KiB of installed RAM. The current machine has {} ({} bytes).", path.display(), bytes.len(), minimum_bytes.div_ceil(1024), self.config.machine.ram_size.label(), installed));
            return;
        }

        let bdos_base = bdos_base_usize as u16;
        let Some(environment) = build_cpm_diagnostic_environment(board, port, bdos_base) else {
            self.report_load_error(format!("CPU diagnostic {} cannot start because {} is not available on the installed {}.", path.display(), port.label(board), board.label()));
            return;
        };

        if !self.machine.powered() { self.set_altair_power(true); }
        self.machine.set_running(false);
        self.machine.reset();
        self.asr33.tx_started = None;
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
        self.machine.clear_memory_protection();
        self.machine.clear_transient_memory_guards();
        let clean_ram = vec![0u8; installed];
        self.machine.load_bytes(0x0000, &clean_ram);
        self.machine.load_bytes(0x0000, &environment.page_zero);
        self.machine.load_bytes(CPM_COM_LOAD_ADDRESS, bytes);
        self.machine.load_bytes(environment.bdos_base, &environment.bdos);

        let reference = diagnostic_reference(path);
        self.machine.begin_cpu_diagnostic_meter(
            diagnostic_display_name(path), environment.bdos_base, environment.bdos.len(),
            reference.map(|reference| reference.instructions), reference.map(|reference| reference.t_states),
        );

        match self.serial_router.device_on(connection) {
            Some(SerialDevice::InternalAsr33) => self.asr33.window_open = true,
            Some(SerialDevice::TextTerminal) => self.terminal.window_open = true,
            Some(SerialDevice::ExternalTcp) => self.external_serial.window_open = true,
            Some(SerialDevice::ExternalCom) => self.external_com.window_open = true,
            None => {}
        }

        self.machine.set_running(true);
        let endpoint = self.serial_router.device_on(connection).map(Self::serial_device_name).unwrap_or("no endpoint connected");
        let reference_label = if reference.is_some() { "reference totals armed" } else { "measurement only" };
        self.status = format!(
            "CPU diagnostic running: {} at 0100h — clean reset/RAM — mini-BDOS {:04X}h functions 2/9 — {} — output via {} → {}",
            path.display(), environment.bdos_base, reference_label, port.label(board), endpoint
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_reference_totals_are_registered() {
        assert_eq!(diagnostic_reference(Path::new("TST8080.COM")), Some(DiagnosticReference { instructions: 651, t_states: 4_924 }));
        assert_eq!(diagnostic_reference(Path::new("8080PRE.com")), Some(DiagnosticReference { instructions: 1_061, t_states: 7_817 }));
        assert_eq!(diagnostic_reference(Path::new("CPUTEST.COM")), Some(DiagnosticReference { instructions: 33_971_311, t_states: 255_653_383 }));
        assert_eq!(diagnostic_reference(Path::new("8080EXM.COM")), Some(DiagnosticReference { instructions: 2_919_050_698, t_states: 23_803_381_171 }));
        assert_eq!(diagnostic_reference(Path::new("custom.com")), None);
    }

    #[test]
    fn cpm_vector_exposes_high_bdos_address_for_8080exm() {
        let env = build_cpm_diagnostic_environment(SerialBoard::TwoSio88, DiagnosticSerialPort::Port0, 0xff00).unwrap();
        assert_eq!(&env.page_zero[0..3], &[0xc3, 0x80, 0x00]);
        assert_eq!(&env.page_zero[5..8], &[0xc3, 0x00, 0xff]);
        assert_eq!(&env.page_zero[0x80..0x83], &[0x31, 0x00, 0xff]);
        assert_eq!(&env.page_zero[0x83..0x88], &[0x3e, 0x76, 0x32, 0x00, 0x00]);
        assert_eq!(&env.page_zero[0x88..0x8b], &[0xc3, 0x00, 0x01]);
        assert_eq!(env.bdos_base, 0xff00);
        assert_eq!(env.bdos.len(), 0x37);
    }

    #[test]
    fn high_bdos_branches_are_relocated() {
        let env = build_cpm_diagnostic_environment(SerialBoard::TwoSio88, DiagnosticSerialPort::Port0, 0x7f00).unwrap();
        assert_eq!(&env.bdos[7..10], &[0xca, 0x12, 0x7f]);
        assert_eq!(&env.bdos[12..15], &[0xca, 0x19, 0x7f]);
        assert_eq!(&env.bdos[15..18], &[0xc3, 0x26, 0x7f]);
        assert_eq!(&env.bdos[19..22], &[0xcd, 0x2b, 0x7f]);
    }

    #[test]
    fn putc_uses_88_sio_busy_semantics() {
        let env = build_cpm_diagnostic_environment(SerialBoard::Sio88, DiagnosticSerialPort::Port0, 0x1f00).unwrap();
        assert_eq!(&env.bdos[0x2b..0x37], &[0x47, 0xdb, 0x00, 0xe6, 0xc0, 0xc2, 0x2c, 0x1f, 0x78, 0xd3, 0x01, 0xc9]);
    }

    #[test]
    fn putc_uses_2sio_ready_semantics_on_both_ports() {
        let p0 = build_cpm_diagnostic_environment(SerialBoard::TwoSio88, DiagnosticSerialPort::Port0, 0x7f00).unwrap();
        let p1 = build_cpm_diagnostic_environment(SerialBoard::TwoSio88, DiagnosticSerialPort::Port1, 0x7f00).unwrap();
        assert_eq!(&p0.bdos[0x2b..0x37], &[0x47, 0xdb, 0x10, 0xe6, 0x02, 0xca, 0x2c, 0x7f, 0x78, 0xd3, 0x11, 0xc9]);
        assert_eq!(&p1.bdos[0x2b..0x37], &[0x47, 0xdb, 0x12, 0xe6, 0x02, 0xca, 0x2c, 0x7f, 0x78, 0xd3, 0x13, 0xc9]);
    }

    #[test]
    fn sio_port1_is_rejected() {
        assert!(build_cpm_diagnostic_environment(SerialBoard::Sio88, DiagnosticSerialPort::Port1, 0x1f00).is_none());
    }
}
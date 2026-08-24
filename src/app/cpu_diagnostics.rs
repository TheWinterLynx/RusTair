use super::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};

const CPM_COM_LOAD_ADDRESS: u16 = 0x0100;
const CPM_PAGE_ZERO_SIZE: usize = 0x0100;
const BOOT_ADDRESS: usize = 0x0080;
const CPM_BDOS_PAGE_BYTES: usize = 0x0100;
const CPM_STACK_GUARD_BYTES: usize = 0x0100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum DiagnosticSerialPort {
    Port0,
    Port1,
}

impl DiagnosticSerialPort {
    fn connection(self) -> SerialConnection {
        match self {
            Self::Port0 => SerialConnection::Port0,
            Self::Port1 => SerialConnection::Port1,
        }
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

fn append_abs(code: &mut Vec<u8>, opcode: u8, address: u16) {
    let [lo, hi] = address.to_le_bytes();
    code.extend_from_slice(&[opcode, lo, hi]);
}

fn build_cpm_diagnostic_environment(
    board: SerialBoard,
    port: DiagnosticSerialPort,
    bdos_base: u16,
) -> Option<CpmDiagnosticEnvironment> {
    let (status_port, data_port, ready_mask, wait_branch) = match (board, port) {
        // 88-SIO reports TX busy in bits 6/7. Wait while either is set.
        (SerialBoard::Sio88, DiagnosticSerialPort::Port0) => (0x00, 0x01, 0xc0, 0xc2),
        (SerialBoard::Sio88, DiagnosticSerialPort::Port1) => return None,
        // 88-2SIO reports transmitter ready in bit 1. Wait while it is clear.
        (SerialBoard::TwoSio88, DiagnosticSerialPort::Port0) => (0x10, 0x11, 0x02, 0xca),
        (SerialBoard::TwoSio88, DiagnosticSerialPort::Port1) => (0x12, 0x13, 0x02, 0xca),
    };

    let mut page_zero = [0u8; CPM_PAGE_ZERO_SIZE];

    // CP/M warm-boot vector. The bootstrap replaces byte 0000h with HLT before
    // entering the .COM image, so diagnostics that finish with JMP/CALL 0000h
    // stop the emulated processor cleanly instead of restarting the test.
    page_zero[0x0000..0x0003].copy_from_slice(&[0xc3, 0x80, 0x00]); // JMP 0080h

    // CP/M puts a JMP BDOS at 0005h. Some diagnostics (notably 8080EXM) read
    // bytes 0006h/0007h directly with LHLD 6 and use that high-memory address as
    // their initial stack limit. Therefore this vector must point to a realistic
    // high-memory BDOS entry, not to helper code in page zero.
    let [bdos_lo, bdos_hi] = bdos_base.to_le_bytes();
    page_zero[0x0005..0x0008].copy_from_slice(&[0xc3, bdos_lo, bdos_hi]);

    // Bootstrap at 0080h: establish the same high-water stack used by the BDOS
    // vector, turn the warm-boot vector into HLT, then enter the .COM at 0100h.
    let boot = [
        0x31, bdos_lo, bdos_hi, // LXI SP,bdos_base
        0x3e, 0x76,             // MVI A,HLT
        0x32, 0x00, 0x00,       // STA 0000h
        0xc3, 0x00, 0x01,       // JMP 0100h
    ];
    page_zero[BOOT_ADDRESS..BOOT_ADDRESS + boot.len()].copy_from_slice(&boot);

    // Relocatable mini-BDOS in high memory. Only the functions used by the
    // classic 8080 diagnostics are implemented:
    //   C=2: output character in E
    //   C=9: output '$'-terminated string at DE
    // All guest registers/flags are restored before RET.
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
    append_abs(&mut bdos, 0xca, char_addr); // JZ char
    bdos.extend_from_slice(&[0xfe, 0x09]); // CPI 9
    append_abs(&mut bdos, 0xca, string_addr); // JZ string
    append_abs(&mut bdos, 0xc3, done_addr); // JMP done

    debug_assert_eq!(bdos.len(), CHAR_OFFSET as usize);
    bdos.push(0x7b); // char: MOV A,E
    append_abs(&mut bdos, 0xcd, putc_addr); // CALL putc
    append_abs(&mut bdos, 0xc3, done_addr); // JMP done

    debug_assert_eq!(bdos.len(), STRING_OFFSET as usize);
    bdos.push(0x1a); // string: LDAX D
    bdos.extend_from_slice(&[0xfe, 0x24]); // CPI '$'
    append_abs(&mut bdos, 0xca, done_addr); // JZ done
    append_abs(&mut bdos, 0xcd, putc_addr); // CALL putc
    bdos.push(0x13); // INX D
    append_abs(&mut bdos, 0xc3, string_addr); // JMP string

    debug_assert_eq!(bdos.len(), DONE_OFFSET as usize);
    bdos.extend_from_slice(&[0xe1, 0xd1, 0xc1, 0xf1, 0xc9]); // POP H/D/B/PSW; RET

    // PUTC saves the character in B while polling status in A. The wait branch
    // loops to the IN instruction using a relocated high-memory address.
    debug_assert_eq!(bdos.len(), PUTC_OFFSET as usize);
    bdos.push(0x47); // MOV B,A
    bdos.push(0xdb); // IN status
    bdos.push(status_port);
    bdos.push(0xe6); // ANI ready/busy mask
    bdos.push(ready_mask);
    append_abs(&mut bdos, wait_branch, poll_addr); // JNZ/JZ poll
    bdos.push(0x78); // MOV A,B
    bdos.push(0xd3); // OUT data
    bdos.push(data_port);
    bdos.push(0xc9); // RET

    debug_assert_eq!(bdos.len(), 0x37);

    Some(CpmDiagnosticEnvironment {
        page_zero,
        bdos_base,
        bdos,
    })
}

impl RusTairApp {
    pub(in crate::app) fn start_cpu_diagnostic_dialog(&mut self, port: DiagnosticSerialPort) {
        if self.diagnostic_file_dialog.is_some() {
            self.status = "A CPU diagnostic file dialog is already open".into();
            return;
        }

        if self.config.machine.serial_board == SerialBoard::Sio88
            && port == DiagnosticSerialPort::Port1
        {
            self.report_load_error(
                "CPU diagnostic cannot use Port 1 because the installed MITS 88-SIO only provides Port 0. Select Port 0 or install the MITS 88-2SIO.",
            );
            return;
        }

        // rfd's synchronous Windows picker can interfere with eframe/winit,
        // especially once secondary egui viewports (terminal/ASR) exist. Run
        // the native picker on its own thread and keep the UI/event loop alive.
        // Freeze the guest while the user chooses the next diagnostic so an
        // unlimited-speed test cannot continue mutating machine state behind
        // the dialog. Cancelling restores the previous RUN state.
        let resume_on_cancel = self.machine.running;
        if resume_on_cancel {
            self.machine.set_running(false);
        }

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let selected = rfd::FileDialog::new()
                .add_filter("CP/M 8080 diagnostic", &["com", "bin"])
                .pick_file();
            let _ = sender.send(selected);
        });

        self.diagnostic_file_dialog = Some(DiagnosticFileDialog {
            receiver,
            port,
            resume_on_cancel,
        });
        self.status = "CPU diagnostic paused — choose a .COM file".into();
    }

    pub(in crate::app) fn poll_cpu_diagnostic_dialog(&mut self, ctx: &egui::Context) {
        // This poller runs every frame, making it a convenient common place to
        // render loader failures raised by any binary-loading command.
        self.draw_load_error_dialog(ctx);

        let result = match self.diagnostic_file_dialog.as_ref() {
            Some(dialog) => match dialog.receiver.try_recv() {
                Ok(path) => Some(Ok((path, dialog.port, dialog.resume_on_cancel))),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    None
                }
                Err(TryRecvError::Disconnected) => Some(Err(())),
            },
            None => None,
        };

        let Some(result) = result else {
            return;
        };
        self.diagnostic_file_dialog = None;

        match result {
            Err(()) => {
                self.report_load_error(
                    "The Windows CPU diagnostic file picker terminated unexpectedly before returning a file.",
                );
            }
            Ok((None, _, resume_on_cancel)) => {
                if resume_on_cancel {
                    self.machine.set_running(true);
                }
                self.status = if resume_on_cancel {
                    "CPU diagnostic selection cancelled — previous machine resumed".into()
                } else {
                    "CPU diagnostic selection cancelled".into()
                };
            }
            Ok((Some(path), port, _)) => match std::fs::read(&path) {
                Ok(bytes) => self.load_cpu_diagnostic(&path, &bytes, port),
                Err(e) => self.report_load_error(format!(
                    "Could not read CPU diagnostic {}: {e}",
                    path.display()
                )),
            },
        }
    }

    fn load_cpu_diagnostic(
        &mut self,
        path: &std::path::Path,
        bytes: &[u8],
        port: DiagnosticSerialPort,
    ) {
        if bytes.is_empty() {
            self.report_load_error(format!(
                "CPU diagnostic {} is empty (0 bytes). Nothing was loaded.",
                path.display()
            ));
            return;
        }

        let board = self.config.machine.serial_board;
        let connection = port.connection();
        if board == SerialBoard::Sio88 && port == DiagnosticSerialPort::Port1 {
            self.report_load_error(
                "CPU diagnostic cannot use Port 1 because the installed MITS 88-SIO only provides Port 0.",
            );
            return;
        }

        let installed = self.machine.installed_ram_bytes();
        let image_end = CPM_COM_LOAD_ADDRESS as usize + bytes.len();
        let minimum_bytes = image_end
            .saturating_add(CPM_STACK_GUARD_BYTES)
            .saturating_add(CPM_BDOS_PAGE_BYTES);
        let Some(bdos_base_usize) = installed.checked_sub(CPM_BDOS_PAGE_BYTES) else {
            self.report_load_error(format!(
                "CPU diagnostic {} cannot start because the current {} RAM configuration is too small for a CP/M page-zero and BDOS environment.",
                path.display(),
                self.config.machine.ram_size.label()
            ));
            return;
        };
        let Some(tpa_limit) = bdos_base_usize.checked_sub(CPM_STACK_GUARD_BYTES) else {
            self.report_load_error(format!(
                "CPU diagnostic {} cannot start because the current {} RAM configuration leaves no stack area below BDOS.",
                path.display(),
                self.config.machine.ram_size.label()
            ));
            return;
        };
        if image_end > tpa_limit {
            self.report_load_error(format!(
                "CPU diagnostic {} is {} bytes and loads at 0100h. Including the CP/M stack/BDOS reserve it needs at least {} KiB of installed RAM. The current machine has {} ({} bytes).",
                path.display(),
                bytes.len(),
                minimum_bytes.div_ceil(1024),
                self.config.machine.ram_size.label(),
                installed
            ));
            return;
        }

        let bdos_base = bdos_base_usize as u16;
        let Some(environment) = build_cpm_diagnostic_environment(board, port, bdos_base) else {
            self.report_load_error(format!(
                "CPU diagnostic {} cannot start because {} is not available on the installed {}.",
                path.display(),
                port.label(board),
                board.label()
            ));
            return;
        };

        // This menu item is deliberately a convenience loader, not a physical
        // front-panel operation. Whatever the current state, establish one
        // deterministic diagnostic boot sequence: power on if needed, STOP,
        // RESET CPU/I/O, clear old guest RAM, install CP/M page zero + .COM + a
        // high-memory BDOS, PC=0000, then RUN. A high BDOS address also matches
        // software such as 8080EXM that derives its stack from bytes 0006h/0007h.
        if !self.machine.powered {
            self.set_altair_power(true);
        }
        self.machine.set_running(false);
        self.machine.reset();
        self.asr33.tx_started = None;
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
        self.machine.bus.clear_protection();
        self.machine.bus.clear_transient_memory_guards();
        let clean_ram = vec![0u8; installed];
        self.machine.bus.load(0x0000, &clean_ram);
        self.machine.bus.load(0x0000, &environment.page_zero);
        self.machine.bus.load(CPM_COM_LOAD_ADDRESS, bytes);
        self.machine
            .bus
            .load(environment.bdos_base, &environment.bdos);
        self.machine.cpu.pc = 0x0000;

        // Reveal, but never rewire, whichever endpoint is already connected to
        // the selected physical serial port.
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
            "CPU diagnostic running: {} at 0100h — clean reset/RAM — mini-BDOS {:04X}h functions 2/9 — output via {} → {}",
            path.display(),
            environment.bdos_base,
            port.label(board),
            endpoint
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpm_vector_exposes_high_bdos_address_for_8080exm() {
        let env = build_cpm_diagnostic_environment(
            SerialBoard::TwoSio88,
            DiagnosticSerialPort::Port0,
            0xff00,
        )
        .unwrap();
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
        let env = build_cpm_diagnostic_environment(
            SerialBoard::TwoSio88,
            DiagnosticSerialPort::Port0,
            0x7f00,
        )
        .unwrap();
        assert_eq!(&env.bdos[7..10], &[0xca, 0x12, 0x7f]);
        assert_eq!(&env.bdos[12..15], &[0xca, 0x19, 0x7f]);
        assert_eq!(&env.bdos[15..18], &[0xc3, 0x26, 0x7f]);
        assert_eq!(&env.bdos[19..22], &[0xcd, 0x2b, 0x7f]);
    }

    #[test]
    fn putc_uses_88_sio_busy_semantics() {
        let env = build_cpm_diagnostic_environment(
            SerialBoard::Sio88,
            DiagnosticSerialPort::Port0,
            0x1f00,
        )
        .unwrap();
        assert_eq!(
            &env.bdos[0x2b..0x37],
            &[0x47, 0xdb, 0x00, 0xe6, 0xc0, 0xc2, 0x2c, 0x1f, 0x78, 0xd3, 0x01, 0xc9]
        );
    }

    #[test]
    fn putc_uses_2sio_ready_semantics_on_both_ports() {
        let p0 = build_cpm_diagnostic_environment(
            SerialBoard::TwoSio88,
            DiagnosticSerialPort::Port0,
            0x7f00,
        )
        .unwrap();
        let p1 = build_cpm_diagnostic_environment(
            SerialBoard::TwoSio88,
            DiagnosticSerialPort::Port1,
            0x7f00,
        )
        .unwrap();
        assert_eq!(
            &p0.bdos[0x2b..0x37],
            &[0x47, 0xdb, 0x10, 0xe6, 0x02, 0xca, 0x2c, 0x7f, 0x78, 0xd3, 0x11, 0xc9]
        );
        assert_eq!(
            &p1.bdos[0x2b..0x37],
            &[0x47, 0xdb, 0x12, 0xe6, 0x02, 0xca, 0x2c, 0x7f, 0x78, 0xd3, 0x13, 0xc9]
        );
    }

    #[test]
    fn sio_port1_is_rejected() {
        assert!(build_cpm_diagnostic_environment(
            SerialBoard::Sio88,
            DiagnosticSerialPort::Port1,
            0x1f00,
        )
        .is_none());
    }
}

use super::*;

const CPM_COM_LOAD_ADDRESS: u16 = 0x0100;
const CPM_PAGE_ZERO_SIZE: usize = 0x0100;
const BOOT_ADDRESS: usize = 0x0080;

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

fn build_cpm_diagnostic_shim(
    board: SerialBoard,
    port: DiagnosticSerialPort,
    stack_top: u16,
) -> Option<[u8; CPM_PAGE_ZERO_SIZE]> {
    let (status_port, data_port, ready_mask, wait_branch) = match (board, port) {
        // 88-SIO reports TX busy in bits 6/7. Wait while either is set.
        (SerialBoard::Sio88, DiagnosticSerialPort::Port0) => (0x00, 0x01, 0xc0, 0xc2),
        (SerialBoard::Sio88, DiagnosticSerialPort::Port1) => return None,
        // 88-2SIO reports transmitter ready in bit 1. Wait while it is clear.
        (SerialBoard::TwoSio88, DiagnosticSerialPort::Port0) => (0x10, 0x11, 0x02, 0xca),
        (SerialBoard::TwoSio88, DiagnosticSerialPort::Port1) => (0x12, 0x13, 0x02, 0xca),
    };

    let mut shim = [0u8; CPM_PAGE_ZERO_SIZE];

    // CP/M warm-boot vector. The bootstrap replaces byte 0000h with HLT before
    // entering the .COM image, so diagnostics that finish with JMP/CALL 0000h
    // stop the emulated processor cleanly instead of restarting the test.
    shim[0x0000..0x0003].copy_from_slice(&[0xc3, 0x80, 0x00]); // JMP 0080h

    // CP/M BDOS entry vector.
    shim[0x0005..0x0008].copy_from_slice(&[0xc3, 0x10, 0x00]); // JMP 0010h

    // Mini-BDOS. Only functions used by the classic 8080 diagnostics are
    // implemented:
    //   C=2: output character in E
    //   C=9: output '$'-terminated string at DE
    // All guest registers/flags are restored before RET.
    let bdos: [u8; 43] = [
        0xf5, 0xc5, 0xd5, 0xe5, // PUSH PSW/B/D/H
        0x79,                   // MOV A,C
        0xfe, 0x02,             // CPI 2
        0xca, 0x22, 0x00,       // JZ char
        0xfe, 0x09,             // CPI 9
        0xca, 0x29, 0x00,       // JZ string
        0xc3, 0x36, 0x00,       // JMP done
        0x7b,                   // char: MOV A,E
        0xcd, 0x3b, 0x00,       // CALL putc
        0xc3, 0x36, 0x00,       // JMP done
        0x1a,                   // string: LDAX D
        0xfe, 0x24,             // CPI '$'
        0xca, 0x36, 0x00,       // JZ done
        0xcd, 0x3b, 0x00,       // CALL putc
        0x13,                   // INX D
        0xc3, 0x29, 0x00,       // JMP string
        0xe1, 0xd1, 0xc1, 0xf1, 0xc9, // done: POP H/D/B/PSW; RET
    ];
    shim[0x0010..0x0010 + bdos.len()].copy_from_slice(&bdos);

    // PUTC at 003Bh. Save the character in B while status polling overwrites A.
    shim[0x003b] = 0x47; // MOV B,A
    shim[0x003c] = 0xdb; // IN status
    shim[0x003d] = status_port;
    shim[0x003e] = 0xe6; // ANI ready/busy mask
    shim[0x003f] = ready_mask;
    shim[0x0040] = wait_branch; // JNZ for 88-SIO, JZ for 88-2SIO
    shim[0x0041] = 0x3c;
    shim[0x0042] = 0x00;
    shim[0x0043] = 0x78; // MOV A,B
    shim[0x0044] = 0xd3; // OUT data
    shim[0x0045] = data_port;
    shim[0x0046] = 0xc9; // RET

    // Bootstrap at 0080h: establish a CP/M-like high stack, turn the warm-boot
    // vector into HLT, then enter the .COM program at 0100h.
    let [sp_lo, sp_hi] = stack_top.to_le_bytes();
    let boot = [
        0x31, sp_lo, sp_hi, // LXI SP,stack_top
        0x3e, 0x76,         // MVI A,HLT
        0x32, 0x00, 0x00,   // STA 0000h
        0xc3, 0x00, 0x01,   // JMP 0100h
    ];
    shim[BOOT_ADDRESS..BOOT_ADDRESS + boot.len()].copy_from_slice(&boot);

    Some(shim)
}

impl RusTairApp {
    pub(in crate::app) fn load_cpu_diagnostic_dialog(&mut self, port: DiagnosticSerialPort) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("CP/M 8080 diagnostic", &["com", "bin"])
            .pick_file()
        else {
            return;
        };

        match std::fs::read(&path) {
            Ok(bytes) => self.load_cpu_diagnostic(&path, &bytes, port),
            Err(e) => self.status = format!("CPU diagnostic load failed: {e}"),
        }
    }

    fn load_cpu_diagnostic(
        &mut self,
        path: &std::path::Path,
        bytes: &[u8],
        port: DiagnosticSerialPort,
    ) {
        if bytes.is_empty() {
            self.status = "CPU diagnostic load failed: selected .COM file is empty".into();
            return;
        }

        let board = self.config.machine.serial_board;
        let connection = port.connection();
        if board == SerialBoard::Sio88 && port == DiagnosticSerialPort::Port1 {
            self.status = "CPU diagnostic load failed: 88-SIO has no Port 1".into();
            return;
        }

        let installed = self.machine.installed_ram_bytes();
        let image_end = CPM_COM_LOAD_ADDRESS as usize + bytes.len();
        if image_end > installed {
            self.status = format!(
                "CPU diagnostic load failed: {} needs at least {} KiB RAM; {} is installed",
                path.display(),
                image_end.div_ceil(1024),
                installed / 1024
            );
            return;
        }

        // Leave 256 bytes at the top of installed RAM for a CP/M-like stack.
        let stack_top = installed.saturating_sub(0x100) as u16;
        let Some(shim) = build_cpm_diagnostic_shim(board, port, stack_top) else {
            self.status = "CPU diagnostic load failed: selected serial port is unavailable".into();
            return;
        };

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
        self.machine.bus.load(0x0000, &shim);
        self.machine.bus.load(CPM_COM_LOAD_ADDRESS, bytes);
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
            "CPU diagnostic running: {} at 0100h — mini-BDOS functions 2/9 — output via {} → {}",
            path.display(),
            port.label(board),
            endpoint
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpm_vectors_and_bootstrap_are_installed() {
        let shim = build_cpm_diagnostic_shim(
            SerialBoard::TwoSio88,
            DiagnosticSerialPort::Port0,
            0xff00,
        )
        .unwrap();
        assert_eq!(&shim[0..3], &[0xc3, 0x80, 0x00]);
        assert_eq!(&shim[5..8], &[0xc3, 0x10, 0x00]);
        assert_eq!(&shim[0x80..0x83], &[0x31, 0x00, 0xff]);
        assert_eq!(&shim[0x83..0x88], &[0x3e, 0x76, 0x32, 0x00, 0x00]);
        assert_eq!(&shim[0x88..0x8b], &[0xc3, 0x00, 0x01]);
    }

    #[test]
    fn putc_uses_88_sio_busy_semantics() {
        let shim = build_cpm_diagnostic_shim(
            SerialBoard::Sio88,
            DiagnosticSerialPort::Port0,
            0x1f00,
        )
        .unwrap();
        assert_eq!(&shim[0x3c..0x47], &[0xdb, 0x00, 0xe6, 0xc0, 0xc2, 0x3c, 0x00, 0x78, 0xd3, 0x01, 0xc9]);
    }

    #[test]
    fn putc_uses_2sio_ready_semantics_on_both_ports() {
        let p0 = build_cpm_diagnostic_shim(
            SerialBoard::TwoSio88,
            DiagnosticSerialPort::Port0,
            0x7f00,
        )
        .unwrap();
        let p1 = build_cpm_diagnostic_shim(
            SerialBoard::TwoSio88,
            DiagnosticSerialPort::Port1,
            0x7f00,
        )
        .unwrap();
        assert_eq!(&p0[0x3c..0x47], &[0xdb, 0x10, 0xe6, 0x02, 0xca, 0x3c, 0x00, 0x78, 0xd3, 0x11, 0xc9]);
        assert_eq!(&p1[0x3c..0x47], &[0xdb, 0x12, 0xe6, 0x02, 0xca, 0x3c, 0x00, 0x78, 0xd3, 0x13, 0xc9]);
    }

    #[test]
    fn sio_port1_is_rejected() {
        assert!(build_cpm_diagnostic_shim(
            SerialBoard::Sio88,
            DiagnosticSerialPort::Port1,
            0x1f00,
        )
        .is_none());
    }
}

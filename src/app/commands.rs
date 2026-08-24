#[path = "cpu_diagnostics.rs"]
mod cpu_diagnostics;

use super::*;

impl RusTairApp {
    pub(in crate::app) fn load_cpu_diagnostic_port0_dialog(&mut self) {
        self.load_cpu_diagnostic_dialog(cpu_diagnostics::DiagnosticSerialPort::Port0);
    }

    pub(in crate::app) fn load_cpu_diagnostic_port1_dialog(&mut self) {
        self.load_cpu_diagnostic_dialog(cpu_diagnostics::DiagnosticSerialPort::Port1);
    }

    /// Select and load a raw binary image at address zero.
    pub(in crate::app) fn load_binary_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };

        match std::fs::read(&path) {
            Ok(bytes) => {
                let installed = self.machine.installed_ram_bytes();
                if bytes.len() > installed {
                    self.status = format!(
                        "Load failed: {} bytes require more than the installed {} KiB RAM",
                        bytes.len(),
                        installed / 1024
                    );
                    return;
                }
                self.machine.bus.load(0, &bytes);
                self.status = format!("Loaded {} bytes from {}", bytes.len(), path.display());
            }
            Err(e) => self.status = format!("Load failed: {e}"),
        }
    }

    /// Reset the machine, load the bundled Microsoft 4K BASIC image at address
    /// zero and start execution.
    pub(in crate::app) fn load_bundled_basic(&mut self) {
        match std::fs::read("assets/4kbas32.bin") {
            Ok(bytes) => {
                if bytes.len() > self.machine.installed_ram_bytes() {
                    self.status = format!(
                        "Microsoft 4K BASIC requires at least 4 KiB RAM; {} is installed",
                        self.config.machine.ram_size.label()
                    );
                    return;
                }
                if !self.machine.powered {
                    self.set_altair_power(true);
                }
                // The physical 8800 intentionally powers up without resetting
                // the 8080. This menu command is a convenience loader, so it
                // explicitly performs the reset that a human operator would do.
                self.machine.set_running(false);
                self.machine.reset();
                self.asr33.tx_started = None;
                self.terminal.tx_started = None;
                self.external_serial.reset_line_timing();
                self.external_com.reset_line_timing();
                self.machine.bus.clear_protection();
                self.machine.bus.load(0, &bytes);
                self.machine.cpu.pc = 0;

                // BASIC 3.2's automatic MEMORY SIZE probe wraps FFFFh -> 0000h
                // on a completely writable 64 KiB machine and overwrites itself.
                // Faithful emulation leaves that bug intact by default; the
                // compatibility option is an explicit convenience workaround.
                let full_memory_probe_guard = if self
                    .config
                    .compatibility
                    .basic32_64k_probe_workaround
                {
                    self.machine.arm_basic32_full_memory_probe_guard()
                } else {
                    false
                };

                // Auto-open only reveals the endpoint already wired to Port 0.
                // It never changes the serial cabling.
                if self.config.preferences.auto_open_basic_console {
                    match self.serial_router.device_on(SerialConnection::Port0) {
                        Some(SerialDevice::InternalAsr33) => self.asr33.window_open = true,
                        Some(SerialDevice::TextTerminal) => self.terminal.window_open = true,
                        Some(SerialDevice::ExternalTcp) => self.external_serial.window_open = true,
                        Some(SerialDevice::ExternalCom) => self.external_com.window_open = true,
                        None => {}
                    }
                }

                self.machine.set_running(true);
                self.status = if full_memory_probe_guard {
                    "Microsoft 4K BASIC loaded and running — optional 64 KiB probe workaround active"
                        .into()
                } else if self.machine.installed_ram_bytes() == 64 * 1024 {
                    "Microsoft 4K BASIC loaded and running — authentic 64 KiB probe bug enabled"
                        .into()
                } else if self.config.preferences.auto_open_basic_console {
                    "Microsoft 4K BASIC loaded and running — console auto-open enabled".into()
                } else {
                    "Microsoft 4K BASIC loaded and running — console auto-open disabled".into()
                };
            }
            Err(e) => self.status = format!("4K BASIC asset missing: {e}"),
        }
    }

    pub(in crate::app) fn load_paper_tape(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Paper tape", &["txt", "tap", "bin"])
            .pick_file()
        else {
            return;
        };

        match std::fs::read(&path) {
            Ok(bytes) => {
                self.tty.load_tape(&bytes);
                self.status = format!("Paper tape loaded: {} bytes", bytes.len());
            }
            Err(e) => self.status = format!("Paper tape load failed: {e}"),
        }
    }

    pub(in crate::app) fn save_punched_tape(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("myPaperTape.txt")
            .save_file()
        else {
            return;
        };

        let tape = self.tty.punched_tape();
        match std::fs::write(&path, tape) {
            Ok(_) => self.status = format!("Punched tape saved: {} bytes", tape.len()),
            Err(e) => self.status = format!("Paper tape save failed: {e}"),
        }
    }

    pub(in crate::app) fn load_terminal_text_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Text / BASIC", &["txt", "bas", "basic"])
            .pick_file()
        else {
            return;
        };

        match std::fs::read(&path) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                let count = self.terminal_enqueue_text(&text, true);
                if count > 0 {
                    self.status = format!(
                        "Terminal queued {count} bytes from {} at {}",
                        path.display(),
                        self.terminal.speed.label()
                    );
                }
            }
            Err(e) => self.status = format!("Terminal file load failed: {e}"),
        }
    }
}
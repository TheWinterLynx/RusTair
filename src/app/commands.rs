use super::*;

const LOAD_ERROR_PREFIX: &str = "LOAD ERROR: ";

impl RusTairApp {
    /// Record a loader failure in the status bar and make it impossible to miss
    /// by presenting the same reason in a centered acknowledgement dialog.
    pub(in crate::app) fn report_load_error(&mut self, reason: impl Into<String>) {
        self.status = format!("{LOAD_ERROR_PREFIX}{}", reason.into());
    }

    /// Draw the shared loader-error dialog. The diagnostic poller calls this on
    /// every frame, so failures from the raw binary, BASIC and CP/M diagnostic
    /// loaders all use the same visible reporting path.
    pub(in crate::app) fn draw_load_error_dialog(&mut self, ctx: &egui::Context) {
        let Some(reason) = self.status.strip_prefix(LOAD_ERROR_PREFIX).map(str::to_owned) else {
            return;
        };

        let mut dismissed = false;
        egui::Window::new("Load failed")
            .id(egui::Id::new("rustair-load-error-dialog"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label(&reason);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        dismissed = true;
                    }
                });
            });

        if dismissed {
            self.status = format!("Load failed: {reason}");
        }
    }

    /// Select and load a raw binary image at address zero.
    pub(in crate::app) fn load_binary_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_file() else { return; };

        match std::fs::read(&path) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    self.report_load_error(format!(
                        "Binary {} is empty (0 bytes). Nothing was loaded.",
                        path.display()
                    ));
                    return;
                }

                let hardware = self.config.machine.s100_hardware;
                let usable = hardware.unique_ram_prefix_bytes();
                let installed = hardware.installed_ram_bytes();
                if bytes.len() > usable {
                    self.report_load_error(format!(
                        "Binary {} is {} bytes and loads at 0000h, so 0000h through {:04X}h must each have exactly one S-100 RAM responder. The current chassis provides {} contiguous uniquely mapped bytes from 0000h ({} bytes installed across all RAM cards).",
                        path.display(),
                        bytes.len(),
                        bytes.len() - 1,
                        usable,
                        installed
                    ));
                    return;
                }
                self.machine.load_bytes(0, &bytes);
                self.status = format!("Loaded {} bytes from {}", bytes.len(), path.display());
            }
            Err(e) => self.report_load_error(format!(
                "Could not read binary {}: {e}",
                path.display()
            )),
        }
    }

    /// Reset the machine, load the bundled Microsoft 4K BASIC image at address
    /// zero and start execution. The BASIC image is compiled into the executable.
    pub(in crate::app) fn load_bundled_basic(&mut self) {
        let Some(bytes) = crate::embedded_assets::get("assets/4kbas32.bin") else {
            self.report_load_error("Bundled Microsoft 4K BASIC is missing from the executable.");
            return;
        };

        let hardware = self.config.machine.s100_hardware;
        let usable = hardware.unique_ram_prefix_bytes();
        if usable < 4 * 1024 {
            self.report_load_error(format!(
                "Microsoft 4K BASIC requires a uniquely mapped S-100 RAM window from 0000h through 0FFFh. The current chassis is uniquely mapped only through {} byte(s) from 0000h ({} bytes installed across RAM cards).",
                usable,
                hardware.installed_ram_bytes()
            ));
            return;
        }
        if !self.machine.powered() {
            self.set_altair_power(true);
        }
        self.machine.set_running(false);
        self.machine.reset();
        self.asr33.tx_started = None;
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
        self.machine.clear_memory_protection();
        self.machine.load_bytes(0, bytes);

        let full_memory_probe_guard = if self.config.compatibility.basic32_64k_probe_workaround {
            self.machine.arm_basic32_full_memory_probe_guard()
        } else {
            false
        };

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
            "Microsoft 4K BASIC loaded and running — optional 64 KiB probe workaround active".into()
        } else if hardware.unique_ram_prefix_bytes() == 64 * 1024 {
            "Microsoft 4K BASIC loaded and running — authentic 64 KiB probe bug enabled".into()
        } else if self.config.preferences.auto_open_basic_console {
            "Microsoft 4K BASIC loaded and running — console auto-open enabled".into()
        } else {
            "Microsoft 4K BASIC loaded and running — console auto-open disabled".into()
        };
    }

    /// Put a paper tape in the reader. Loading media does not start it: the
    /// operator must explicitly press Read after the Altair is powered/RUNning.
    pub(in crate::app) fn load_paper_tape(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Paper tape", &["txt", "tap", "bin"])
            .pick_file()
        else {
            return;
        };

        match std::fs::read(&path) {
            Ok(bytes) => {
                self.asr33.reader_running = false;
                self.asr33.last_reader_byte = None;
                self.tty.load_tape(&bytes);
                self.asr33.last_reader_tick = Instant::now();
                self.audio.play_once("assets/click.mp3");
                self.status = format!(
                    "Paper tape mounted: {} bytes from {} — press Read to start",
                    bytes.len(),
                    path.display()
                );
            }
            Err(e) => self.status = format!("Paper tape load failed: {e}"),
        }
    }

    /// Save a completed punched tape. Return true only when bytes reached disk;
    /// cancellation/error deliberately leaves the virtual tape intact so the
    /// operator can retry rather than losing the physical-media equivalent.
    pub(in crate::app) fn save_punched_tape(&mut self) -> bool {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("myPaperTape.tap")
            .save_file()
        else {
            self.status = format!(
                "Punch save cancelled — {} bytes remain in the virtual tape",
                self.tty.punched_tape_len()
            );
            return false;
        };

        let tape = self.tty.punched_tape();
        let len = tape.len();
        match std::fs::write(&path, tape) {
            Ok(_) => {
                self.status = format!("Punched tape saved: {len} bytes to {}", path.display());
                self.tty.eject_punched_tape();
                true
            }
            Err(e) => {
                self.status = format!("Paper tape save failed: {e} — tape retained for retry");
                false
            }
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

use super::*;

impl RusTairApp {
    /// Select and load a raw binary image at address zero.
    pub(in crate::app) fn load_binary_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };

        match std::fs::read(&path) {
            Ok(bytes) => {
                self.machine.bus.load(0, &bytes);
                self.status = format!("Loaded {} bytes from {}", bytes.len(), path.display());
            }
            Err(e) => self.status = format!("Load failed: {e}"),
        }
    }

    /// Reset the machine into the same state as the existing bundled BASIC
    /// command, then load and run Microsoft 4K BASIC from address zero.
    pub(in crate::app) fn load_bundled_basic(&mut self) {
        match std::fs::read("assets/4kbas32.bin") {
            Ok(bytes) => {
                if !self.machine.powered {
                    self.set_altair_power(true);
                } else {
                    self.machine.set_running(false);
                    self.machine.reset();
                }
                self.asr33.tx_started = None;
                self.machine.bus.clear_protection();
                self.machine.bus.load(0, &bytes);
                self.machine.cpu.pc = 0;
                self.asr33.window_open = true;
                self.machine.set_running(true);
                self.status = "Microsoft 4K BASIC loaded and running".into();
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

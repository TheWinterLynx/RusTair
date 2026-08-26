use super::*;

/// Front-panel bootstrap for Microsoft 4K BASIC 3.2 paper tape.
///
/// The loader bytes are the MITS front-panel bootstrap, not a RusTair helper
/// program. BASIC 3.2 uses leader/checksum-loader marker 256 octal (AEh); 4K
/// uses checksum-loader selector 017 octal. The 88-2SIO variant below uses the
/// historically appropriate two-stop-bit ACIA setup for an ASR-33.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BootstrapDefinition {
    pub(super) board: SerialBoard,
    pub(super) name: &'static str,
    pub(super) bytes: &'static [u8],
    pub(super) required_sense: u8,
    pub(super) status_port: u8,
    pub(super) data_port: u8,
}

const BASIC32_4K_88_SIO: [u8; 20] = [
    0x21, 0xAE, 0x0F, 0x31, 0x12, 0x00, 0xDB, 0x00, 0x0F, 0xD8,
    0xDB, 0x01, 0xBD, 0xC8, 0x2D, 0x77, 0xC0, 0xE9, 0x03, 0x00,
];

const BASIC32_4K_88_2SIO: [u8; 28] = [
    0x3E, 0x03, 0xD3, 0x10, 0x3E, 0x11, 0xD3, 0x10, 0x21, 0xAE,
    0x0F, 0x31, 0x1A, 0x00, 0xDB, 0x10, 0x0F, 0xD0, 0xDB, 0x11,
    0xBD, 0xC8, 0x2D, 0x77, 0xC0, 0xE9, 0x0B, 0x00,
];

impl BootstrapDefinition {
    pub(super) const fn for_board(board: SerialBoard) -> Self {
        match board {
            SerialBoard::Sio88 => Self {
                board,
                name: "Microsoft 4K BASIC 3.2 — MITS 88-SIO rev. 1 bootstrap",
                bytes: &BASIC32_4K_88_SIO,
                // BASIC 3.X uses the same selected device for loading and console.
                // SIO A/B/C (not rev. 0): all A15..A8 down.
                required_sense: 0x00,
                status_port: 0x00,
                data_port: 0x01,
            },
            SerialBoard::TwoSio88 => Self {
                board,
                name: "Microsoft 4K BASIC 3.2 — MITS 88-2SIO Port 0 bootstrap",
                bytes: &BASIC32_4K_88_2SIO,
                // BASIC 3.X, 88-2SIO Port 0, two stop bits: A11 up, all other
                // A15..A8 switches down. This is the normal ASR-33 setting.
                required_sense: 0x08,
                status_port: 0x10,
                data_port: 0x11,
            },
        }
    }

    const fn last_address(self) -> u16 {
        self.bytes.len() as u16 - 1
    }
}

#[derive(Default)]
pub(super) struct AuthenticLoaderState {
    pub(super) window_open: bool,
    pub(super) last_install_log: Vec<String>,
}

fn bootstrap_matches(machine: &mut BackendHost, definition: BootstrapDefinition) -> bool {
    definition
        .bytes
        .iter()
        .enumerate()
        .all(|(address, expected)| machine.peek_memory(address as u16) == Some(*expected))
}

/// Perform the exact front-panel entry sequence described by MITS: EXAMINE 0000,
/// DEPOSIT the first byte, then DEPOSIT NEXT for every following byte. This is
/// intentionally *not* `load_bytes`; the selected CPU backend and S-100/front
/// panel path perform every write.
fn install_via_front_panel(
    machine: &mut BackendHost,
    definition: BootstrapDefinition,
) -> Result<Vec<String>, String> {
    if !machine.powered() {
        return Err("Power ON the Altair before installing the bootstrap.".into());
    }
    if machine.running() {
        return Err("STOP the Altair before installing the bootstrap.".into());
    }
    if machine.installed_ram_bytes() < definition.bytes.len() {
        return Err(format!(
            "The installed RAM is too small for the {}-byte bootstrap.",
            definition.bytes.len()
        ));
    }

    machine.front_panel_reset();
    machine.set_switch_register(0x0000);
    machine.examine(false);
    if machine.front_panel_state().address != 0 {
        return Err("EXAMINE 0000h did not place the front panel at address 0000h.".into());
    }

    let mut log = Vec::with_capacity(definition.bytes.len());
    for (index, byte) in definition.bytes.iter().copied().enumerate() {
        // MITS specifies all high address/sense switches DOWN during data entry;
        // only the low eight data switches are changed for each byte.
        machine.set_switch_register(u16::from(byte));
        machine.deposit(index != 0);

        let address = index as u16;
        let observed = machine.peek_memory(address);
        if observed != Some(byte) {
            return Err(format!(
                "Front-panel deposit failed at {address:04X}h: entered {byte:02X}h, read back {}.",
                observed
                    .map(|value| format!("{value:02X}h"))
                    .unwrap_or_else(|| "unmapped memory".into())
            ));
        }
        log.push(format!(
            "{address:04X}h / {address:03o}o  ←  {byte:02X}h / {byte:03o}o"
        ));
    }

    Ok(log)
}

impl RusTairApp {
    pub(in crate::app) fn open_authentic_basic_loader(&mut self) {
        self.authentic_loader.window_open = true;
        self.status = "Authentic BASIC 3.2 loader opened — BASIC will not be copied directly into RAM".into();
    }

    fn arm_authentic_tape_reader(&mut self) -> Result<(), String> {
        if self.tty.tape_input_total_len() == 0 {
            return Err("Mount a BASIC 3.2 paper-tape image first.".into());
        }
        if self.tty.mode != TtyMode::Line {
            return Err("Set the ASR-33 to LINE before starting the reader.".into());
        }
        if self.asr_connection() != SerialConnection::Port0 {
            return Err("Connect the ASR-33 to Port 0; the historical bootstrap reads the board's first port.".into());
        }
        if !self.machine.powered() {
            return Err("Power ON the Altair before starting the reader.".into());
        }

        self.asr33.reader_running = true;
        self.asr33.last_reader_tick = Instant::now()
            .checked_sub(self.asr33.reader_speed.char_time())
            .unwrap_or_else(Instant::now);
        self.audio.play_once("assets/click.mp3");
        Ok(())
    }

    fn authentic_stage_label(
        &mut self,
        definition: BootstrapDefinition,
        verified: bool,
        tape_position: usize,
    ) -> String {
        if !verified {
            return "Bootstrap not verified in RAM".into();
        }
        let cpu = self.machine.intel8080_state();
        if !self.machine.running() {
            return "Bootstrap verified · CPU stopped".into();
        }
        if cpu.pc <= definition.last_address().saturating_add(1) {
            return format!("Bootstrap executing · PC {:04X}h", cpu.pc);
        }
        if (0x0F00..=0x0FFF).contains(&cpu.pc) {
            return format!("Checksum loader executing · PC {:04X}h", cpu.pc);
        }
        if tape_position > 0 {
            return format!("Tape/program load in progress · PC {:04X}h", cpu.pc);
        }
        format!("CPU running · PC {:04X}h", cpu.pc)
    }

    pub(in crate::app) fn draw_authentic_loader_window(&mut self, ctx: &egui::Context) {
        if !self.authentic_loader.window_open {
            return;
        }

        let mut open = self.authentic_loader.window_open;
        let definition = BootstrapDefinition::for_board(self.config.machine.serial_board);

        egui::Window::new("Authentic Microsoft 4K BASIC 3.2 Load")
            .id(egui::Id::new("rustair-authentic-basic-loader"))
            .open(&mut open)
            .default_width(700.0)
            .resizable(true)
            .show(ctx, |ui| {
                let panel = self.machine.front_panel_state();
                let sense = (panel.switches >> 8) as u8;
                let bootstrap_verified = bootstrap_matches(&mut self.machine, definition);
                let tape_total = self.tty.tape_input_total_len();
                let tape_position = self.tty.tape_input_position();
                let asr_port_ok = self.asr_connection() == SerialConnection::Port0;
                let line_ok = self.tty.mode == TtyMode::Line;
                let sense_ok = sense == definition.required_sense;
                let stage = self.authentic_stage_label(definition, bootstrap_verified, tape_position);

                ui.strong(definition.name);
                ui.small("Authentic path: the bootstrap executes on the emulated 8080 and consumes the mounted tape through the selected UART. No BASIC bytes are injected directly into RAM.");
                ui.add_space(6.0);

                egui::Grid::new("authentic-basic-status")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Serial board");
                        ui.label(format!(
                            "{} · status {:02X}h / data {:02X}h",
                            definition.board.label(), definition.status_port, definition.data_port
                        ));
                        ui.end_row();

                        ui.label("ASR-33 cable");
                        ui.colored_label(
                            if asr_port_ok { Color32::LIGHT_GREEN } else { Color32::LIGHT_RED },
                            if asr_port_ok {
                                "Port 0 — correct"
                            } else {
                                "Must be connected to Port 0"
                            },
                        );
                        ui.end_row();

                        ui.label("ASR-33 mode");
                        ui.colored_label(
                            if line_ok { Color32::LIGHT_GREEN } else { Color32::LIGHT_RED },
                            if line_ok { "LINE" } else { "Set to LINE" },
                        );
                        ui.end_row();

                        ui.label("Sense switches A15..A8");
                        ui.colored_label(
                            if sense_ok { Color32::LIGHT_GREEN } else { Color32::YELLOW },
                            format!(
                                "current {sense:02X}h · required {:02X}h",
                                definition.required_sense
                            ),
                        );
                        ui.end_row();

                        ui.label("Bootstrap RAM");
                        ui.colored_label(
                            if bootstrap_verified { Color32::LIGHT_GREEN } else { Color32::YELLOW },
                            if bootstrap_verified {
                                format!("verified · {} bytes", definition.bytes.len())
                            } else {
                                "not installed / does not match".into()
                            },
                        );
                        ui.end_row();

                        ui.label("Paper tape");
                        if tape_total == 0 {
                            ui.colored_label(Color32::YELLOW, "not mounted");
                        } else {
                            let percent = 100.0 * tape_position as f32 / tape_total.max(1) as f32;
                            ui.label(format!(
                                "{tape_position}/{tape_total} bytes ({percent:.1}%) · {}",
                                self.asr33.reader_speed.label()
                            ));
                        }
                        ui.end_row();

                        ui.label("Reader");
                        ui.label(if self.asr33.reader_running {
                            if self.machine.running() { "READING / guest-paced" } else { "ARMED · waiting for RUN" }
                        } else {
                            "stopped"
                        });
                        ui.end_row();

                        ui.label("Stage");
                        ui.label(stage);
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Put BASIC 3.2 tape…").clicked() {
                        self.load_paper_tape();
                    }
                    if ui.button("Open ASR-33").clicked() {
                        self.asr33.window_open = true;
                    }
                    if ui.button("Set ASR-33 LINE").clicked() {
                        self.set_tty_mode(TtyMode::Line);
                    }
                    if ui.button("Install bootstrap via front panel").clicked() {
                        match install_via_front_panel(&mut self.machine, definition) {
                            Ok(log) => {
                                self.authentic_loader.last_install_log = log;
                                self.status = format!(
                                    "Authentic bootstrap installed via EXAMINE/DEPOSIT: {} bytes — now EXAMINE 0000h and set sense {:02X}h",
                                    definition.bytes.len(), definition.required_sense
                                );
                            }
                            Err(error) => self.report_load_error(error),
                        }
                    }
                    if ui.button("Arm / start paper reader").clicked() {
                        match self.arm_authentic_tape_reader() {
                            Ok(()) => {
                                self.status = if self.machine.running() {
                                    format!("ASR-33 paper reader started — {}", self.asr33.reader_speed.label())
                                } else {
                                    "ASR-33 paper reader armed — it will not advance until the Altair RUN latch is on".into()
                                };
                            }
                            Err(error) => self.report_load_error(error),
                        }
                    }
                });

                ui.small("Install bootstrap is an assisted front-panel operation: it performs RESET, EXAMINE 0000, DEPOSIT, then DEPOSIT NEXT for every byte and verifies every write. It deliberately leaves the sense switches for you to set before RUN.");

                ui.separator();
                ui.strong("Manual front-panel procedure");
                ui.label("1. Power ON, STOP the machine, RESET, set the ASR-33 to LINE and connect it to Port 0.");
                ui.label("2. Put all 16 switches DOWN and operate EXAMINE. Enter the first byte with switches A7..A0 and DEPOSIT; enter each following byte with DEPOSIT NEXT.");
                ui.label("3. Verify the loader if desired, then put all switches DOWN and EXAMINE 0000h again.");
                ui.label(format!(
                    "4. Set A15..A8 to {:02X}h ({}) before loading BASIC 3.2.",
                    definition.required_sense,
                    match definition.board {
                        SerialBoard::Sio88 => "all sense switches down for 88-SIO rev. 1",
                        SerialBoard::TwoSio88 => "A11 up for 88-2SIO Port 0 with the ASR-33 two-stop-bit setting",
                    }
                ));
                ui.label(match definition.board {
                    SerialBoard::Sio88 => "5. Historical SIO sequence: start/arm the paper reader, then operate RUN. RusTair will keep the tape stationary until RUN is actually active.",
                    SerialBoard::TwoSio88 => "5. Historical 2SIO sequence: operate RUN, then start the paper reader.",
                });
                ui.label("6. The reader advances only when the guest UART can accept another byte. WAIT GUEST RX therefore means the bootstrap/checksum loader has not yet consumed the previous byte with a real IN instruction.");

                ui.collapsing("Exact bootstrap bytes (front-panel octal table)", |ui| {
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        egui::Grid::new("authentic-bootstrap-byte-table")
                            .num_columns(4)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Address");
                                ui.strong("Data");
                                ui.strong("Hex");
                                ui.strong("Entered by");
                                ui.end_row();
                                for (index, byte) in definition.bytes.iter().copied().enumerate() {
                                    let address = index as u16;
                                    ui.monospace(format!("{address:03o}"));
                                    ui.monospace(format!("{byte:03o}"));
                                    ui.monospace(format!("{byte:02X}"));
                                    ui.label(if index == 0 { "DEPOSIT" } else { "DEPOSIT NEXT" });
                                    ui.end_row();
                                }
                            });
                    });
                });

                if !self.authentic_loader.last_install_log.is_empty() {
                    ui.collapsing("Last assisted deposit log", |ui| {
                        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                            for line in &self.authentic_loader.last_install_log {
                                ui.monospace(line);
                            }
                        });
                    });
                }
            });

        self.authentic_loader.window_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic32_4k_bootstraps_keep_historical_leader_and_ports() {
        let sio = BootstrapDefinition::for_board(SerialBoard::Sio88);
        assert_eq!(sio.bytes.len(), 20);
        assert_eq!(&sio.bytes[0..3], &[0x21, 0xAE, 0x0F]);
        assert!(sio.bytes.windows(2).any(|bytes| bytes == [0xDB, 0x00]));
        assert!(sio.bytes.windows(2).any(|bytes| bytes == [0xDB, 0x01]));
        assert_eq!(sio.required_sense, 0x00);

        let two_sio = BootstrapDefinition::for_board(SerialBoard::TwoSio88);
        assert_eq!(two_sio.bytes.len(), 28);
        assert_eq!(&two_sio.bytes[0..8], &[0x3E, 0x03, 0xD3, 0x10, 0x3E, 0x11, 0xD3, 0x10]);
        assert_eq!(&two_sio.bytes[8..11], &[0x21, 0xAE, 0x0F]);
        assert!(two_sio.bytes.windows(2).any(|bytes| bytes == [0xDB, 0x10]));
        assert!(two_sio.bytes.windows(2).any(|bytes| bytes == [0xDB, 0x11]));
        assert_eq!(two_sio.required_sense, 0x08);
    }

    #[test]
    fn assisted_bootstrap_really_uses_front_panel_on_both_rust_engines() {
        for engine in [
            EmulationEngine::RustFast8080,
            EmulationEngine::RustCycleAccurate8080,
        ] {
            for board in [SerialBoard::Sio88, SerialBoard::TwoSio88] {
                let mut machine = BackendHost::from_engine(engine).unwrap();
                machine.configure_memory(RamSize::K1, RamInit::Zeroed);
                machine.configure_serial_board(board);
                machine.power(true);
                machine.set_running(false);

                let definition = BootstrapDefinition::for_board(board);
                let log = install_via_front_panel(&mut machine, definition).unwrap();
                assert_eq!(log.len(), definition.bytes.len());
                assert!(bootstrap_matches(&mut machine, definition));
                assert_eq!(machine.front_panel_state().address, definition.last_address());
                assert_eq!(machine.switch_register() & 0x00FF, 0x00);
            }
        }
    }
}

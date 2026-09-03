use super::super::*;
use super::s100_memory_inspection::{mapping_detail, mapping_summary};

#[derive(Clone)]
struct FrontPanelOperatorUiState {
    open: bool,
    source_name: String,
    bytes: Vec<u8>,
    base_address: u16,
    base_text: String,
}

impl Default for FrontPanelOperatorUiState {
    fn default() -> Self {
        Self {
            open: false,
            source_name: String::new(),
            bytes: Vec::new(),
            base_address: 0,
            base_text: "0000".into(),
        }
    }
}

impl RusTairApp {
    fn standalone_operator_state(ctx: &egui::Context) -> FrontPanelOperatorUiState {
        ctx.data(|data| {
            data.get_temp::<FrontPanelOperatorUiState>(egui::Id::new(
                "rustair-standalone-front-panel-operator-state",
            ))
            .unwrap_or_default()
        })
    }

    fn store_standalone_operator_state(
        ctx: &egui::Context,
        state: FrontPanelOperatorUiState,
    ) {
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("rustair-standalone-front-panel-operator-state"),
                state,
            );
        });
    }

    pub(in crate::app) fn open_standalone_front_panel_operator(
        &mut self,
        ctx: &egui::Context,
    ) {
        let mut state = Self::standalone_operator_state(ctx);
        state.open = true;
        Self::store_standalone_operator_state(ctx, state);
        self.status = "Front Panel Operator opened — load an assembled binary or tape image and operate the Altair row by row".into();
    }

    fn strip_operator_hex_affixes(text: &str) -> &str {
        let trimmed = text.trim();
        let trimmed = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);
        trimmed
            .strip_suffix('h')
            .or_else(|| trimmed.strip_suffix('H'))
            .unwrap_or(trimmed)
    }

    fn parse_operator_hex_address(text: &str) -> Option<u16> {
        let trimmed = Self::strip_operator_hex_affixes(text);
        (!trimmed.is_empty())
            .then(|| u16::from_str_radix(trimmed, 16).ok())
            .flatten()
    }

    fn standalone_operator_target_address(base: u16, index: usize) -> Option<u16> {
        base.checked_add(u16::try_from(index).ok()?)
    }

    fn load_standalone_operator_source(&mut self, state: &mut FrontPanelOperatorUiState) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Assembled binary / paper tape", &["bin", "rom", "com", "tap"])
            .pick_file()
        else {
            return;
        };

        match std::fs::read(&path) {
            Ok(bytes) if bytes.is_empty() => self.report_load_error(format!(
                "{} is empty; there are no bytes for the front-panel operator.",
                path.display()
            )),
            Ok(bytes) => {
                state.source_name = path.display().to_string();
                state.bytes = bytes;
                if path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("com"))
                {
                    state.base_address = 0x0100;
                    state.base_text = "0100".into();
                }
                self.status = format!(
                    "Front Panel Operator loaded {} bytes from {}{}",
                    state.bytes.len(),
                    path.display(),
                    if state.base_address == 0x0100
                        && path
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("com"))
                    {
                        " — CP/M .COM base set to 0100h"
                    } else {
                        ""
                    }
                );
            }
            Err(error) => self.report_load_error(format!(
                "Could not read operator source {}: {error}",
                path.display()
            )),
        }
    }

    fn standalone_operator_configure_switches(&mut self, value: u16, description: &str) {
        self.machine.set_switch_register(value);
        self.status = format!(
            "Front Panel Operator: A15..A0 = {value:04X}h — {description}; no panel operation executed yet"
        );
    }

    fn standalone_operator_require_stopped(&mut self) -> Result<(), String> {
        if !self.machine.powered() {
            return Err("Power ON the Altair before using the Front Panel Operator.".into());
        }
        if self.machine.running() {
            return Err("STOP the Altair before using EXAMINE / DEPOSIT from the Front Panel Operator.".into());
        }
        Ok(())
    }

    fn standalone_operator_execute_examine(&mut self, address: u16) -> Result<(), String> {
        self.standalone_operator_require_stopped()?;
        let switches = self.machine.switch_register();
        if switches != address {
            return Err(format!(
                "A15..A0 are {switches:04X}h, but EXAMINE requires {address:04X}h. Press Config switches first."
            ));
        }
        self.machine.examine(false);
        let actual = self.machine.front_panel_state().address;
        if actual != address {
            return Err(format!(
                "EXAMINE expected {address:04X}h but the front panel selected {actual:04X}h."
            ));
        }
        self.audio.play_once("assets/click.mp3");
        let mapping = self.machine.inspect_memory_mapping(address);
        self.status = format!(
            "Front Panel Operator: EXAMINE selected {address:04X}h through the real panel path — {}",
            mapping_summary(&mapping),
        );
        Ok(())
    }

    fn standalone_operator_execute_deposit(
        &mut self,
        address: u16,
        byte: u8,
        deposit_next: bool,
    ) -> Result<(), String> {
        self.standalone_operator_require_stopped()?;

        let switches = self.machine.switch_register();
        if switches != u16::from(byte) {
            return Err(format!(
                "A15..A0 are {switches:04X}h, but this row requires data {byte:02X}h with A15..A8 DOWN. Press Config switches first."
            ));
        }

        let required_before = if deposit_next {
            address.wrapping_sub(1)
        } else {
            address
        };
        let current_address = self.machine.front_panel_state().address;
        if current_address != required_before {
            return Err(format!(
                "{} for {address:04X}h requires the panel to be at {required_before:04X}h first; it is at {current_address:04X}h. Follow the rows in order.",
                if deposit_next { "DEPOSIT NEXT" } else { "DEPOSIT" }
            ));
        }

        // Do not pre-validate the address with a host-side RAM-size shortcut.
        // A real Altair operator can assert DEPOSIT on an unmapped or overlapped
        // address too. Let the physical S-100 transaction happen, then inspect
        // which cards actually decoded it.
        self.machine.deposit(deposit_next);
        let inspection = self.machine.inspect_memory_mapping(address);
        let operation = if deposit_next { "DEPOSIT NEXT" } else { "DEPOSIT" };

        match inspection.drivers.as_slice() {
            [] => {
                return Err(format!(
                    "{operation} bus cycle executed at {address:04X}h with {byte:02X}h, but no RAM card decoded the address — {}.",
                    mapping_summary(&inspection),
                ));
            }
            [driver] if driver.value != byte => {
                return Err(format!(
                    "{operation} bus cycle executed at {address:04X}h with {byte:02X}h, but Slot {:02} now contains {:02X}h{}.",
                    driver.slot,
                    driver.value,
                    if driver.protected { " (card/protection state blocked the write)" } else { "" },
                ));
            }
            [driver] => {
                self.audio.play_once("assets/click.mp3");
                self.status = format!(
                    "Front Panel Operator: {operation} stored {byte:02X}h at {address:04X}h in Slot {:02}",
                    driver.slot,
                );
                Ok(())
            }
            _ => Err(format!(
                "{operation} bus cycle executed at {address:04X}h with {byte:02X}h, but multiple RAM cards decode that address. The operator did not choose one card: {}",
                mapping_detail(address, &inspection),
            )),
        }
    }

    fn standalone_switch_tooltip(value: u16, role: &str) -> String {
        format!(
            "Set the real Altair A15..A0 switches to {value:04X}h / {value:06o}o.\nBinary: {:04b} {:04b} {:04b} {:04b}\n\n{role}\n\nConfig switches only moves the front-panel switches. It does not write memory or execute an 8080 instruction.",
            (value >> 12) & 0x0f,
            (value >> 8) & 0x0f,
            (value >> 4) & 0x0f,
            value & 0x0f,
        )
    }

    fn draw_standalone_front_panel_operator(
        &mut self,
        ctx: &egui::Context,
        state: &mut FrontPanelOperatorUiState,
    ) {
        egui::TopBottomPanel::top("standalone-front-panel-operator-toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Load assembled binary / tape…").clicked() {
                    self.load_standalone_operator_source(state);
                }
                ui.separator();
                ui.label("Base address:");
                let response = ui.add_sized(
                    [88.0, 24.0],
                    egui::TextEdit::singleline(&mut state.base_text)
                        .font(egui::TextStyle::Monospace),
                );
                let enter = response.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ui.button("Apply").clicked() || enter {
                    match Self::parse_operator_hex_address(&state.base_text) {
                        Some(address) => {
                            state.base_address = address;
                            state.base_text = format!("{address:04X}");
                            self.status = format!(
                                "Front Panel Operator base address set to {address:04X}h"
                            );
                        }
                        None => self.report_load_error(
                            "Front Panel Operator base address must be hexadecimal, e.g. 0000, 0100, or 0x0100.",
                        ),
                    }
                }
                if ui.button("Clear source").clicked() {
                    state.source_name.clear();
                    state.bytes.clear();
                    self.status = "Front Panel Operator source cleared".into();
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.heading("Front Panel Operator");
                    ui.small("A didactic front-panel console: each Config switches action positions the real A15..A0 switches; each Execute action uses the emulated EXAMINE / DEPOSIT / DEPOSIT NEXT path. Watch the main Altair panel while stepping through the program.");
                    ui.small(".BIN/.ROM and assembled machine-code images are treated as sequential bytes. CP/M .COM defaults to 0100h. A .TAP can be inspected as raw bytes here, but authentic paper-tape loading should still use the ASR-33 plus the appropriate bootstrap.");
                    ui.small("S-100 mapping/read-back shown here is host-side instrumentation after the panel operation. It never chooses a RAM card or substitutes for the panel bus cycle.");
                    ui.add_space(6.0);

                    let base = state.base_address;
                    let byte_count = state.bytes.len();
                    let last_address = byte_count
                        .checked_sub(1)
                        .and_then(|index| Self::standalone_operator_target_address(base, index));

                    egui::CollapsingHeader::new("Source / machine status")
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new("standalone-front-panel-operator-summary")
                                .num_columns(2)
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label("Source");
                                    ui.label(if state.source_name.is_empty() {
                                        "No source loaded".into()
                                    } else {
                                        state.source_name.clone()
                                    });
                                    ui.end_row();
                                    ui.label("Base");
                                    ui.monospace(format!("{base:04X}h / {base:06o}o"));
                                    ui.end_row();
                                    ui.label("Bytes");
                                    ui.label(byte_count.to_string());
                                    ui.end_row();
                                    ui.label("Range");
                                    ui.label(match last_address {
                                        Some(last) => format!("{base:04X}h..{last:04X}h"),
                                        None if byte_count == 0 => "—".into(),
                                        None => "wraps past FFFFh — invalid".into(),
                                    });
                                    ui.end_row();
                                    ui.label("Machine");
                                    ui.label(format!(
                                        "{} · {} · {} bytes across installed S-100 RAM cards",
                                        if self.machine.powered() { "POWER ON" } else { "POWER OFF" },
                                        if self.machine.running() { "RUNNING" } else { "STOPPED" },
                                        self.machine.installed_ram_bytes(),
                                    ));
                                    ui.end_row();
                                });
                        });

                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("1 · Select initial address")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label("Place the program start address on A15..A0, then operate EXAMINE.");
                                let config = ui.button("Config switches").on_hover_text(
                                    Self::standalone_switch_tooltip(
                                        base,
                                        "For EXAMINE the sixteen switches represent the memory address the operator wants to select.",
                                    ),
                                );
                                if config.clicked() {
                                    self.standalone_operator_configure_switches(
                                        base,
                                        "initial EXAMINE address",
                                    );
                                }
                                if ui.button("Execute EXAMINE").clicked() {
                                    if let Err(error) = self.standalone_operator_execute_examine(base) {
                                        self.report_load_error(error);
                                    }
                                }
                            });
                        });

                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("2 · Enter program bytes")
                        .default_open(true)
                        .show(ui, |ui| {
                            if byte_count == 0 {
                                ui.label("Load a binary or tape image to build the operator sequence.");
                                return;
                            }
                            if last_address.is_none() {
                                ui.colored_label(
                                    Color32::LIGHT_RED,
                                    "Base + file length exceeds FFFFh. Choose a lower base or a smaller image.",
                                );
                                return;
                            }

                            ui.small("The first byte uses DEPOSIT at the address selected above. Every following byte uses DEPOSIT NEXT, which advances the panel address and writes the new byte just as an operator would.");
                            ui.add_space(4.0);

                            egui::Grid::new("standalone-front-panel-operator-header")
                                .num_columns(6)
                                .spacing([12.0, 4.0])
                                .show(ui, |ui| {
                                    ui.strong("Address");
                                    ui.strong("Octal");
                                    ui.strong("Data");
                                    ui.strong("Operation");
                                    ui.strong("");
                                    ui.strong("");
                                    ui.end_row();
                                });

                            let row_height = 28.0;
                            egui::ScrollArea::vertical()
                                .max_height(420.0)
                                .auto_shrink([false, false])
                                .show_rows(ui, row_height, byte_count, |ui, rows| {
                                    for index in rows {
                                        let byte = state.bytes[index];
                                        let Some(address) =
                                            Self::standalone_operator_target_address(base, index)
                                        else {
                                            continue;
                                        };
                                        let inspection = self.machine.inspect_memory_mapping(address);
                                        let stored = matches!(
                                            inspection.drivers.as_slice(),
                                            [driver] if driver.value == byte
                                        );
                                        egui::Grid::new((
                                            "standalone-front-panel-operator-row",
                                            index,
                                        ))
                                        .num_columns(6)
                                        .spacing([12.0, 4.0])
                                        .show(ui, |ui| {
                                            ui.colored_label(
                                                if stored {
                                                    Color32::LIGHT_GREEN
                                                } else {
                                                    ui.visuals().text_color()
                                                },
                                                egui::RichText::new(format!("{address:04X}"))
                                                    .monospace(),
                                            )
                                            .on_hover_text(mapping_detail(address, &inspection));
                                            ui.monospace(format!("{address:06o}"));
                                            ui.monospace(format!("{byte:02X} / {byte:03o}"));
                                            ui.label(if index == 0 {
                                                "DEPOSIT"
                                            } else {
                                                "DEPOSIT NEXT"
                                            });
                                            let config = ui.button("Config switches").on_hover_text(
                                                Self::standalone_switch_tooltip(
                                                    u16::from(byte),
                                                    &format!(
                                                        "For a deposit, A7..A0 are the data byte for address {address:04X}h. A15..A8 are kept DOWN so the panel visibly shows only the eight data bits.",
                                                    ),
                                                ),
                                            );
                                            if config.clicked() {
                                                self.standalone_operator_configure_switches(
                                                    u16::from(byte),
                                                    &format!(
                                                        "data {byte:02X}h for address {address:04X}h"
                                                    ),
                                                );
                                            }
                                            if ui.button("Execute").clicked() {
                                                if let Err(error) =
                                                    self.standalone_operator_execute_deposit(
                                                        address,
                                                        byte,
                                                        index != 0,
                                                    )
                                                {
                                                    self.report_load_error(error);
                                                }
                                            }
                                            ui.end_row();
                                        });
                                    }
                                });
                            ui.small("Green addresses mean exactly one S-100 RAM card currently decodes the address and contains the expected byte. Mapping/read-back is observation only: Execute still performs the real panel operation and never silently jumps to a row.");
                        });

                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("3 · Run from base")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let config = ui.button("Config switches").on_hover_text(
                                    Self::standalone_switch_tooltip(
                                        base,
                                        "Before RUN, the operator normally EXAMINEs the desired entry address so the program counter/front-panel address is positioned at the program start.",
                                    ),
                                );
                                if config.clicked() {
                                    self.standalone_operator_configure_switches(
                                        base,
                                        "program entry address",
                                    );
                                }
                                if ui.button("Execute EXAMINE").clicked() {
                                    if let Err(error) = self.standalone_operator_execute_examine(base) {
                                        self.report_load_error(error);
                                    }
                                }
                                if ui
                                    .add_enabled(
                                        self.machine.powered() && !self.machine.running(),
                                        egui::Button::new("RUN"),
                                    )
                                    .clicked()
                                {
                                    self.audio.play_once("assets/click.mp3");
                                    self.machine.set_running(true);
                                    self.status = format!(
                                        "Front Panel Operator: RUN enabled from {base:04X}h"
                                    );
                                }
                            });
                        });
                });
        });
    }

    pub(in crate::app) fn show_standalone_front_panel_operator_viewport(
        &mut self,
        parent_ctx: &egui::Context,
    ) {
        let mut state = Self::standalone_operator_state(parent_ctx);
        if !state.open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-standalone-front-panel-operator"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — Front Panel Operator")
                .with_inner_size([1080.0, 760.0])
                .with_min_inner_size([760.0, 460.0])
                .with_resizable(true),
            |operator_ctx, _class| {
                self.draw_standalone_front_panel_operator(operator_ctx, &mut state);
                if operator_ctx.input(|input| input.viewport().close_requested()) {
                    state.open = false;
                }
            },
        );

        Self::store_standalone_operator_state(parent_ctx, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_operator_address_math_rejects_wrap() {
        assert_eq!(
            RusTairApp::standalone_operator_target_address(0x0100, 0),
            Some(0x0100)
        );
        assert_eq!(
            RusTairApp::standalone_operator_target_address(0xffff, 1),
            None
        );
    }

    #[test]
    fn standalone_operator_parses_common_hex_address_forms() {
        assert_eq!(RusTairApp::parse_operator_hex_address("0100"), Some(0x0100));
        assert_eq!(
            RusTairApp::parse_operator_hex_address("0x0100"),
            Some(0x0100)
        );
        assert_eq!(RusTairApp::parse_operator_hex_address("0100h"), Some(0x0100));
        assert_eq!(RusTairApp::parse_operator_hex_address("10000"), None);
    }
}

use super::super::{egui, RusTairApp};
use crate::machine::{MAX_MEM_SIZE, MEMORY_BOARD_SIZE};

const BYTES_PER_ROW: usize = 16;
const ROW_COUNT: usize = MAX_MEM_SIZE / BYTES_PER_ROW;
const ROW_HEIGHT: f32 = 22.0;

#[derive(Clone)]
struct MemoryViewerUiState {
    window_open: bool,
    address_input: String,
    selected_address: u16,
    pending_jump: Option<u16>,
    follow_pc: bool,
}

impl Default for MemoryViewerUiState {
    fn default() -> Self {
        Self {
            window_open: false,
            address_input: "0000".into(),
            selected_address: 0,
            pending_jump: Some(0),
            follow_pc: false,
        }
    }
}

impl RusTairApp {
    fn memory_viewer_state(ctx: &egui::Context) -> MemoryViewerUiState {
        ctx.data(|data| {
            data.get_temp::<MemoryViewerUiState>(egui::Id::new("rustair-memory-viewer-state"))
                .unwrap_or_default()
        })
    }

    fn store_memory_viewer_state(ctx: &egui::Context, state: MemoryViewerUiState) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("rustair-memory-viewer-state"), state);
        });
    }

    pub(in crate::app) fn open_memory_viewer(&mut self, ctx: &egui::Context) {
        let mut state = Self::memory_viewer_state(ctx);
        state.window_open = true;
        state.selected_address = self.machine.cpu.pc;
        state.address_input = format!("{:04X}", state.selected_address);
        state.pending_jump = Some(state.selected_address);
        Self::store_memory_viewer_state(ctx, state);
    }

    fn parse_memory_address(text: &str) -> Option<u16> {
        let trimmed = text.trim();
        let trimmed = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);
        let trimmed = trimmed
            .strip_suffix('h')
            .or_else(|| trimmed.strip_suffix('H'))
            .unwrap_or(trimmed);
        (!trimmed.is_empty())
            .then(|| u16::from_str_radix(trimmed, 16).ok())
            .flatten()
    }

    fn select_memory_address(state: &mut MemoryViewerUiState, address: u16, jump: bool) {
        state.selected_address = address;
        state.address_input = format!("{address:04X}");
        if jump {
            state.pending_jump = Some(address);
        }
    }

    fn printable_ascii(byte: u8) -> char {
        if byte.is_ascii_graphic() || byte == b' ' {
            byte as char
        } else {
            '.'
        }
    }

    fn draw_memory_board_map(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let installed = self.machine.installed_ram_bytes();
        let selected_board = state.selected_address as usize / MEMORY_BOARD_SIZE;

        ui.collapsing("1 KiB board map — click a board to jump", |ui| {
            egui::Grid::new("ram-board-map")
                .num_columns(8)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for board in 0..(MAX_MEM_SIZE / MEMORY_BOARD_SIZE) {
                        let start = board * MEMORY_BOARD_SIZE;
                        let installed_board = start < installed;
                        let protected =
                            installed_board && self.machine.bus.is_protected(start as u16);
                        let mut label = egui::RichText::new(if protected {
                            format!("P {start:04X}")
                        } else {
                            format!("  {start:04X}")
                        })
                        .monospace();
                        if !installed_board {
                            label = label.weak();
                        }

                        let response = ui.selectable_label(selected_board == board, label);
                        if response.clicked() {
                            state.follow_pc = false;
                            Self::select_memory_address(state, start as u16, true);
                        }
                        response.on_hover_text(if installed_board {
                            if protected {
                                "Installed 1 KiB RAM board — write protected"
                            } else {
                                "Installed 1 KiB RAM board"
                            }
                        } else {
                            "Uninstalled address-space slot"
                        });

                        if board % 8 == 7 {
                            ui.end_row();
                        }
                    }
                });
        });
    }

    fn draw_memory_toolbar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let installed = self.machine.installed_ram_bytes();
        let installed_end = installed.saturating_sub(1);
        let pc = self.machine.cpu.pc;
        let sp = self.machine.cpu.sp;

        ui.horizontal_wrapped(|ui| {
            ui.strong("RAM VIEWER");
            ui.separator();
            ui.label(format!(
                "{} installed — 0000h–{:04X}h",
                self.config.machine.ram_size.label(),
                installed_end
            ));
            ui.separator();
            ui.label("Address:");

            let response = ui.add_sized(
                [72.0, 24.0],
                egui::TextEdit::singleline(&mut state.address_input)
                    .font(egui::TextStyle::Monospace)
                    .char_limit(6),
            );
            if response.changed() {
                state.address_input = state
                    .address_input
                    .chars()
                    .filter(|c| c.is_ascii_hexdigit() || matches!(c, 'x' | 'X' | 'h' | 'H'))
                    .collect::<String>()
                    .to_uppercase();
            }

            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Go").clicked() || enter {
                if let Some(address) = Self::parse_memory_address(&state.address_input) {
                    state.follow_pc = false;
                    Self::select_memory_address(state, address, true);
                }
            }

            if ui.button(format!("PC {pc:04X}")).clicked() {
                state.follow_pc = false;
                Self::select_memory_address(state, pc, true);
            }
            if ui.button(format!("SP {sp:04X}")).clicked() {
                state.follow_pc = false;
                Self::select_memory_address(state, sp, true);
            }

            if ui.checkbox(&mut state.follow_pc, "Follow PC").changed() && state.follow_pc {
                Self::select_memory_address(state, pc, true);
            }
        });
        ui.small("Read-only and non-invasive: debugger peeks never trigger guest memory-read side effects. The table always covers the full 8080 address space 0000h–FFFFh; uninstalled locations are shown as --.");
    }

    fn draw_memory_table(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let pc = self.machine.cpu.pc;
        if state.follow_pc {
            Self::select_memory_address(state, pc, true);
        }

        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            ui.add_sized(
                [54.0, ROW_HEIGHT],
                egui::Label::new(egui::RichText::new("ADDR").monospace().strong()),
            );
            ui.add_sized(
                [20.0, ROW_HEIGHT],
                egui::Label::new(egui::RichText::new("P").monospace().strong()),
            );
            for column in 0..BYTES_PER_ROW {
                ui.add_sized(
                    [28.0, ROW_HEIGHT],
                    egui::Label::new(
                        egui::RichText::new(format!("{column:02X}"))
                            .monospace()
                            .strong(),
                    ),
                );
            }
            ui.separator();
            ui.label(egui::RichText::new("ASCII").monospace().strong());
        });
        ui.separator();

        let target = state.pending_jump.take();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("ram-viewer-scroll")
            .auto_shrink([false, false])
            .animated(false);
        if let Some(address) = target {
            let target_row = address as usize / BYTES_PER_ROW;
            let context_row = target_row.saturating_sub(5);
            scroll = scroll.vertical_scroll_offset(context_row as f32 * ROW_HEIGHT);
        }

        scroll.show_rows(ui, ROW_HEIGHT, ROW_COUNT, |ui, rows| {
            for row in rows {
                let start = row * BYTES_PER_ROW;
                let row_address = start as u16;
                let protected = self.machine.bus.is_protected(row_address);
                let row_contains_pc = (start..start + BYTES_PER_ROW).contains(&(pc as usize));
                let row_contains_selected = (start..start + BYTES_PER_ROW)
                    .contains(&(state.selected_address as usize));

                ui.horizontal(|ui| {
                    let mut address_text =
                        egui::RichText::new(format!("{start:04X}")).monospace();
                    if row_contains_pc {
                        address_text = address_text.strong();
                    }
                    if row_contains_selected {
                        address_text = address_text.underline();
                    }
                    ui.add_sized([54.0, ROW_HEIGHT], egui::Label::new(address_text));
                    ui.add_sized(
                        [20.0, ROW_HEIGHT],
                        egui::Label::new(
                            egui::RichText::new(if protected { "P" } else { " " }).monospace(),
                        ),
                    );

                    let selected_fill = ui.visuals().selection.bg_fill;
                    let pc_fill = ui.visuals().widgets.active.bg_fill;
                    let weak_color = ui.visuals().weak_text_color();
                    let mut ascii = String::with_capacity(BYTES_PER_ROW);

                    for column in 0..BYTES_PER_ROW {
                        let address = (start + column) as u16;
                        match self.machine.bus.peek_memory(address) {
                            Some(byte) => {
                                ascii.push(Self::printable_ascii(byte));
                                let mut text =
                                    egui::RichText::new(format!("{byte:02X}")).monospace();
                                if address == pc {
                                    text = text.strong().background_color(pc_fill);
                                }
                                if address == state.selected_address {
                                    text = text.background_color(selected_fill);
                                }
                                let response = ui.add_sized(
                                    [28.0, ROW_HEIGHT],
                                    egui::Label::new(text).sense(egui::Sense::click()),
                                );
                                if response.clicked() {
                                    state.follow_pc = false;
                                    Self::select_memory_address(state, address, false);
                                }
                                response.on_hover_text(format!(
                                    "{:04X}h = {:02X}h = {}{}",
                                    address,
                                    byte,
                                    byte,
                                    if protected {
                                        " — protected board"
                                    } else {
                                        ""
                                    }
                                ));
                            }
                            None => {
                                ascii.push(' ');
                                ui.add_sized(
                                    [28.0, ROW_HEIGHT],
                                    egui::Label::new(
                                        egui::RichText::new("--")
                                            .monospace()
                                            .color(weak_color),
                                    ),
                                );
                            }
                        }
                    }
                    ui.separator();
                    ui.label(egui::RichText::new(ascii).monospace());
                });
            }
        });
    }

    fn draw_memory_status(&self, ui: &mut egui::Ui, state: &MemoryViewerUiState) {
        let address = state.selected_address;
        match self.machine.bus.peek_memory(address) {
            Some(byte) => {
                let protected = self.machine.bus.is_protected(address);
                let board = address as usize / MEMORY_BOARD_SIZE;
                ui.small(format!(
                    "Selected {:04X}h  |  {:02X}h  |  decimal {}  |  ASCII '{}'  |  1 KiB board {}{}",
                    address,
                    byte,
                    byte,
                    Self::printable_ascii(byte),
                    board,
                    if protected { "  |  WRITE PROTECTED" } else { "" }
                ));
            }
            None => {
                ui.small(format!(
                    "Selected {:04X}h  |  UNINSTALLED — guest reads return 00h and writes are ignored",
                    address
                ));
            }
        }
    }

    fn draw_memory_viewer_window(
        &mut self,
        ctx: &egui::Context,
        state: &mut MemoryViewerUiState,
    ) {
        egui::TopBottomPanel::top("memory-viewer-toolbar").show(ctx, |ui| {
            self.draw_memory_toolbar(ui, state);
        });

        egui::TopBottomPanel::bottom("memory-viewer-status").show(ctx, |ui| {
            self.draw_memory_status(ui, state);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_memory_board_map(ui, state);
            ui.separator();
            self.draw_memory_table(ui, state);
        });
    }

    pub(in crate::app) fn show_memory_viewer_viewport(&mut self, parent_ctx: &egui::Context) {
        let mut state = Self::memory_viewer_state(parent_ctx);
        if !state.window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-memory-viewer"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — RAM Viewer")
                .with_inner_size([900.0, 760.0])
                .with_min_inner_size([740.0, 420.0])
                .with_resizable(true),
            |memory_ctx, _class| {
                self.draw_memory_viewer_window(memory_ctx, &mut state);
                if memory_ctx.input(|i| i.viewport().close_requested()) {
                    state.window_open = false;
                }
            },
        );

        Self::store_memory_viewer_state(parent_ctx, state);
    }
}

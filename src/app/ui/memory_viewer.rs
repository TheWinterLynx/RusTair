use super::super::{egui, RusTairApp};
use super::execution_position::current_instruction_address;
use super::s100_memory_inspection::{
    card_window, mapping_cell_text, mapping_detail, mapping_summary, ram_driver_line,
    visible_ram_value,
};
use crate::cpu8080::{FLAG_AC, FLAG_C, FLAG_P, FLAG_S, FLAG_Z};
use crate::debugger8080::{decode_at, detect_simple_backward_loop, InstructionAt, SimpleLoop};
use crate::explain8080::{explain_instruction, MemoryValue8080};
use crate::machine::MAX_MEM_SIZE;
use crate::memory_activity8080::{
    summarize_memory_activity_8080, MemoryActivity8080, MemoryActivityMap8080,
};

const BYTES_PER_ROW: usize = 16;
const ROW_COUNT: usize = MAX_MEM_SIZE / BYTES_PER_ROW;
const ROW_HEIGHT: f32 = 22.0;
const SIDEBAR_DEFAULT_WIDTH: f32 = 455.0;
const CURRENT_INSTRUCTION_HEIGHT: f32 = 142.0;
const INSTRUCTION_EXPLAINER_HEIGHT: f32 = 220.0;

#[derive(Clone)]
struct MemoryViewerUiState {
    window_open: bool,
    address_input: String,
    selected_address: u16,
    pending_jump: Option<u16>,
    follow_exec: bool,
    edit_input: String,
    edit_value: u8,
    respect_protection: bool,
    last_edit_message: Option<String>,
    activity_overlay: bool,
    activity_show_execute: bool,
    activity_show_read: bool,
    activity_show_write: bool,
}

impl Default for MemoryViewerUiState {
    fn default() -> Self {
        Self {
            window_open: false,
            address_input: "0000".into(),
            selected_address: 0,
            pending_jump: Some(0),
            follow_exec: false,
            edit_input: "00".into(),
            edit_value: 0,
            respect_protection: true,
            last_edit_message: None,
            activity_overlay: false,
            activity_show_execute: true,
            activity_show_read: true,
            activity_show_write: true,
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
        let execution_address = current_instruction_address(self);
        self.select_memory_address(&mut state, execution_address, true);
        Self::store_memory_viewer_state(ctx, state);
    }

    fn strip_hex_affixes(text: &str) -> &str {
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

    fn parse_memory_address(text: &str) -> Option<u16> {
        let trimmed = Self::strip_hex_affixes(text);
        (!trimmed.is_empty())
            .then(|| u16::from_str_radix(trimmed, 16).ok())
            .flatten()
    }

    fn parse_memory_byte(text: &str) -> Option<u8> {
        let trimmed = Self::strip_hex_affixes(text);
        (!trimmed.is_empty())
            .then(|| u8::from_str_radix(trimmed, 16).ok())
            .flatten()
    }

    fn select_memory_address(
        &mut self,
        state: &mut MemoryViewerUiState,
        address: u16,
        jump: bool,
    ) {
        state.selected_address = address;
        state.address_input = format!("{address:04X}");
        let inspection = self.machine.inspect_memory_mapping(address);
        if inspection.drivers.len() == 1 {
            let byte = inspection.drivers[0].value;
            state.edit_value = byte;
            state.edit_input = format!("{byte:02X}");
        } else {
            state.edit_value = visible_ram_value(&inspection).unwrap_or(0);
            state.edit_input = format!("{:02X}", state.edit_value);
        }
        state.last_edit_message = None;
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

    fn ascii_description(byte: u8) -> String {
        if byte.is_ascii_graphic() || byte == b' ' {
            format!("'{}'", byte as char)
        } else {
            "non-printable".into()
        }
    }

    fn grouped_binary8(value: u8) -> String {
        format!("{:04b} {:04b}", value >> 4, value & 0x0f)
    }

    fn grouped_binary16(value: u16) -> String {
        format!(
            "{:04b} {:04b} {:04b} {:04b}",
            (value >> 12) & 0x0f,
            (value >> 8) & 0x0f,
            (value >> 4) & 0x0f,
            value & 0x0f,
        )
    }

    fn decode_memory_instruction(&mut self, address: u16) -> Option<InstructionAt> {
        decode_at(|candidate| self.machine.peek_memory(candidate), address)
    }

    fn current_simple_loop(&mut self) -> Option<SimpleLoop> {
        let execution_address = current_instruction_address(self);
        let flags = self.machine.intel8080_state().flags;
        detect_simple_backward_loop(
            |address| self.machine.peek_memory(address),
            execution_address,
            flags,
        )
    }

    fn activity_exec_color() -> egui::Color32 {
        egui::Color32::from_rgb(72, 116, 220)
    }

    fn activity_read_color() -> egui::Color32 {
        egui::Color32::from_rgb(68, 174, 104)
    }

    fn activity_write_color() -> egui::Color32 {
        egui::Color32::from_rgb(220, 82, 68)
    }

    fn filtered_activity_count(state: &MemoryViewerUiState, activity: MemoryActivity8080) -> u64 {
        let mut total = 0u64;
        if state.activity_show_execute {
            total = total.saturating_add(activity.execute_count);
        }
        if state.activity_show_read {
            total = total.saturating_add(activity.read_count);
        }
        if state.activity_show_write {
            total = total.saturating_add(activity.write_count);
        }
        total
    }

    fn latest_selected_activity(
        state: &MemoryViewerUiState,
        activity: MemoryActivity8080,
    ) -> Option<(&'static str, u64)> {
        let mut latest: Option<(&'static str, u64)> = None;
        let mut consider = |label: &'static str, sequence: Option<u64>| {
            let Some(sequence) = sequence else { return; };
            if latest.is_none_or(|(_, current)| sequence >= current) {
                latest = Some((label, sequence));
            }
        };
        if state.activity_show_execute {
            consider("EXEC", activity.last_execute_sequence);
        }
        if state.activity_show_read {
            consider("READ", activity.last_read_sequence);
        }
        if state.activity_show_write {
            consider("WRITE", activity.last_write_sequence);
        }
        latest
    }

    fn activity_overlay_fill(
        state: &MemoryViewerUiState,
        activity: MemoryActivity8080,
        max_count: u64,
    ) -> Option<egui::Color32> {
        let count = Self::filtered_activity_count(state, activity);
        let (kind, _) = Self::latest_selected_activity(state, activity)?;
        if count == 0 || max_count == 0 {
            return None;
        }
        let denominator = (max_count as f32 + 1.0).ln().max(1.0);
        let strength = ((count as f32 + 1.0).ln() / denominator).clamp(0.0, 1.0);
        let alpha = (54.0 + strength * 140.0).round() as u8;
        let base = match kind {
            "WRITE" => Self::activity_write_color(),
            "READ" => Self::activity_read_color(),
            _ => Self::activity_exec_color(),
        };
        Some(egui::Color32::from_rgba_unmultiplied(
            base.r(),
            base.g(),
            base.b(),
            alpha,
        ))
    }

    fn activity_hover_text(activity: MemoryActivity8080) -> String {
        if !activity.any() {
            return "Activity: none in retained instruction trace".into();
        }
        format!(
            "Activity (retained trace): EXEC {} | READ {} | WRITE {} | last #{}",
            activity.execute_count,
            activity.read_count,
            activity.write_count,
            activity
                .last_sequence()
                .map(|sequence| sequence.to_string())
                .unwrap_or_else(|| "-".into()),
        )
    }

    fn draw_activity_legend_item(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 1.0, color);
        ui.small(label);
    }

    fn draw_activity_stripes(
        ui: &egui::Ui,
        rect: egui::Rect,
        state: &MemoryViewerUiState,
        activity: MemoryActivity8080,
    ) {
        if !state.activity_overlay || !activity.any() {
            return;
        }
        let inner = rect.shrink(2.5);
        let stripe_width = 3.0;
        let x0 = inner.right() - stripe_width;
        let segment_height = inner.height() / 3.0;
        let painter = ui.painter();
        let slots = [
            (
                state.activity_show_execute && activity.execute_count != 0,
                Self::activity_exec_color(),
            ),
            (
                state.activity_show_read && activity.read_count != 0,
                Self::activity_read_color(),
            ),
            (
                state.activity_show_write && activity.write_count != 0,
                Self::activity_write_color(),
            ),
        ];
        for (index, (visible, color)) in slots.into_iter().enumerate() {
            if !visible {
                continue;
            }
            let top = inner.top() + segment_height * index as f32;
            let bottom = if index == 2 {
                inner.bottom()
            } else {
                top + segment_height - 0.8
            };
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x0, top),
                    egui::pos2(inner.right(), bottom),
                ),
                0.0,
                color,
            );
        }
    }

    fn draw_cell_markers(
        ui: &egui::Ui,
        rect: egui::Rect,
        address: u16,
        execution_address: u16,
        pc: u16,
        sp: u16,
        hl: u16,
    ) {
        let painter = ui.painter();
        let rect = rect.shrink(1.0);
        if address == execution_address {
            painter.rect_stroke(
                rect,
                1.0,
                egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if address == pc {
            painter.line_segment(
                [rect.left_top(), rect.left_bottom()],
                egui::Stroke::new(2.0_f32, ui.visuals().widgets.active.fg_stroke.color),
            );
        }
        if address == hl {
            painter.line_segment(
                [rect.left_top(), rect.right_top()],
                egui::Stroke::new(2.0_f32, egui::Color32::LIGHT_BLUE),
            );
        }
        if address == sp {
            painter.line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                egui::Stroke::new(2.0_f32, egui::Color32::YELLOW),
            );
        }
    }

    fn instruction_hover_text(
        &mut self,
        address: u16,
        byte: u8,
        execution_address: u16,
        pc: u16,
        sp: u16,
        hl: u16,
        activity: MemoryActivity8080,
    ) -> String {
        let inspection = self.machine.inspect_memory_mapping(address);
        let mut hover = mapping_detail(address, &inspection);
        hover.push_str(&format!(
            "\n\n{address:04X}h = {byte:02X}h = decimal {byte} = {}",
            Self::ascii_description(byte),
        ));
        if address == execution_address {
            hover.push_str(" · EXEC");
        }
        if address == pc {
            hover.push_str(" · PC(reg)");
        }
        if address == sp {
            hover.push_str(" · SP");
        }
        if address == hl {
            hover.push_str(" · HL/M");
        }
        if activity.any() {
            hover.push_str("\n");
            hover.push_str(&Self::activity_hover_text(activity));
        }

        if inspection.drivers.len() != 1 {
            hover.push_str("\n\nInstruction decode is suppressed for overlapped RAM because host tools do not choose one physical card arbitrarily.");
            return hover;
        }

        hover.push_str("\n\n");
        let Some(instruction) = self.decode_memory_instruction(address) else {
            hover.push_str("No complete 8080 instruction can be decoded here because one or more bytes are not uniquely mapped RAM.");
            return hover;
        };
        if address == execution_address {
            hover.push_str("CPU instruction at EXEC boundary:\n");
        } else {
            hover.push_str("Decode only - if execution started at this byte:\n");
        }
        hover.push_str(&format!(
            "{}  {}\nLength: {} byte(s) | {}\nFlags affected: {}\nMemory: {} | I/O: {}\nFlow: {}",
            instruction.decoded.bytes_text(instruction.bytes),
            instruction.decoded.text(),
            instruction.decoded.length,
            instruction.decoded.timing.label(),
            instruction.decoded.flags.label(),
            instruction.decoded.memory_label(),
            instruction.decoded.io.label(),
            instruction.decoded.flow_label(),
        ));
        if instruction.decoded.undocumented_alias {
            hover.push_str("\nUndocumented Intel 8080 alias accepted by the RusTair cores.");
        }
        hover
    }

    fn draw_memory_toolbar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let hardware = self.machine.s100_hardware();
        let ram_cards = hardware
            .installed_cards()
            .filter(|(_, card)| card_window(*card).is_some())
            .count();
        let physical_bytes = hardware.installed_ram_bytes();
        let cpu_slot = hardware.cpu_slots().next();
        let execution_address = current_instruction_address(self);
        let cpu = self.machine.intel8080_state();
        let panel = self.machine.front_panel_state();
        let execution_state = if cpu.halted.unwrap_or(false) {
            if panel.running {
                "HALTED | RUN latch ON"
            } else {
                "HALTED | RUN latch OFF"
            }
        } else if panel.running {
            "RUNNING"
        } else {
            "STOPPED"
        };

        ui.horizontal_wrapped(|ui| {
            ui.strong("S-100 RAM INSPECTOR / 8080 DEBUGGER");
            ui.separator();
            ui.label(execution_state);
            ui.separator();
            ui.small(format!("Core {}", self.machine.engine().label()));
            if let Some(slot) = cpu_slot {
                ui.separator();
                ui.small(format!("CPU board S{slot:02}"));
            }
            ui.separator();
            ui.small(format!(
                "RAM {ram_cards} card(s) · {} bytes physical",
                physical_bytes,
            ));
            ui.separator();
            ui.label("Jump:");
            let response = ui.add_sized(
                [68.0, 22.0],
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
            if ui.small_button("Go").clicked() || enter {
                if let Some(address) = Self::parse_memory_address(&state.address_input) {
                    state.follow_exec = false;
                    self.select_memory_address(state, address, true);
                }
            }
            for (label, address) in [
                ("EXEC", execution_address),
                ("PC", cpu.pc),
                ("SP", cpu.sp),
                ("HL", cpu.hl()),
            ] {
                if ui.small_button(format!("{label} {address:04X}")).clicked() {
                    state.follow_exec = false;
                    self.select_memory_address(state, address, true);
                }
            }
            if ui.checkbox(&mut state.follow_exec, "Follow EXEC").changed() && state.follow_exec {
                self.select_memory_address(state, execution_address, true);
            }
        });
    }

    fn draw_memory_table(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let execution_address = current_instruction_address(self);
        let cpu = self.machine.intel8080_state();
        let pc = cpu.pc;
        let sp = cpu.sp;
        let hl = cpu.hl();
        if state.follow_exec && state.selected_address != execution_address {
            self.select_memory_address(state, execution_address, true);
        }

        let activity_map: Option<MemoryActivityMap8080> = if state.activity_overlay {
            Some(summarize_memory_activity_8080(
                &self.machine.instruction_trace_snapshot(),
                self.machine.instruction_trace_metadata(),
            ))
        } else {
            None
        };
        let max_activity_count = activity_map
            .as_ref()
            .map(|map| {
                map.iter()
                    .map(|(_, activity)| Self::filtered_activity_count(state, activity))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        ui.set_min_width(720.0);
        ui.spacing_mut().item_spacing.x = 2.0;
        if state.activity_overlay {
            ui.horizontal(|ui| {
                ui.strong("ACTIVITY");
                Self::draw_activity_legend_item(ui, "EXEC", Self::activity_exec_color());
                ui.separator();
                Self::draw_activity_legend_item(ui, "READ", Self::activity_read_color());
                ui.separator();
                Self::draw_activity_legend_item(ui, "WRITE", Self::activity_write_color());
                ui.separator();
                ui.small("right edge: top / middle / bottom; tint = relative frequency");
            });
        }
        ui.horizontal(|ui| {
            ui.add_sized(
                [54.0, ROW_HEIGHT],
                egui::Label::new(egui::RichText::new("ADDR").monospace().strong()),
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
        ui.small("-- = UNMAPPED/open bus FFh · underlined byte = non-contended OVERLAP · !! = electrical CONTENTION");
        ui.separator();

        let target = state.pending_jump.take();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("ram-viewer-scroll")
            .auto_shrink([false, false])
            .animated(false);
        if let Some(address) = target {
            let target_row = address as usize / BYTES_PER_ROW;
            scroll = scroll.vertical_scroll_offset(target_row.saturating_sub(5) as f32 * ROW_HEIGHT);
        }

        scroll.show_rows(ui, ROW_HEIGHT, ROW_COUNT, |ui, rows| {
            for row in rows {
                let start = row * BYTES_PER_ROW;
                let row_contains_exec =
                    (start..start + BYTES_PER_ROW).contains(&(execution_address as usize));
                let row_contains_selected =
                    (start..start + BYTES_PER_ROW).contains(&(state.selected_address as usize));

                ui.horizontal(|ui| {
                    let mut address_text = egui::RichText::new(format!("{start:04X}")).monospace();
                    if row_contains_exec {
                        address_text = address_text.strong();
                    }
                    if row_contains_selected {
                        address_text = address_text.underline();
                    }
                    ui.add_sized([54.0, ROW_HEIGHT], egui::Label::new(address_text));

                    let selected_fill = ui.visuals().selection.bg_fill;
                    let weak_color = ui.visuals().weak_text_color();
                    let mut ascii = String::with_capacity(BYTES_PER_ROW);

                    for column in 0..BYTES_PER_ROW {
                        let address = (start + column) as u16;
                        let inspection = self.machine.inspect_memory_mapping(address);
                        let activity = activity_map
                            .as_ref()
                            .map(|map| map.get(address))
                            .unwrap_or_default();
                        let overlay_fill =
                            Self::activity_overlay_fill(state, activity, max_activity_count);
                        let visible = visible_ram_value(&inspection);
                        let cell = mapping_cell_text(&inspection);

                        ascii.push(match visible {
                            Some(byte) if !inspection.electrically_contended() => {
                                Self::printable_ascii(byte)
                            }
                            _ if inspection.electrically_contended() => '!',
                            _ => ' ',
                        });

                        let mut text = egui::RichText::new(cell).monospace();
                        if inspection.is_unmapped() {
                            text = text.color(weak_color);
                        } else if inspection.is_overlap() && !inspection.electrically_contended() {
                            text = text.underline();
                        } else if inspection.electrically_contended() {
                            text = text.strong();
                        }
                        if let Some(fill) = overlay_fill {
                            text = text.background_color(fill);
                        }
                        if address == execution_address {
                            text = text.strong();
                        }
                        if address == state.selected_address {
                            text = text.background_color(selected_fill);
                        }

                        let response = ui.add_sized(
                            [28.0, ROW_HEIGHT],
                            egui::Label::new(text).sense(egui::Sense::click()),
                        );
                        Self::draw_activity_stripes(ui, response.rect, state, activity);
                        Self::draw_cell_markers(
                            ui,
                            response.rect,
                            address,
                            execution_address,
                            pc,
                            sp,
                            hl,
                        );
                        if response.clicked() {
                            state.follow_exec = false;
                            self.select_memory_address(state, address, false);
                        }

                        if response.hovered() {
                            let mut hover = if inspection.drivers.len() == 1 {
                                self.instruction_hover_text(
                                    address,
                                    inspection.drivers[0].value,
                                    execution_address,
                                    pc,
                                    sp,
                                    hl,
                                    activity,
                                )
                            } else {
                                mapping_detail(address, &inspection)
                            };
                            if activity.any() && inspection.drivers.len() != 1 {
                                hover.push_str("\n");
                                hover.push_str(&Self::activity_hover_text(activity));
                            }
                            if activity.write_count != 0 && inspection.is_unmapped() {
                                hover.push_str("\nWRITE activity records attempted guest bus transfers even though no RAM card decoded the address.");
                            }
                            response.on_hover_text(hover);
                        }
                    }
                    ui.separator();
                    ui.label(egui::RichText::new(ascii).monospace());
                });
            }
        });
    }

    fn draw_current_instruction(&mut self, ui: &mut egui::Ui) {
        let execution_address = current_instruction_address(self);
        let cpu = self.machine.intel8080_state();
        let inspection = self.machine.inspect_memory_mapping(execution_address);
        let instruction = self.decode_memory_instruction(execution_address);
        let loop_info = self.current_simple_loop();

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), CURRENT_INSTRUCTION_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!("EXEC ${execution_address:04X}"));
                    ui.separator();
                    ui.monospace(format!("PC(reg) ${:04X} · SP ${:04X}", cpu.pc, cpu.sp));
                });
                ui.label(egui::RichText::new(format!("S-100: {}", mapping_summary(&inspection))).small())
                    .on_hover_text(mapping_detail(execution_address, &inspection));
                if let Some(instruction) = instruction.as_ref() {
                    ui.monospace(format!(
                        "{}  {}",
                        instruction.decoded.bytes_text(instruction.bytes),
                        instruction.decoded.text(),
                    ));
                    ui.small(format!(
                        "{} · flags {} · memory {} · I/O {} · flow {}",
                        instruction.decoded.timing.label(),
                        instruction.decoded.flags.label(),
                        instruction.decoded.memory_label(),
                        instruction.decoded.io.label(),
                        instruction.decoded.flow_label(),
                    ));
                } else {
                    ui.small("No instruction decode: EXEC is not backed by one uniquely mapped sequence of RAM bytes.");
                }
                ui.horizontal(|ui| {
                    if let Some(loop_info) = loop_info.as_ref() {
                        ui.small(format!(
                            "Simple loop {:04X}h-{:04X}h",
                            loop_info.start, loop_info.back_edge,
                        ));
                        if ui.small_button("Loop Inspector").clicked() {
                            self.open_loop_inspector(ui.ctx(), loop_info.clone());
                        }
                    } else {
                        ui.small("Simple loop: -");
                    }
                    if ui.small_button("8080 Debugger").clicked() {
                        self.open_debugger_controls(ui.ctx());
                    }
                    if ui.small_button("Bus / T-state Teacher").clicked() {
                        self.open_bus_teacher(ui.ctx());
                    }
                });
            },
        );
    }

    fn draw_instruction_explainer(&mut self, ui: &mut egui::Ui, state: &MemoryViewerUiState) {
        let address = state.selected_address;
        let inspection = self.machine.inspect_memory_mapping(address);
        let instruction = self.decode_memory_instruction(address);
        let cpu = self.machine.intel8080_state();
        let memory_at_hl = match self.machine.peek_memory(cpu.hl()) {
            Some(value) => MemoryValue8080::Known(value),
            None => MemoryValue8080::Unmapped,
        };

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), INSTRUCTION_EXPLAINER_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.label(egui::RichText::new(mapping_summary(&inspection)).small())
                    .on_hover_text(mapping_detail(address, &inspection));
                let Some(instruction) = instruction.as_ref() else {
                    ui.small("Selected address cannot be decoded as one instruction because its bytes are not uniquely mapped RAM.");
                    return;
                };
                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!("${address:04X}"));
                    ui.monospace(instruction.decoded.bytes_text(instruction.bytes));
                    ui.monospace(instruction.decoded.text());
                });
                let explanation = explain_instruction(&instruction.decoded, cpu, memory_at_hl);
                ui.label(&explanation.summary);
                egui::Grid::new("ram-instruction-explanation-grid")
                    .num_columns(2)
                    .spacing([8.0, 3.0])
                    .show(ui, |ui| {
                        ui.strong("Reads");
                        ui.label(&explanation.reads);
                        ui.end_row();
                        ui.strong("Writes");
                        ui.label(&explanation.writes);
                        ui.end_row();
                        ui.strong("Flags");
                        ui.label(&explanation.flags);
                        ui.end_row();
                        ui.strong("Timing");
                        ui.label(instruction.decoded.timing.label());
                        ui.end_row();
                        ui.strong("Memory");
                        ui.label(&explanation.memory);
                        ui.end_row();
                        ui.strong("I/O");
                        ui.label(&explanation.io);
                        ui.end_row();
                    });
                for line in &explanation.context {
                    ui.small(line);
                }
            },
        );
    }

    fn draw_bit_editor(&self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        egui::Grid::new("ram-byte-bit-editor")
            .num_columns(8)
            .spacing([6.0, 2.0])
            .show(ui, |ui| {
                for bit in (0..8).rev() {
                    ui.label(egui::RichText::new(format!("b{bit}")).monospace().strong());
                }
                ui.end_row();
                for bit in (0..8).rev() {
                    let mask = 1u8 << bit;
                    let set = state.edit_value & mask != 0;
                    if ui.selectable_label(set, if set { "1" } else { "0" }).clicked() {
                        state.edit_value ^= mask;
                        state.edit_input = format!("{:02X}", state.edit_value);
                        state.last_edit_message = None;
                    }
                }
                ui.end_row();
            });
    }

    fn draw_memory_editor(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let address = state.selected_address;
        let inspection = self.machine.inspect_memory_mapping(address);

        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("{:04X}h", address));
            ui.separator();
            ui.strong(mapping_summary(&inspection));
        });
        for driver in &inspection.drivers {
            ui.monospace(ram_driver_line(driver));
        }

        if inspection.is_unmapped() {
            ui.small("No RAM card decodes this address. Guest memory reads see the chassis open-bus value FFh; there is no RAM cell for the debugger to edit.");
            return;
        }
        if inspection.electrically_contended() {
            ui.small("Multiple RAM cards drive different values here. The electrical contention is shown as !! and the debugger will not choose one card to edit.");
            return;
        }
        if inspection.drivers.len() != 1 {
            ui.small("Multiple RAM cards decode this address. Even though they currently agree on DI, debugger editing is disabled because choosing one physical card would hide the overlap.");
            return;
        }

        let driver = &inspection.drivers[0];
        let current_byte = driver.value;
        let protected = driver.protected;
        let protection_supported = driver.config.supports_front_panel_protect();
        let protection_unit = driver.config.protection_unit_bytes();
        ui.small(if protection_supported {
            format!(
                "Front-panel protection: {} · physical protection unit {} byte(s)",
                if protected { "PROTECTED" } else { "writable" },
                protection_unit,
            )
        } else {
            "Front-panel protection: this RAM board does not implement the protect function.".into()
        });

        ui.horizontal_wrapped(|ui| {
            ui.label("New hex:");
            let response = ui.add_sized(
                [58.0, 24.0],
                egui::TextEdit::singleline(&mut state.edit_input)
                    .font(egui::TextStyle::Monospace)
                    .char_limit(4),
            );
            if response.changed() {
                state.edit_input = state
                    .edit_input
                    .chars()
                    .filter(|c| c.is_ascii_hexdigit() || matches!(c, 'x' | 'X' | 'h' | 'H'))
                    .collect::<String>()
                    .to_uppercase();
                if let Some(value) = Self::parse_memory_byte(&state.edit_input) {
                    state.edit_value = value;
                }
                state.last_edit_message = None;
            }
            if ui.small_button("Reload physical byte").clicked() {
                state.edit_value = current_byte;
                state.edit_input = format!("{current_byte:02X}");
                state.last_edit_message = None;
            }
        });
        ui.small(format!(
            "dec {} · {:08b} · ASCII {}",
            state.edit_value,
            state.edit_value,
            Self::ascii_description(state.edit_value),
        ));
        self.draw_bit_editor(ui, state);
        ui.checkbox(&mut state.respect_protection, "Respect physical write protection");

        let valid = Self::parse_memory_byte(&state.edit_input).is_some();
        let blocked = protected && state.respect_protection;
        if ui
            .add_enabled(valid && !blocked, egui::Button::new("Patch physical RAM byte"))
            .clicked()
        {
            let written =
                self.machine
                    .write_memory(address, state.edit_value, state.respect_protection);
            state.last_edit_message = Some(if written {
                format!(
                    "Patched Slot {:02} at {:04X}h with {:02X}h{}",
                    driver.slot,
                    address,
                    state.edit_value,
                    if protected && !state.respect_protection {
                        " using debugger protection override"
                    } else {
                        ""
                    },
                )
            } else {
                "Patch rejected by the current physical RAM mapping/protection state.".into()
            });
        }
        if blocked {
            ui.small("Physical protection is active. Disable 'Respect physical write protection' only for a deliberate debugger override.");
        } else if protected && !state.respect_protection {
            ui.small("Debugger override active: the host tool is bypassing the card's protection latch without fabricating a guest bus cycle.");
        }
        if let Some(message) = &state.last_edit_message {
            ui.small(message);
        }
        if self.machine.running() {
            ui.small("Machine is RUNNING; guest execution can change this same physical RAM byte immediately.");
        }
    }

    fn draw_physical_card_map(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let hardware = self.machine.s100_hardware();
        let cards = hardware
            .installed_cards()
            .filter_map(|(slot, card)| card_window(card).map(|window| (slot, window)))
            .collect::<Vec<_>>();

        ui.small(format!(
            "{} fitted connector(s) · {} RAM card(s) · physical bytes are summed even when address windows overlap.",
            hardware.fitted_connectors(),
            cards.len(),
        ));
        if cards.is_empty() {
            ui.small("No RAM cards are fitted in the S-100 chassis.");
            return;
        }

        egui::Grid::new("s100-ram-physical-card-map")
            .num_columns(2)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                for (slot, (start, end, label)) in cards {
                    let selected = state.selected_address >= start && state.selected_address <= end;
                    let response = ui.selectable_label(
                        selected,
                        egui::RichText::new(format!("S{slot:02} {start:04X}-{end:04X}"))
                            .monospace(),
                    );
                    ui.small(label);
                    ui.end_row();
                    if response.clicked() {
                        state.follow_exec = false;
                        self.select_memory_address(state, start, true);
                    }
                    let inspection = self.machine.inspect_memory_mapping(start);
                    let mut hover = format!(
                        "Slot {slot:02} · {label}\nAddress window {start:04X}h-{end:04X}h",
                    );
                    if let Some(driver) = inspection.drivers.iter().find(|driver| driver.slot == slot)
                    {
                        hover.push_str(&format!(
                            "\nProtection support: {} · unit {} byte(s)",
                            if driver.config.supports_front_panel_protect() {
                                "yes"
                            } else {
                                "no"
                            },
                            driver.config.protection_unit_bytes(),
                        ));
                    }
                    if selected {
                        let selected_inspection =
                            self.machine.inspect_memory_mapping(state.selected_address);
                        hover.push_str("\n\n");
                        hover.push_str(&mapping_detail(
                            state.selected_address,
                            &selected_inspection,
                        ));
                    }
                    response.on_hover_text(hover);
                }
            });
    }

    fn draw_memory_activity_overlay_controls(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut MemoryViewerUiState,
    ) {
        if ui
            .checkbox(
                &mut state.activity_overlay,
                "Enable READ / WRITE / EXECUTE overlay",
            )
            .changed()
        {
            ui.ctx().request_repaint();
        }
        ui.horizontal(|ui| {
            ui.label("Show:");
            ui.checkbox(&mut state.activity_show_execute, "EXEC");
            ui.checkbox(&mut state.activity_show_read, "READ");
            ui.checkbox(&mut state.activity_show_write, "WRITE");
        });
        ui.horizontal(|ui| {
            ui.strong("Legend:");
            Self::draw_activity_legend_item(ui, "EXEC", Self::activity_exec_color());
            ui.separator();
            Self::draw_activity_legend_item(ui, "READ", Self::activity_read_color());
            ui.separator();
            Self::draw_activity_legend_item(ui, "WRITE", Self::activity_write_color());
        });
        ui.small("Every byte cell has one fixed right-edge activity bar: top = EXEC, middle = READ, bottom = WRITE.");
        ui.small("IN/OUT are I/O bus activity, not RAM activity; attempted writes to UNMAPPED addresses can still have WRITE activity without creating a RAM cell.");

        if state.activity_overlay {
            let history = self.machine.instruction_trace_snapshot();
            let metadata = self.machine.instruction_trace_metadata();
            let activity = summarize_memory_activity_8080(&history, metadata);
            let selected = activity.get(state.selected_address);
            ui.monospace(format!(
                "Selected ${:04X}: EXEC {} · READ {} · WRITE {}",
                state.selected_address,
                selected.execute_count,
                selected.read_count,
                selected.write_count,
            ));
            ui.horizontal(|ui| {
                if ui.button("Open full Memory Activity").clicked() {
                    self.open_memory_activity(ui.ctx());
                }
                if ui.button("Clear shared activity / history").clicked() {
                    self.machine.clear_instruction_trace();
                }
            });
        } else {
            ui.small("Overlay OFF: the RAM Inspector does not request instruction capture on its own.");
        }
    }

    fn draw_register8_cells(ui: &mut egui::Ui, name: &str, value: u8) {
        ui.strong(name);
        ui.label(egui::RichText::new(Self::grouped_binary8(value)).monospace());
        ui.label(egui::RichText::new(format!("${value:02X}")).monospace().strong());
    }

    fn draw_cpu_registers_sidebar(&mut self, ui: &mut egui::Ui) {
        let cpu = self.machine.intel8080_state();
        ui.small("Live architectural Intel 8080 register state. The execution engine is connected through the installed MITS 8080 CPU Board.");
        egui::Grid::new("ram-cpu-registers")
            .num_columns(3)
            .spacing([7.0, 3.0])
            .show(ui, |ui| {
                for (name, value) in [
                    ("A", cpu.a),
                    ("F", cpu.flags),
                    ("B", cpu.b),
                    ("C", cpu.c),
                    ("D", cpu.d),
                    ("E", cpu.e),
                    ("H", cpu.h),
                    ("L", cpu.l),
                ] {
                    Self::draw_register8_cells(ui, name, value);
                    ui.end_row();
                }
            });
        ui.small(format!(
            "BC ${:04X} · DE ${:04X} · HL ${:04X} · SP ${:04X} · PC(reg) ${:04X}",
            cpu.bc(),
            cpu.de(),
            cpu.hl(),
            cpu.sp,
            cpu.pc,
        ));
        ui.small(format!(
            "Flags: S={} Z={} AC={} P={} C={}",
            u8::from(cpu.flags & FLAG_S != 0),
            u8::from(cpu.flags & FLAG_Z != 0),
            u8::from(cpu.flags & FLAG_AC != 0),
            u8::from(cpu.flags & FLAG_P != 0),
            u8::from(cpu.flags & FLAG_C != 0),
        ));
        ui.small(egui::RichText::new(Self::grouped_binary16(cpu.pc)).monospace());
    }

    fn draw_memory_help(&self, ui: &mut egui::Ui) {
        ui.small("- The table is always the Intel 8080 address space 0000h-FFFFh. It is not truncated to an aggregate RAM size.");
        ui.small("- -- means no RAM card decodes the address. Guest reads see S-100 open bus FFh; the debugger shows -- because no RAM cell exists there.");
        ui.small("- An underlined byte means multiple RAM cards decode the address but currently drive the same DI value. The overlap remains physically real.");
        ui.small("- !! means responding RAM cards drive different values: real electrical contention. The debugger never chooses one card arbitrarily.");
        ui.small("- Protection belongs to the installed RAM board. Historical boards use their real card-level behavior; the non-historical compatibility card alone retains legacy 1 KiB blocks.");
        ui.small("- Host debugger reads/writes are instrumentation shortcuts to the same physical card storage. They do not fabricate guest CPU cycles and cannot change which card decodes an address.");
        ui.small("- Editing is enabled only for one uniquely mapped RAM card. Overlap and contention must be corrected in the S-100 hardware configuration, not hidden here.");
        ui.small("- Cell markers: EXEC = box, PC(reg) = left line, HL/M = top line, SP = bottom line.");
        ui.small("- Activity overlay: blue = EXECUTE, green = data/stack READ, red = data/stack WRITE. IN/OUT are I/O bus activity, not RAM activity.");
    }

    fn draw_memory_sidebar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        egui::ScrollArea::vertical()
            .id_salt("ram-inspector-sidebar-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                super::collapsible_section(ui, "CURRENT INSTRUCTION", true, |ui| {
                    self.draw_current_instruction(ui)
                });
                ui.separator();
                super::collapsible_section(ui, "Explain selected instruction", false, |ui| {
                    self.draw_instruction_explainer(ui, state)
                });
                ui.separator();
                super::collapsible_section(ui, "Selected byte / editor", true, |ui| {
                    self.draw_memory_editor(ui, state)
                });
                ui.separator();
                super::collapsible_section(ui, "S-100 RAM cards / physical map", true, |ui| {
                    self.draw_physical_card_map(ui, state)
                });
                ui.separator();
                super::collapsible_section(ui, "Memory activity overlay", false, |ui| {
                    self.draw_memory_activity_overlay_controls(ui, state)
                });
                ui.separator();
                super::collapsible_section(ui, "CPU REGISTERS", false, |ui| {
                    self.draw_cpu_registers_sidebar(ui)
                });
                ui.separator();
                super::collapsible_section(ui, "How to read this inspector", false, |ui| {
                    self.draw_memory_help(ui)
                });
            });
    }

    fn draw_memory_viewer_window(&mut self, ctx: &egui::Context, state: &mut MemoryViewerUiState) {
        egui::TopBottomPanel::top("memory-viewer-toolbar")
            .resizable(false)
            .show(ctx, |ui| self.draw_memory_toolbar(ui, state));
        egui::SidePanel::right("memory-viewer-sidebar")
            .resizable(false)
            .exact_width(SIDEBAR_DEFAULT_WIDTH)
            .show(ctx, |ui| self.draw_memory_sidebar(ui, state));
        egui::CentralPanel::default().show(ctx, |ui| {
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
                .with_title("RusTair - S-100 RAM Inspector / Intel 8080 Debugger")
                .with_inner_size([1400.0, 820.0])
                .with_min_inner_size([1200.0, 620.0])
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

pub(super) fn trace_requested(ctx: &egui::Context) -> bool {
    let state = RusTairApp::memory_viewer_state(ctx);
    state.window_open && state.activity_overlay
}

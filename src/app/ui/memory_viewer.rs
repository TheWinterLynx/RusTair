use super::super::{egui, RusTairApp};
use super::execution_position::current_instruction_address;
use crate::cpu8080::{FLAG_AC, FLAG_C, FLAG_P, FLAG_S, FLAG_Z};
use crate::debugger8080::{decode_at, detect_simple_backward_loop, InstructionAt, SimpleLoop};
use crate::explain8080::{explain_instruction, MemoryValue8080};
use crate::machine::{MAX_MEM_SIZE, MEMORY_BOARD_SIZE};
use crate::memory_activity8080::{
    summarize_memory_activity_8080, MemoryActivity8080, MemoryActivityMap8080,
};

const BYTES_PER_ROW: usize = 16;
const ROW_COUNT: usize = MAX_MEM_SIZE / BYTES_PER_ROW;
const ROW_HEIGHT: f32 = 22.0;
const SIDEBAR_DEFAULT_WIDTH: f32 = 410.0;
const CURRENT_INSTRUCTION_LINE_HEIGHT: f32 = 18.0;
const CURRENT_INSTRUCTION_BLOCK_HEIGHT: f32 = 126.0;
const INSTRUCTION_EXPLAINER_HEIGHT: f32 = 230.0;

#[derive(Clone)]
struct MemoryViewerUiState {
    window_open: bool,
    address_input: String,
    selected_address: u16,
    pending_jump: Option<u16>,
    follow_pc: bool,
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
            follow_pc: false,
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
        let pc = self.machine.intel8080_state().pc;
        self.select_memory_address(&mut state, pc, true);
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
        if let Some(byte) = self.machine.peek_memory(address) {
            state.edit_value = byte;
            state.edit_input = format!("{byte:02X}");
        } else {
            state.edit_value = 0;
            state.edit_input = "00".into();
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
            value & 0x0f
        )
    }

    fn draw_register8_cells(ui: &mut egui::Ui, name: &str, value: u8) -> egui::Response {
        ui.strong(name);
        ui.label(egui::RichText::new(Self::grouped_binary8(value)).monospace());
        ui.label(
            egui::RichText::new(format!("${value:02X}"))
                .monospace()
                .strong(),
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

    fn activity_exec_color() -> egui::Color32 { egui::Color32::from_rgb(72, 116, 220) }
    fn activity_read_color() -> egui::Color32 { egui::Color32::from_rgb(68, 174, 104) }
    fn activity_write_color() -> egui::Color32 { egui::Color32::from_rgb(220, 82, 68) }

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
        // WRITE wins ties over READ, and READ wins ties over EXECUTE. A single
        // instruction can both execute at and transfer data through one address;
        // choosing the data transfer makes the tint more informative.
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
        Some(egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha))
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
            (state.activity_show_execute && activity.execute_count != 0, Self::activity_exec_color()),
            (state.activity_show_read && activity.read_count != 0, Self::activity_read_color()),
            (state.activity_show_write && activity.write_count != 0, Self::activity_write_color()),
        ];
        for (index, (visible, color)) in slots.into_iter().enumerate() {
            if !visible {
                continue;
            }
            let top = inner.top() + segment_height * index as f32;
            let bottom = if index == 2 { inner.bottom() } else { top + segment_height - 0.8 };
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

    fn instruction_hover_text(
        &mut self,
        address: u16,
        byte: u8,
        execution_address: u16,
        pc: u16,
        sp: u16,
        hl: u16,
        protected: bool,
        activity: MemoryActivity8080,
    ) -> String {
        let mut hover = format!(
            "{address:04X}h = {byte:02X}h = decimal {byte} = {}",
            Self::ascii_description(byte)
        );
        if address == execution_address {
            hover.push_str(" - EXEC");
        }
        if address == pc {
            hover.push_str(" - PC(reg)");
        }
        if address == sp {
            hover.push_str(" - SP");
        }
        if address == hl {
            hover.push_str(" - HL/M");
        }
        if protected {
            hover.push_str(" - protected 1 KiB block");
        }
        if activity.any() {
            hover.push_str("\n");
            hover.push_str(&Self::activity_hover_text(activity));
        }

        hover.push_str("\n\n");
        let Some(instruction) = self.decode_memory_instruction(address) else {
            hover.push_str("No complete 8080 instruction can be decoded here because one or more instruction bytes are outside installed RAM.");
            return hover;
        };

        if address == execution_address {
            hover.push_str("CPU instruction at EXEC boundary:\n");
        } else {
            hover.push_str("Decode only - if execution started at this byte:\n");
            hover.push_str("This address is not the current EXEC boundary; it may be code, an operand, text or arbitrary data.\n");
        }
        hover.push_str(&format!(
            "{}  {}\n",
            instruction.decoded.bytes_text(instruction.bytes),
            instruction.decoded.text()
        ));
        hover.push_str(&format!("Length: {} byte(s) | {}\n", instruction.decoded.length, instruction.decoded.timing.label()));
        hover.push_str(&format!("Flags affected: {}\n", instruction.decoded.flags.label()));
        hover.push_str(&format!("Memory: {} | I/O: {}\n", instruction.decoded.memory_label(), instruction.decoded.io.label()));
        hover.push_str(&format!("Flow: {}", instruction.decoded.flow_label()));
        if instruction.decoded.undocumented_alias {
            hover.push_str("\nUndocumented Intel 8080 alias accepted by the RusTair cores.");
        }
        hover
    }

    fn draw_register_pair_header(ui: &mut egui::Ui, pair: &str, value: u16, description: &str) {
        ui.horizontal_wrapped(|ui| {
            ui.strong(pair);
            ui.label(egui::RichText::new(format!("${value:04X}")).monospace().strong());
            ui.small(description);
        });
    }

    fn draw_cpu_registers_sidebar(&mut self, ui: &mut egui::Ui) {
        let cpu = self.machine.intel8080_state();
        ui.small("The 8080 can use BC, DE and HL as 16-bit pairs. A and F form the PSW when pushed or popped together.");
        ui.add_space(5.0);

        Self::draw_register_pair_header(ui, "PSW", cpu.af(), "A = accumulator | F = flags");
        egui::Grid::new("ram-cpu-registers-psw").num_columns(7).spacing([6.0, 3.0]).show(ui, |ui| {
            Self::draw_register8_cells(ui, "A", cpu.a);
            ui.separator();
            let f = Self::draw_register8_cells(ui, "F", cpu.flags);
            f.on_hover_text(format!(
                "Flags: S={} Z={} AC={} P={} C={}",
                u8::from(cpu.flags & FLAG_S != 0), u8::from(cpu.flags & FLAG_Z != 0),
                u8::from(cpu.flags & FLAG_AC != 0), u8::from(cpu.flags & FLAG_P != 0),
                u8::from(cpu.flags & FLAG_C != 0)
            ));
            ui.end_row();
        });
        ui.add_space(4.0);

        Self::draw_register_pair_header(ui, "BC", cpu.bc(), "general 16-bit register pair");
        egui::Grid::new("ram-cpu-registers-bc").num_columns(7).spacing([6.0, 3.0]).show(ui, |ui| {
            Self::draw_register8_cells(ui, "B", cpu.b); ui.separator(); Self::draw_register8_cells(ui, "C", cpu.c); ui.end_row();
        });
        ui.add_space(4.0);

        Self::draw_register_pair_header(ui, "DE", cpu.de(), "general 16-bit register pair");
        egui::Grid::new("ram-cpu-registers-de").num_columns(7).spacing([6.0, 3.0]).show(ui, |ui| {
            Self::draw_register8_cells(ui, "D", cpu.d); ui.separator(); Self::draw_register8_cells(ui, "E", cpu.e); ui.end_row();
        });
        ui.add_space(4.0);

        Self::draw_register_pair_header(ui, "HL", cpu.hl(), "address pair | M means memory at [HL]");
        egui::Grid::new("ram-cpu-registers-hl").num_columns(7).spacing([6.0, 3.0]).show(ui, |ui| {
            Self::draw_register8_cells(ui, "H", cpu.h); ui.separator(); Self::draw_register8_cells(ui, "L", cpu.l); ui.end_row();
        });

        ui.separator();
        ui.small("16-BIT CONTROL REGISTERS");
        egui::Grid::new("ram-cpu-registers-16-sidebar").num_columns(3).spacing([6.0, 3.0]).show(ui, |ui| {
            ui.strong("SP").on_hover_text("Stack Pointer - address of the top of the 8080 stack");
            ui.label(egui::RichText::new(Self::grouped_binary16(cpu.sp)).monospace());
            ui.label(egui::RichText::new(format!("${:04X}", cpu.sp)).monospace().strong());
            ui.end_row();
            ui.strong("PC").on_hover_text("Program Counter register - in Cycle Accurate it can point inside the current instruction between machine cycles");
            ui.label(egui::RichText::new(Self::grouped_binary16(cpu.pc)).monospace());
            ui.label(egui::RichText::new(format!("${:04X}", cpu.pc)).monospace().strong());
            ui.end_row();
        });
    }

    fn draw_memory_toolbar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let installed = self.machine.installed_ram_bytes();
        let installed_end = installed.saturating_sub(1);
        let execution_address = current_instruction_address(self);
        let cpu = self.machine.intel8080_state();
        let panel = self.machine.front_panel_state();
        let pc = cpu.pc;
        let sp = cpu.sp;
        let hl = cpu.hl();
        let execution_state = if cpu.halted.unwrap_or(false) {
            if panel.running { "HALTED | RUN latch ON" } else { "HALTED | RUN latch OFF" }
        } else if panel.running { "RUNNING" } else { "STOPPED" };

        ui.horizontal_wrapped(|ui| {
            ui.strong("RAM INSPECTOR / 8080 DEBUGGER");
            ui.separator(); ui.label(execution_state); ui.separator();
            ui.small(format!("cycles {}", cpu.total_t_states.unwrap_or(0))); ui.separator();
            ui.small(format!("RAM {} (0000h-{:04X}h)", self.config.machine.ram_size.label(), installed_end));
            ui.separator(); ui.label("Jump:");
            let response = ui.add_sized(
                [66.0, 22.0],
                egui::TextEdit::singleline(&mut state.address_input).font(egui::TextStyle::Monospace).char_limit(6),
            );
            if response.changed() {
                state.address_input = state.address_input.chars()
                    .filter(|c| c.is_ascii_hexdigit() || matches!(c, 'x' | 'X' | 'h' | 'H'))
                    .collect::<String>().to_uppercase();
            }
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.small_button("Go").clicked() || enter {
                if let Some(address) = Self::parse_memory_address(&state.address_input) {
                    state.follow_pc = false; self.select_memory_address(state, address, true);
                }
            }
            if ui.small_button(format!("EXEC {execution_address:04X}")).clicked() { state.follow_pc = false; self.select_memory_address(state, execution_address, true); }
            if ui.small_button(format!("PC {pc:04X}")).clicked() { state.follow_pc = false; self.select_memory_address(state, pc, true); }
            if ui.small_button(format!("SP {sp:04X}")).clicked() { state.follow_pc = false; self.select_memory_address(state, sp, true); }
            if ui.small_button(format!("HL {hl:04X}")).clicked() { state.follow_pc = false; self.select_memory_address(state, hl, true); }
            if ui.checkbox(&mut state.follow_pc, "Follow EXEC").changed() && state.follow_pc { self.select_memory_address(state, execution_address, true); }
        });
    }

    fn draw_memory_help(&self, ui: &mut egui::Ui) {
        ui.small("- Hover a RAM byte to decode the 8080 instruction that would begin there. Away from EXEC this is explicitly only a possible decode: RAM may contain operands or data.");
        ui.small("- EXEC is the stable current-instruction boundary. In Cycle Accurate, the physical PC register can temporarily point at an operand/next byte while the current instruction is still in flight.");
        ui.small("- The instruction metadata comes from one shared decoder used by debugger analysis, not a second UI-only opcode table.");
        ui.small("- Explain selected instruction is deliberately tied to the selected RAM address so a fast loop cannot resize or flicker the explanation layout.");
        ui.small("- A is the accumulator and F contains the condition flags. PUSH/POP PSW transfers A and F together.");
        ui.small("- B+C, D+E and H+L are the 8080's natural 16-bit register pairs: BC, DE and HL. The first register is the high byte and the second is the low byte.");
        ui.small("- HL is especially important for memory access: register M in 8080 assembly means the byte in memory addressed by HL.");
        ui.small("- Cell markers do not change layout: EXEC = box, PC(reg) = left line, HL/M = top line, SP = bottom line.");
        ui.small("- Activity overlay: blue = EXECUTE, green = data/stack READ, red = data/stack WRITE. The right-edge bar has fixed slots: top EXEC, middle READ, bottom WRITE.");
        ui.small("- I/O instructions such as IN/OUT are not RAM READ/WRITE activity; inspect them in Execution History or the I/O Inspector.");
        ui.small("- READ/WRITE activity means guest data/stack transfers. Opcode/operand fetches are intentionally not counted as READ activity.");
        ui.small("- The Loop Inspector only claims simple straight-line loops ending in one direct backward JMP/Jcc; ambiguous/nested control flow is deliberately rejected.");
        ui.small("- The 8080 address space is 0000h-FFFFh. '--' means that no physical RAM is installed at that address.");
        ui.small("- ADDR is the row base; 00-0F are the hexadecimal byte offsets. ASCII is the printable interpretation of the same 16 bytes.");
        ui.small("- P marks a write-protected 1 KiB block. The block map is a logical protection map, not a literal S-100 card inventory.");
        ui.small("- RAM editing is a debugger feature. Keep 'Respect write protection' enabled unless you deliberately want to force-patch protected memory.");
    }

    fn draw_memory_activity_overlay_controls(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        if ui.checkbox(&mut state.activity_overlay, "Enable READ / WRITE / EXECUTE overlay").changed() {
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
            Self::draw_activity_legend_item(ui, "READ RAM/stack", Self::activity_read_color());
            ui.separator();
            Self::draw_activity_legend_item(ui, "WRITE RAM/stack", Self::activity_write_color());
        });
        ui.small("Every byte cell has one fixed right-edge activity bar: top = EXEC, middle = READ, bottom = WRITE. Multiple slots can be visible at the same time.");
        ui.small("Background tint shows relative frequency. The right-edge slots show presence even when selection/pointer highlighting hides the tint.");
        ui.small("IN/OUT are I/O bus activity, not RAM activity. Use Execution History / I/O Inspector to inspect port transfers.");
        ui.small("Fixed pointer markers: EXEC = box | PC(reg) = left | HL/M = top | SP = bottom. They never insert/remove widgets.");

        if state.activity_overlay {
            let history = self.machine.instruction_trace_snapshot();
            let metadata = self.machine.instruction_trace_metadata();
            let activity = summarize_memory_activity_8080(&history, metadata);
            let selected = activity.get(state.selected_address);
            ui.monospace(format!(
                "Selected ${:04X}: EXEC {} | READ {} | WRITE {} | last #{}",
                state.selected_address,
                selected.execute_count,
                selected.read_count,
                selected.write_count,
                selected.last_sequence().map(|value| value.to_string()).unwrap_or_else(|| "-".into()),
            ));
            ui.small(format!(
                "Retained: {} instruction(s) | {} active address(es) | dropped {}",
                history.len(), activity.active_addresses(), activity.dropped_entries,
            ));
            if activity.sequence_gap {
                ui.small("Trace sequence gap detected: overlay counts are incomplete.");
            } else if activity.dropped_entries != 0 {
                ui.small("Older trace entries were evicted: overlay counts are lower bounds for this generation.");
            }
            ui.horizontal(|ui| {
                if ui.button("Open full Memory Activity").clicked() {
                    self.open_memory_activity(ui.ctx());
                }
                if ui.button("Clear shared activity / history").clicked() {
                    self.machine.clear_instruction_trace();
                }
            });
        } else {
            ui.small("Overlay OFF: RAM Inspector does not request instruction capture on its own.");
        }
    }

    fn draw_memory_block_map(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let installed = self.machine.installed_ram_bytes();
        let selected_block = state.selected_address as usize / MEMORY_BOARD_SIZE;
        let columns = ((ui.available_width() / 70.0).floor() as usize).clamp(2, 4);

        ui.small("Click a 1 KiB block to jump. P = write protected; dimmed blocks are not installed.");
        ui.add_space(3.0);
        egui::Grid::new("ram-protection-block-map").num_columns(columns).spacing([6.0, 3.0]).show(ui, |ui| {
            for block in 0..(MAX_MEM_SIZE / MEMORY_BOARD_SIZE) {
                let start = block * MEMORY_BOARD_SIZE;
                let end = start + MEMORY_BOARD_SIZE - 1;
                let installed_block = start < installed;
                let protected = installed_block && self.machine.memory_is_protected(start as u16);
                let mut label = egui::RichText::new(if protected { format!("P {start:04X}") } else { format!("  {start:04X}") }).monospace();
                if !installed_block { label = label.weak(); }
                let response = ui.selectable_label(selected_block == block, label);
                if response.clicked() { state.follow_pc = false; self.select_memory_address(state, start as u16, true); }
                response.on_hover_text(if installed_block {
                    format!("Installed RAM {start:04X}h-{end:04X}h - {}", if protected { "WRITE PROTECTED" } else { "writable" })
                } else { format!("No RAM installed at {start:04X}h-{end:04X}h") });
                if (block + 1) % columns == 0 { ui.end_row(); }
            }
        });
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
                egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if address == pc {
            painter.line_segment(
                [rect.left_top(), rect.left_bottom()],
                egui::Stroke::new(2.0, ui.visuals().widgets.active.fg_stroke.color),
            );
        }
        if address == hl {
            painter.line_segment(
                [rect.left_top(), rect.right_top()],
                egui::Stroke::new(2.0, egui::Color32::LIGHT_BLUE),
            );
        }
        if address == sp {
            painter.line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                egui::Stroke::new(2.0, egui::Color32::YELLOW),
            );
        }
    }

    fn draw_memory_table(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let execution_address = current_instruction_address(self);
        let cpu = self.machine.intel8080_state();
        let pc = cpu.pc;
        let sp = cpu.sp;
        let hl = cpu.hl();
        if state.follow_pc && state.selected_address != execution_address { self.select_memory_address(state, execution_address, true); }

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

        ui.set_min_width(748.0);
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
            ui.add_sized([54.0, ROW_HEIGHT], egui::Label::new(egui::RichText::new("ADDR").monospace().strong()));
            ui.add_sized([20.0, ROW_HEIGHT], egui::Label::new(egui::RichText::new("P").monospace().strong()))
                .on_hover_text("P = this 1 KiB block is write-protected");
            for column in 0..BYTES_PER_ROW {
                ui.add_sized([28.0, ROW_HEIGHT], egui::Label::new(egui::RichText::new(format!("{column:02X}")).monospace().strong()));
            }
            ui.separator(); ui.label(egui::RichText::new("ASCII").monospace().strong());
        });
        ui.separator();

        let target = state.pending_jump.take();
        let mut scroll = egui::ScrollArea::vertical().id_salt("ram-viewer-scroll").auto_shrink([false, false]).animated(false);
        if let Some(address) = target {
            let target_row = address as usize / BYTES_PER_ROW;
            scroll = scroll.vertical_scroll_offset(target_row.saturating_sub(5) as f32 * ROW_HEIGHT);
        }

        scroll.show_rows(ui, ROW_HEIGHT, ROW_COUNT, |ui, rows| {
            for row in rows {
                let start = row * BYTES_PER_ROW;
                let row_address = start as u16;
                let protected = self.machine.memory_is_protected(row_address);
                let row_contains_exec = (start..start + BYTES_PER_ROW).contains(&(execution_address as usize));
                let row_contains_selected = (start..start + BYTES_PER_ROW).contains(&(state.selected_address as usize));

                ui.horizontal(|ui| {
                    let mut address_text = egui::RichText::new(format!("{start:04X}")).monospace();
                    if row_contains_exec { address_text = address_text.strong(); }
                    if row_contains_selected { address_text = address_text.underline(); }
                    ui.add_sized([54.0, ROW_HEIGHT], egui::Label::new(address_text));
                    ui.add_sized([20.0, ROW_HEIGHT], egui::Label::new(egui::RichText::new(if protected { "P" } else { " " }).monospace()));

                    let selected_fill = ui.visuals().selection.bg_fill;
                    let weak_color = ui.visuals().weak_text_color();
                    let mut ascii = String::with_capacity(BYTES_PER_ROW);

                    for column in 0..BYTES_PER_ROW {
                        let address = (start + column) as u16;
                        let activity = activity_map
                            .as_ref()
                            .map(|map| map.get(address))
                            .unwrap_or_default();
                        let overlay_fill = Self::activity_overlay_fill(state, activity, max_activity_count);

                        match self.machine.peek_memory(address) {
                            Some(byte) => {
                                ascii.push(Self::printable_ascii(byte));
                                let mut text = egui::RichText::new(format!("{byte:02X}")).monospace();
                                if let Some(fill) = overlay_fill { text = text.background_color(fill); }
                                if address == execution_address { text = text.strong(); }
                                if address == state.selected_address { text = text.background_color(selected_fill); }
                                let response = ui.add_sized([28.0, ROW_HEIGHT], egui::Label::new(text).sense(egui::Sense::click()));
                                Self::draw_activity_stripes(ui, response.rect, state, activity);
                                Self::draw_cell_markers(ui, response.rect, address, execution_address, pc, sp, hl);
                                if response.clicked() { state.follow_pc = false; self.select_memory_address(state, address, false); }
                                if response.hovered() {
                                    let hover = self.instruction_hover_text(
                                        address,
                                        byte,
                                        execution_address,
                                        pc,
                                        sp,
                                        hl,
                                        protected,
                                        activity,
                                    );
                                    response.on_hover_text(hover);
                                }
                            }
                            None => {
                                ascii.push(' ');
                                let mut text = egui::RichText::new("--").monospace().color(weak_color);
                                if let Some(fill) = overlay_fill { text = text.background_color(fill); }
                                if address == state.selected_address { text = text.background_color(selected_fill); }
                                let response = ui.add_sized([28.0, ROW_HEIGHT], egui::Label::new(text).sense(egui::Sense::click()));
                                Self::draw_activity_stripes(ui, response.rect, state, activity);
                                Self::draw_cell_markers(ui, response.rect, address, execution_address, pc, sp, hl);
                                if response.clicked() { state.follow_pc = false; self.select_memory_address(state, address, false); }
                                let mut hover = format!("{:04X}h - no RAM installed; guest reads return 00h", address);
                                if activity.any() {
                                    hover.push_str("\n");
                                    hover.push_str(&Self::activity_hover_text(activity));
                                    hover.push_str("\nWRITE activity records attempted bus transfers even though no RAM cell exists here.");
                                }
                                response.on_hover_text(hover);
                            }
                        }
                    }
                    ui.separator(); ui.label(egui::RichText::new(ascii).monospace());
                });
            }
        });
    }

    fn draw_bit_editor(&self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        egui::Grid::new("ram-byte-bit-editor").num_columns(8).spacing([6.0, 2.0]).show(ui, |ui| {
            for bit in (0..8).rev() { ui.label(egui::RichText::new(format!("b{bit}")).monospace().strong()); }
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
            for bit in (0..8).rev() { ui.small(format!("{}", 1u16 << bit)); }
            ui.end_row();
        });
    }

    fn draw_memory_editor(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let address = state.selected_address;
        let current = self.machine.peek_memory(address);

        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("{:04X}h", address)); ui.separator();
            match current {
                Some(byte) => { ui.monospace(format!("{:02X}h", byte)); ui.label(format!("dec {byte}")); ui.monospace(format!("{:08b}", byte)); ui.label(Self::ascii_description(byte)); }
                None => { ui.label("UNINSTALLED"); }
            }
        });

        let Some(current_byte) = current else {
            ui.small("No RAM is fitted at this address. Increase the configured RAM size before editing it.");
            return;
        };

        let protected = self.machine.memory_is_protected(address);
        let block = address as usize / MEMORY_BOARD_SIZE;
        let block_start = block * MEMORY_BOARD_SIZE;
        let block_end = block_start + MEMORY_BOARD_SIZE - 1;
        ui.small(format!("Block {block}: {block_start:04X}h-{block_end:04X}h - {}", if protected { "WRITE PROTECTED" } else { "writable" }));

        let execution_address = current_instruction_address(self);
        let cpu = self.machine.intel8080_state();
        let exec_here = address == execution_address;
        let pc_here = address == cpu.pc;
        let sp_here = address == cpu.sp;
        let hl_here = address == cpu.hl();
        ui.horizontal(|ui| {
            ui.small("Pointers:");
            ui.add_sized([42.0, CURRENT_INSTRUCTION_LINE_HEIGHT], egui::Label::new(egui::RichText::new(if exec_here { "EXEC" } else { "    " }).strong()));
            ui.add_sized([56.0, CURRENT_INSTRUCTION_LINE_HEIGHT], egui::Label::new(egui::RichText::new(if pc_here { "PC(reg)" } else { "       " }).strong()));
            ui.add_sized([32.0, CURRENT_INSTRUCTION_LINE_HEIGHT], egui::Label::new(egui::RichText::new(if sp_here { "SP" } else { "  " }).strong()));
            ui.add_sized([46.0, CURRENT_INSTRUCTION_LINE_HEIGHT], egui::Label::new(egui::RichText::new(if hl_here { "HL/M" } else { "    " }).strong()));
        });

        if let Some(instruction) = self.decode_memory_instruction(address) {
            ui.small(if exec_here {
                format!("Instruction at EXEC: {}", instruction.decoded.text())
            } else {
                format!("Possible decode at selected address: {}", instruction.decoded.text())
            });
        }

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label("New hex:");
            let response = ui.add_sized([58.0, 24.0], egui::TextEdit::singleline(&mut state.edit_input).font(egui::TextStyle::Monospace).char_limit(4));
            if response.changed() {
                state.edit_input = state.edit_input.chars()
                    .filter(|c| c.is_ascii_hexdigit() || matches!(c, 'x' | 'X' | 'h' | 'H'))
                    .collect::<String>().to_uppercase();
                if let Some(value) = Self::parse_memory_byte(&state.edit_input) { state.edit_value = value; }
                state.last_edit_message = None;
            }
            if ui.small_button("Reload").clicked() {
                state.edit_value = current_byte; state.edit_input = format!("{current_byte:02X}"); state.last_edit_message = None;
            }
        });
        ui.small(format!("dec {}  |  {:08b}  |  ASCII {}", state.edit_value, state.edit_value, Self::ascii_description(state.edit_value)));

        self.draw_bit_editor(ui, state);
        ui.separator();
        ui.checkbox(&mut state.respect_protection, "Respect write protection");
        let valid = Self::parse_memory_byte(&state.edit_input).is_some();
        let blocked = protected && state.respect_protection;
        if ui.add_enabled(valid && !blocked, egui::Button::new("Write byte to RAM")).clicked() {
            let written = self.machine.write_memory(address, state.edit_value, state.respect_protection);
            state.last_edit_message = Some(if written {
                format!("Wrote {:02X}h to {:04X}h{}", state.edit_value, address,
                    if protected && !state.respect_protection { " using debugger override" } else { "" })
            } else { "Write rejected by current memory configuration".into() });
        }
        if blocked { ui.small("Uncheck protection only when you deliberately want a debugger override."); }
        else if protected && !state.respect_protection { ui.small("Debugger override active: protection is being bypassed."); }
        if let Some(message) = &state.last_edit_message { ui.small(message); }
        if self.machine.running() { ui.small("Machine is RUNNING; the CPU may overwrite this byte immediately."); }
    }

    fn draw_instruction_explainer(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let address = state.selected_address;
        let execution_address = current_instruction_address(self);
        let cpu = self.machine.intel8080_state();
        let instruction = self.decode_memory_instruction(address);
        let memory_at_hl = match self.machine.peek_memory(cpu.hl()) {
            Some(value) => MemoryValue8080::Known(value),
            None => MemoryValue8080::Unmapped,
        };
        let width = ui.available_width();

        ui.allocate_ui_with_layout(
            egui::vec2(width, INSTRUCTION_EXPLAINER_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("ram-instruction-explainer-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let Some(instruction) = instruction.as_ref() else {
                            ui.label("No complete 8080 instruction can be decoded at the selected address.");
                            return;
                        };

                        ui.horizontal(|ui| {
                            ui.strong(format!("${address:04X}"));
                            ui.monospace(instruction.decoded.bytes_text(instruction.bytes));
                            ui.monospace(instruction.decoded.text());
                            if address == execution_address { ui.strong("EXEC"); }
                            if address == cpu.pc { ui.small("PC(reg)"); }
                        });
                        if address != execution_address {
                            ui.small("Decode context: selected RAM address. Live register values below still come from the current CPU state.");
                        }
                        ui.separator();

                        let explanation = explain_instruction(&instruction.decoded, cpu, memory_at_hl);
                        ui.label(&explanation.summary);
                        ui.add_space(4.0);
                        egui::Grid::new("ram-instruction-explanation-grid")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.strong("Reads"); ui.label(&explanation.reads); ui.end_row();
                                ui.strong("Writes"); ui.label(&explanation.writes); ui.end_row();
                                ui.strong("Flags"); ui.label(&explanation.flags); ui.end_row();
                                ui.strong("Timing"); ui.label(instruction.decoded.timing.label()); ui.end_row();
                                ui.strong("Memory"); ui.label(&explanation.memory); ui.end_row();
                                ui.strong("I/O"); ui.label(&explanation.io); ui.end_row();
                                ui.strong("Flow"); ui.label(&explanation.flow); ui.end_row();
                            });
                        for line in &explanation.context {
                            ui.small(line);
                        }
                    });
            },
        );
    }

    fn draw_current_instruction_side(&mut self, ui: &mut egui::Ui) {
        let execution_address = current_instruction_address(self);
        let cpu = self.machine.intel8080_state();
        let instruction = self.decode_memory_instruction(execution_address);
        let loop_info = self.current_simple_loop();
        let width = ui.available_width();

        ui.allocate_ui_with_layout(
            egui::vec2(width, CURRENT_INSTRUCTION_BLOCK_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let add_row = |ui: &mut egui::Ui, text: egui::RichText| {
                    ui.add_sized(
                        [ui.available_width(), CURRENT_INSTRUCTION_LINE_HEIGHT],
                        egui::Label::new(text),
                    );
                };

                let Some(instruction) = instruction.as_ref() else {
                    add_row(ui, egui::RichText::new(format!("${execution_address:04X}  UNMAPPED")).monospace().strong());
                    add_row(ui, egui::RichText::new("--").monospace().weak());
                    add_row(ui, egui::RichText::new("Timing: - | flags: -").small());
                    add_row(ui, egui::RichText::new("Memory: - | I/O: -").small());
                    add_row(ui, egui::RichText::new("Flow: -").small());
                    add_row(ui, egui::RichText::new("Loop: -").small());
                    return;
                };

                ui.horizontal(|ui| {
                    ui.add_sized(
                        [64.0, CURRENT_INSTRUCTION_LINE_HEIGHT],
                        egui::Label::new(egui::RichText::new(format!("${execution_address:04X}")).monospace().strong()),
                    );
                    ui.add_sized(
                        [ui.available_width(), CURRENT_INSTRUCTION_LINE_HEIGHT],
                        egui::Label::new(egui::RichText::new(instruction.decoded.text()).monospace().strong()),
                    );
                });
                add_row(
                    ui,
                    egui::RichText::new(instruction.decoded.bytes_text(instruction.bytes)).monospace().weak(),
                );
                add_row(
                    ui,
                    egui::RichText::new(format!(
                        "{} | flags {}{} | PC(reg) ${:04X}",
                        instruction.decoded.timing.label(),
                        instruction.decoded.flags.label(),
                        if instruction.decoded.undocumented_alias { " | undocumented alias" } else { "" },
                        cpu.pc,
                    )).small(),
                );
                add_row(
                    ui,
                    egui::RichText::new(format!(
                        "Memory: {} | I/O: {}",
                        instruction.decoded.memory_label(),
                        instruction.decoded.io.label()
                    )).small(),
                );
                let flow_text = if let Some(condition) = instruction.decoded.control_flow.condition() {
                    format!(
                        "Flow: {} | {} -> {}",
                        instruction.decoded.flow_label(),
                        condition.label(),
                        if condition.evaluate(cpu.flags) { "TAKEN" } else { "NOT TAKEN" }
                    )
                } else {
                    format!("Flow: {}", instruction.decoded.flow_label())
                };
                add_row(ui, egui::RichText::new(flow_text).small());

                ui.horizontal(|ui| {
                    let label_width = (width - 132.0).max(120.0);
                    if let Some(loop_info) = loop_info.as_ref() {
                        let loop_state = if execution_address == loop_info.back_edge {
                            if loop_info.branch_taken_now { "TAKEN now" } else { "EXIT now" }
                        } else {
                            "inside loop"
                        };
                        ui.add_sized(
                            [label_width, 22.0],
                            egui::Label::new(
                                egui::RichText::new(format!(
                                    "Loop: {:04X}-{:04X} | {loop_state}",
                                    loop_info.start,
                                    loop_info.back_edge,
                                )).small(),
                            ),
                        );
                        if ui.add_sized([124.0, 22.0], egui::Button::new("Loop Inspector")).clicked() {
                            self.open_loop_inspector(ui.ctx(), loop_info.clone());
                        }
                    } else {
                        ui.add_sized(
                            [label_width, 22.0],
                            egui::Label::new(egui::RichText::new("Loop: -").small()),
                        );
                        ui.add_enabled_ui(false, |ui| {
                            ui.add_sized([124.0, 22.0], egui::Button::new("Loop Inspector"));
                        });
                    }
                });
            },
        );
    }

    fn draw_memory_sidebar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        egui::ScrollArea::vertical().id_salt("ram-inspector-sidebar-scroll").auto_shrink([false, false]).show(ui, |ui| {
            super::collapsible_section(ui, "CURRENT INSTRUCTION", true, |ui| self.draw_current_instruction_side(ui));
            ui.separator();
            super::collapsible_section(ui, "Explain selected instruction", true, |ui| self.draw_instruction_explainer(ui, state));
            ui.separator();
            super::collapsible_section(ui, "Selected byte / editor", false, |ui| self.draw_memory_editor(ui, state));
            ui.separator();
            super::collapsible_section(ui, "Memory activity overlay", false, |ui| self.draw_memory_activity_overlay_controls(ui, state));
            ui.separator();
            super::collapsible_section(ui, "1 KiB protection map", false, |ui| self.draw_memory_block_map(ui, state));
            ui.separator();
            super::collapsible_section(ui, "CPU REGISTERS", true, |ui| self.draw_cpu_registers_sidebar(ui));
            ui.separator();
            super::collapsible_section(ui, "How to read this inspector", false, |ui| self.draw_memory_help(ui));
        });
    }

    fn draw_memory_viewer_window(&mut self, ctx: &egui::Context, state: &mut MemoryViewerUiState) {
        egui::TopBottomPanel::top("memory-viewer-toolbar").resizable(false).show(ctx, |ui| self.draw_memory_toolbar(ui, state));
        egui::SidePanel::right("memory-viewer-sidebar")
            .resizable(false)
            .exact_width(SIDEBAR_DEFAULT_WIDTH)
            .show(ctx, |ui| self.draw_memory_sidebar(ui, state));
        egui::CentralPanel::default().show(ctx, |ui| { self.draw_memory_table(ui, state); });
    }

    pub(in crate::app) fn show_memory_viewer_viewport(&mut self, parent_ctx: &egui::Context) {
        let mut state = Self::memory_viewer_state(parent_ctx);
        if !state.window_open { return; }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-memory-viewer"),
            egui::ViewportBuilder::default()
                .with_title("RusTair - RAM Inspector / 8080 Debugger")
                .with_inner_size([1380.0, 800.0])
                .with_min_inner_size([1190.0, 600.0])
                .with_resizable(true),
            |memory_ctx, _class| {
                self.draw_memory_viewer_window(memory_ctx, &mut state);
                if memory_ctx.input(|i| i.viewport().close_requested()) { state.window_open = false; }
            },
        );

        Self::store_memory_viewer_state(parent_ctx, state);
    }
}

pub(super) fn trace_requested(ctx: &egui::Context) -> bool {
    let state = RusTairApp::memory_viewer_state(ctx);
    state.window_open && state.activity_overlay
}

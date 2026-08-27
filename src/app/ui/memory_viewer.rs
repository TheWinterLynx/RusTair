use super::super::{egui, RusTairApp};
use crate::cpu8080::{FLAG_AC, FLAG_C, FLAG_P, FLAG_S, FLAG_Z};
use crate::debugger8080::{decode_at, detect_simple_backward_loop, InstructionAt, SimpleLoop};
use crate::explain8080::{explain_instruction, MemoryValue8080};
use crate::machine::{MAX_MEM_SIZE, MEMORY_BOARD_SIZE};

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
        let cpu = self.machine.intel8080_state();
        detect_simple_backward_loop(
            |address| self.machine.peek_memory(address),
            cpu.pc,
            cpu.flags,
        )
    }

    fn instruction_hover_text(
        &mut self,
        address: u16,
        byte: u8,
        pc: u16,
        sp: u16,
        protected: bool,
    ) -> String {
        let mut hover = format!(
            "{address:04X}h = {byte:02X}h = decimal {byte} = {}",
            Self::ascii_description(byte)
        );
        if address == pc {
            hover.push_str(" - PC");
        }
        if address == sp {
            hover.push_str(" - SP");
        }
        if protected {
            hover.push_str(" - protected 1 KiB block");
        }

        hover.push_str("\n\n");
        let Some(instruction) = self.decode_memory_instruction(address) else {
            hover.push_str("No complete 8080 instruction can be decoded here because one or more instruction bytes are outside installed RAM.");
            return hover;
        };

        if address == pc {
            hover.push_str("CPU instruction at PC:\n");
        } else {
            hover.push_str("Decode only - if execution started at this byte:\n");
            hover.push_str("This address is not the current PC; it may be code, an operand, text or arbitrary data.\n");
        }
        hover.push_str(&format!(
            "{}  {}\n",
            instruction.decoded.bytes_text(instruction.bytes),
            instruction.decoded.text()
        ));
        hover.push_str(&format!("Length: {} byte(s) | {}\n", instruction.decoded.length, instruction.decoded.timing.label()));
        hover.push_str(&format!("Flags affected: {}\n", instruction.decoded.flags.label()));
        hover.push_str(&format!("Memory: {} | I/O: {}\n", instruction.decoded.memory.label(), instruction.decoded.io.label()));
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
        ui.strong("CPU REGISTERS");
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
            ui.strong("PC").on_hover_text("Program Counter - address of the next instruction to fetch");
            ui.label(egui::RichText::new(Self::grouped_binary16(cpu.pc)).monospace());
            ui.label(egui::RichText::new(format!("${:04X}", cpu.pc)).monospace().strong());
            ui.end_row();
        });
    }

    fn draw_memory_toolbar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let installed = self.machine.installed_ram_bytes();
        let installed_end = installed.saturating_sub(1);
        let cpu = self.machine.intel8080_state();
        let panel = self.machine.front_panel_state();
        let pc = cpu.pc;
        let sp = cpu.sp;
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
            if ui.small_button(format!("PC {pc:04X}")).clicked() { state.follow_pc = false; self.select_memory_address(state, pc, true); }
            if ui.small_button(format!("SP {sp:04X}")).clicked() { state.follow_pc = false; self.select_memory_address(state, sp, true); }
            if ui.checkbox(&mut state.follow_pc, "Follow PC").changed() && state.follow_pc { self.select_memory_address(state, pc, true); }
        });
    }

    fn draw_memory_help(&self, ui: &mut egui::Ui) {
        ui.small("- Hover a RAM byte to decode the 8080 instruction that would begin there. Away from PC this is explicitly only a possible decode: RAM may contain operands or data.");
        ui.small("- The instruction metadata comes from one shared decoder used by debugger analysis, not a second UI-only opcode table.");
        ui.small("- Explain selected instruction is deliberately tied to the selected RAM address so a fast loop cannot resize or flicker the explanation layout.");
        ui.small("- A is the accumulator and F contains the condition flags. PUSH/POP PSW transfers A and F together.");
        ui.small("- B+C, D+E and H+L are the 8080's natural 16-bit register pairs: BC, DE and HL. The first register is the high byte and the second is the low byte.");
        ui.small("- HL is especially important for memory access: register M in 8080 assembly means the byte in memory addressed by HL.");
        ui.small("- PC is the Program Counter (next instruction address). SP is the Stack Pointer.");
        ui.small("- The Loop Inspector only claims simple straight-line loops ending in one direct backward JMP/Jcc; ambiguous/nested control flow is deliberately rejected.");
        ui.small("- The 8080 address space is 0000h-FFFFh. '--' means that no physical RAM is installed at that address.");
        ui.small("- ADDR is the row base; 00-0F are the hexadecimal byte offsets. ASCII is the printable interpretation of the same 16 bytes.");
        ui.small("- PC bytes are highlighted; SP is underlined when it falls on a visible byte.");
        ui.small("- P marks a write-protected 1 KiB block. The block map is a logical protection map, not a literal S-100 card inventory.");
        ui.small("- RAM editing is a debugger feature. Keep 'Respect write protection' enabled unless you deliberately want to force-patch protected memory.");
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

    fn draw_memory_table(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let cpu = self.machine.intel8080_state();
        let pc = cpu.pc;
        let sp = cpu.sp;
        if state.follow_pc && state.selected_address != pc { self.select_memory_address(state, pc, true); }

        ui.set_min_width(748.0);
        ui.spacing_mut().item_spacing.x = 2.0;
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
                let row_contains_pc = (start..start + BYTES_PER_ROW).contains(&(pc as usize));
                let row_contains_selected = (start..start + BYTES_PER_ROW).contains(&(state.selected_address as usize));

                ui.horizontal(|ui| {
                    let mut address_text = egui::RichText::new(format!("{start:04X}")).monospace();
                    if row_contains_pc { address_text = address_text.strong(); }
                    if row_contains_selected { address_text = address_text.underline(); }
                    ui.add_sized([54.0, ROW_HEIGHT], egui::Label::new(address_text));
                    ui.add_sized([20.0, ROW_HEIGHT], egui::Label::new(egui::RichText::new(if protected { "P" } else { " " }).monospace()));

                    let selected_fill = ui.visuals().selection.bg_fill;
                    let pc_fill = ui.visuals().widgets.active.bg_fill;
                    let weak_color = ui.visuals().weak_text_color();
                    let mut ascii = String::with_capacity(BYTES_PER_ROW);

                    for column in 0..BYTES_PER_ROW {
                        let address = (start + column) as u16;
                        match self.machine.peek_memory(address) {
                            Some(byte) => {
                                ascii.push(Self::printable_ascii(byte));
                                let mut text = egui::RichText::new(format!("{byte:02X}")).monospace();
                                if address == sp { text = text.underline(); }
                                if address == pc { text = text.strong().background_color(pc_fill); }
                                if address == state.selected_address { text = text.background_color(selected_fill); }
                                let response = ui.add_sized([28.0, ROW_HEIGHT], egui::Label::new(text).sense(egui::Sense::click()));
                                if response.clicked() { state.follow_pc = false; self.select_memory_address(state, address, false); }
                                if response.hovered() {
                                    let hover = self.instruction_hover_text(address, byte, pc, sp, protected);
                                    response.on_hover_text(hover);
                                }
                            }
                            None => {
                                ascii.push(' ');
                                ui.add_sized([28.0, ROW_HEIGHT], egui::Label::new(egui::RichText::new("--").monospace().color(weak_color)))
                                    .on_hover_text(format!("{:04X}h - no RAM installed; guest reads return 00h", address));
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

        let cpu = self.machine.intel8080_state();
        let pc_here = address == cpu.pc;
        let sp_here = address == cpu.sp;
        ui.horizontal(|ui| {
            ui.small("Pointers:");
            ui.add_sized([28.0, CURRENT_INSTRUCTION_LINE_HEIGHT], egui::Label::new(egui::RichText::new(if pc_here { "PC" } else { "  " }).strong()));
            ui.add_sized([28.0, CURRENT_INSTRUCTION_LINE_HEIGHT], egui::Label::new(egui::RichText::new(if sp_here { "SP" } else { "  " }).strong()));
        });

        if let Some(instruction) = self.decode_memory_instruction(address) {
            ui.small(if pc_here {
                format!("Instruction at PC: {}", instruction.decoded.text())
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
                            if address == cpu.pc { ui.strong("PC"); }
                        });
                        if address != cpu.pc {
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
        let cpu = self.machine.intel8080_state();
        let pc = cpu.pc;
        let instruction = self.decode_memory_instruction(pc);
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
                    add_row(ui, egui::RichText::new(format!("${pc:04X}  UNMAPPED")).monospace().strong());
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
                        egui::Label::new(egui::RichText::new(format!("${pc:04X}")).monospace().strong()),
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
                        "{} | flags {}{}",
                        instruction.decoded.timing.label(),
                        instruction.decoded.flags.label(),
                        if instruction.decoded.undocumented_alias { " | undocumented alias" } else { "" }
                    )).small(),
                );
                add_row(
                    ui,
                    egui::RichText::new(format!(
                        "Memory: {} | I/O: {}",
                        instruction.decoded.memory.label(),
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
                    if let Some(loop_info) = loop_info.as_ref() {
                        let loop_state = if cpu.pc == loop_info.back_edge {
                            if loop_info.branch_taken_now { "TAKEN now" } else { "EXIT now" }
                        } else {
                            "inside loop"
                        };
                        let label_width = (width - 132.0).max(120.0);
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
                            [ui.available_width(), 22.0],
                            egui::Label::new(egui::RichText::new("Loop: -").small()),
                        );
                    }
                });
            },
        );
    }

    fn draw_memory_sidebar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        egui::ScrollArea::vertical().id_salt("ram-inspector-sidebar-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.strong("CURRENT INSTRUCTION");
            self.draw_current_instruction_side(ui);
            ui.separator();
            egui::CollapsingHeader::new("Explain selected instruction").default_open(true).show(ui, |ui| self.draw_instruction_explainer(ui, state));
            ui.separator();
            egui::CollapsingHeader::new("Selected byte / editor").default_open(false).show(ui, |ui| self.draw_memory_editor(ui, state));
            ui.separator();
            egui::CollapsingHeader::new("1 KiB protection map").default_open(false).show(ui, |ui| self.draw_memory_block_map(ui, state));
            ui.separator();
            egui::Frame::group(ui.style()).show(ui, |ui| { self.draw_cpu_registers_sidebar(ui); });
            ui.separator();
            egui::CollapsingHeader::new("How to read this inspector").default_open(false).show(ui, |ui| self.draw_memory_help(ui));
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

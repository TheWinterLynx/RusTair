use super::super::{egui, RusTairApp};
use crate::cpu8080::{FLAG_AC, FLAG_C, FLAG_P, FLAG_S, FLAG_Z};
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
        let pc = self.machine.cpu.pc;
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
        &self,
        state: &mut MemoryViewerUiState,
        address: u16,
        jump: bool,
    ) {
        state.selected_address = address;
        state.address_input = format!("{address:04X}");
        if let Some(byte) = self.machine.bus.peek_memory(address) {
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

    fn draw_register8_inline(ui: &mut egui::Ui, name: &str, value: u8) -> egui::Response {
        ui.horizontal(|ui| {
            ui.strong(name);
            ui.label(egui::RichText::new(Self::grouped_binary8(value)).monospace());
            ui.label(
                egui::RichText::new(format!("${value:02X}"))
                    .monospace()
                    .strong(),
            );
        })
        .response
    }

    fn instruction_word(lo: u8, hi: u8) -> u16 {
        lo as u16 | ((hi as u16) << 8)
    }

    fn disassemble_8080(op: u8, b1: u8, b2: u8) -> (String, usize) {
        const REG: [&str; 8] = ["B", "C", "D", "E", "H", "L", "M", "A"];
        const RP: [&str; 4] = ["B", "D", "H", "SP"];
        const RP_PUSH: [&str; 4] = ["B", "D", "H", "PSW"];
        const COND: [&str; 8] = ["NZ", "Z", "NC", "C", "PO", "PE", "P", "M"];
        const ALU: [&str; 8] = ["ADD", "ADC", "SUB", "SBB", "ANA", "XRA", "ORA", "CMP"];

        if (0x40..=0x7f).contains(&op) {
            if op == 0x76 {
                return ("HLT".into(), 1);
            }
            let dst = REG[((op >> 3) & 7) as usize];
            let src = REG[(op & 7) as usize];
            return (format!("MOV {dst},{src}"), 1);
        }

        if (0x80..=0xbf).contains(&op) {
            let alu = ALU[((op >> 3) & 7) as usize];
            let src = REG[(op & 7) as usize];
            return (format!("{alu} {src}"), 1);
        }

        if op & 0xc7 == 0x04 {
            return (format!("INR {}", REG[((op >> 3) & 7) as usize]), 1);
        }
        if op & 0xc7 == 0x05 {
            return (format!("DCR {}", REG[((op >> 3) & 7) as usize]), 1);
        }
        if op & 0xc7 == 0x06 {
            return (
                format!("MVI {},${b1:02X}", REG[((op >> 3) & 7) as usize]),
                2,
            );
        }
        if op & 0xcf == 0x01 {
            let value = Self::instruction_word(b1, b2);
            return (
                format!("LXI {},${value:04X}", RP[((op >> 4) & 3) as usize]),
                3,
            );
        }
        if op & 0xcf == 0x03 {
            return (format!("INX {}", RP[((op >> 4) & 3) as usize]), 1);
        }
        if op & 0xcf == 0x0b {
            return (format!("DCX {}", RP[((op >> 4) & 3) as usize]), 1);
        }
        if op & 0xcf == 0x09 {
            return (format!("DAD {}", RP[((op >> 4) & 3) as usize]), 1);
        }
        if op & 0xc7 == 0xc0 {
            return (format!("R{}", COND[((op >> 3) & 7) as usize]), 1);
        }
        if op & 0xc7 == 0xc2 {
            let value = Self::instruction_word(b1, b2);
            return (
                format!("J{} ${value:04X}", COND[((op >> 3) & 7) as usize]),
                3,
            );
        }
        if op & 0xc7 == 0xc4 {
            let value = Self::instruction_word(b1, b2);
            return (
                format!("C{} ${value:04X}", COND[((op >> 3) & 7) as usize]),
                3,
            );
        }
        if op & 0xcf == 0xc1 {
            return (
                format!("POP {}", RP_PUSH[((op >> 4) & 3) as usize]),
                1,
            );
        }
        if op & 0xcf == 0xc5 {
            return (
                format!("PUSH {}", RP_PUSH[((op >> 4) & 3) as usize]),
                1,
            );
        }
        if op & 0xc7 == 0xc7 {
            return (format!("RST {}", (op >> 3) & 7), 1);
        }

        let word = Self::instruction_word(b1, b2);
        match op {
            0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => ("NOP".into(), 1),
            0x02 => ("STAX B".into(), 1),
            0x07 => ("RLC".into(), 1),
            0x0a => ("LDAX B".into(), 1),
            0x0f => ("RRC".into(), 1),
            0x12 => ("STAX D".into(), 1),
            0x17 => ("RAL".into(), 1),
            0x1a => ("LDAX D".into(), 1),
            0x1f => ("RAR".into(), 1),
            0x22 => (format!("SHLD ${word:04X}"), 3),
            0x27 => ("DAA".into(), 1),
            0x2a => (format!("LHLD ${word:04X}"), 3),
            0x2f => ("CMA".into(), 1),
            0x32 => (format!("STA ${word:04X}"), 3),
            0x37 => ("STC".into(), 1),
            0x3a => (format!("LDA ${word:04X}"), 3),
            0x3f => ("CMC".into(), 1),
            0xc3 | 0xcb => (format!("JMP ${word:04X}"), 3),
            0xc6 => (format!("ADI ${b1:02X}"), 2),
            0xc9 | 0xd9 => ("RET".into(), 1),
            0xcd | 0xdd | 0xed | 0xfd => (format!("CALL ${word:04X}"), 3),
            0xce => (format!("ACI ${b1:02X}"), 2),
            0xd3 => (format!("OUT ${b1:02X}"), 2),
            0xd6 => (format!("SUI ${b1:02X}"), 2),
            0xdb => (format!("IN ${b1:02X}"), 2),
            0xde => (format!("SBI ${b1:02X}"), 2),
            0xe3 => ("XTHL".into(), 1),
            0xe6 => (format!("ANI ${b1:02X}"), 2),
            0xe9 => ("PCHL".into(), 1),
            0xeb => ("XCHG".into(), 1),
            0xee => (format!("XRI ${b1:02X}"), 2),
            0xf3 => ("DI".into(), 1),
            0xf6 => (format!("ORI ${b1:02X}"), 2),
            0xf9 => ("SPHL".into(), 1),
            0xfb => ("EI".into(), 1),
            0xfe => (format!("CPI ${b1:02X}"), 2),
            _ => (format!("DB ${op:02X}"), 1),
        }
    }

    fn current_instruction(&self) -> (String, String) {
        let pc = self.machine.cpu.pc;
        let Some(op) = self.machine.bus.peek_memory(pc) else {
            return ("UNMAPPED".into(), "--".into());
        };
        let b1 = self
            .machine
            .bus
            .peek_memory(pc.wrapping_add(1))
            .unwrap_or(0);
        let b2 = self
            .machine
            .bus
            .peek_memory(pc.wrapping_add(2))
            .unwrap_or(0);
        let (text, len) = Self::disassemble_8080(op, b1, b2);
        let bytes = match len {
            1 => format!("{op:02X}"),
            2 => format!("{op:02X} {b1:02X}"),
            _ => format!("{op:02X} {b1:02X} {b2:02X}"),
        };
        (text, bytes)
    }

    fn draw_cpu_registers_compact(&self, ui: &mut egui::Ui) {
        let cpu = &self.machine.cpu;
        ui.horizontal_wrapped(|ui| {
            let values = [
                ("A", cpu.a),
                ("F", cpu.f),
                ("B", cpu.b),
                ("C", cpu.c),
                ("D", cpu.d),
                ("E", cpu.e),
                ("H", cpu.h),
                ("L", cpu.l),
            ];
            for (index, (name, value)) in values.into_iter().enumerate() {
                let response = Self::draw_register8_inline(ui, name, value);
                if name == "F" {
                    response.on_hover_text(format!(
                        "Flags: S={} Z={} AC={} P={} C={}",
                        u8::from(cpu.f & FLAG_S != 0),
                        u8::from(cpu.f & FLAG_Z != 0),
                        u8::from(cpu.f & FLAG_AC != 0),
                        u8::from(cpu.f & FLAG_P != 0),
                        u8::from(cpu.f & FLAG_C != 0)
                    ));
                }
                if index != values.len() - 1 {
                    ui.separator();
                }
            }
        });

        let (instruction, bytes) = self.current_instruction();
        ui.horizontal_wrapped(|ui| {
            ui.strong("SP");
            ui.label(egui::RichText::new(Self::grouped_binary16(cpu.sp)).monospace());
            ui.label(egui::RichText::new(format!("${:04X}", cpu.sp)).monospace().strong());
            ui.separator();
            ui.strong("PC");
            ui.label(egui::RichText::new(Self::grouped_binary16(cpu.pc)).monospace());
            ui.label(egui::RichText::new(format!("${:04X}", cpu.pc)).monospace().strong());
            ui.separator();
            ui.strong("NEXT");
            ui.label(egui::RichText::new(instruction).monospace().strong());
            ui.small(egui::RichText::new(bytes).monospace().weak());
        });
    }

    fn draw_memory_toolbar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let installed = self.machine.installed_ram_bytes();
        let installed_end = installed.saturating_sub(1);
        let pc = self.machine.cpu.pc;
        let sp = self.machine.cpu.sp;

        ui.horizontal_wrapped(|ui| {
            ui.strong("RAM INSPECTOR / CPU REGISTERS");
            ui.separator();
            ui.label(if self.machine.running { "RUNNING" } else { "STOPPED" });
            if self.machine.cpu.halted {
                ui.separator();
                ui.label("HLT");
            }
            ui.separator();
            ui.small(format!("cycles {}", self.machine.cpu.cycles));
            ui.separator();
            ui.small(format!(
                "RAM {} (0000h–{:04X}h)",
                self.config.machine.ram_size.label(),
                installed_end
            ));
            ui.separator();
            ui.label("Jump:");
            let response = ui.add_sized(
                [66.0, 22.0],
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
                    state.follow_pc = false;
                    self.select_memory_address(state, address, true);
                }
            }
            if ui.small_button(format!("PC {pc:04X}")).clicked() {
                state.follow_pc = false;
                self.select_memory_address(state, pc, true);
            }
            if ui.small_button(format!("SP {sp:04X}")).clicked() {
                state.follow_pc = false;
                self.select_memory_address(state, sp, true);
            }
            if ui.checkbox(&mut state.follow_pc, "Follow PC").changed() && state.follow_pc {
                self.select_memory_address(state, pc, true);
            }
        });
        ui.separator();
        self.draw_cpu_registers_compact(ui);
    }

    fn draw_memory_help(&self, ui: &mut egui::Ui) {
        ui.small("• Registers A/F/B/C/D/E/H/L are 8-bit; PC and SP are 16-bit. Values are shown in binary and hexadecimal.");
        ui.small("• NEXT is the Intel 8080 instruction currently addressed by PC. Reading it here is non-invasive.");
        ui.small("• The 8080 address space is 0000h–FFFFh. '--' means that no physical RAM is installed at that address.");
        ui.small("• ADDR is the row base; 00–0F are the hexadecimal byte offsets. ASCII is the printable interpretation of the same 16 bytes.");
        ui.small("• PC bytes are highlighted; SP is underlined when it falls on a visible byte.");
        ui.small("• P marks a write-protected 1 KiB block. The block map is a logical protection map, not a literal S-100 card inventory.");
        ui.small("• RAM editing is a debugger feature. Keep 'Respect write protection' enabled unless you deliberately want to force-patch protected memory.");
    }

    fn draw_memory_block_map(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let installed = self.machine.installed_ram_bytes();
        let selected_block = state.selected_address as usize / MEMORY_BOARD_SIZE;
        let columns = ((ui.available_width() / 70.0).floor() as usize).clamp(2, 4);

        ui.small("Click a 1 KiB block to jump. P = write protected; dimmed blocks are not installed.");
        ui.add_space(3.0);
        egui::Grid::new("ram-protection-block-map")
            .num_columns(columns)
            .spacing([6.0, 3.0])
            .show(ui, |ui| {
                for block in 0..(MAX_MEM_SIZE / MEMORY_BOARD_SIZE) {
                    let start = block * MEMORY_BOARD_SIZE;
                    let end = start + MEMORY_BOARD_SIZE - 1;
                    let installed_block = start < installed;
                    let protected = installed_block && self.machine.bus.is_protected(start as u16);
                    let mut label = egui::RichText::new(if protected {
                        format!("P {start:04X}")
                    } else {
                        format!("  {start:04X}")
                    })
                    .monospace();
                    if !installed_block {
                        label = label.weak();
                    }
                    let response = ui.selectable_label(selected_block == block, label);
                    if response.clicked() {
                        state.follow_pc = false;
                        self.select_memory_address(state, start as u16, true);
                    }
                    response.on_hover_text(if installed_block {
                        format!(
                            "Installed RAM {start:04X}h–{end:04X}h — {}",
                            if protected { "WRITE PROTECTED" } else { "writable" }
                        )
                    } else {
                        format!("No RAM installed at {start:04X}h–{end:04X}h")
                    });
                    if (block + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn draw_memory_table(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let pc = self.machine.cpu.pc;
        let sp = self.machine.cpu.sp;
        if state.follow_pc && state.selected_address != pc {
            self.select_memory_address(state, pc, true);
        }

        ui.set_min_width(748.0);
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            ui.add_sized(
                [54.0, ROW_HEIGHT],
                egui::Label::new(egui::RichText::new("ADDR").monospace().strong()),
            );
            ui.add_sized(
                [20.0, ROW_HEIGHT],
                egui::Label::new(egui::RichText::new("P").monospace().strong()),
            )
            .on_hover_text("P = this 1 KiB block is write-protected");
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
                let row_contains_selected =
                    (start..start + BYTES_PER_ROW).contains(&(state.selected_address as usize));

                ui.horizontal(|ui| {
                    let mut address_text = egui::RichText::new(format!("{start:04X}")).monospace();
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
                                let mut text = egui::RichText::new(format!("{byte:02X}")).monospace();
                                if address == sp {
                                    text = text.underline();
                                }
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
                                    self.select_memory_address(state, address, false);
                                }
                                let mut hover = format!(
                                    "{:04X}h = {:02X}h = decimal {} = {}",
                                    address,
                                    byte,
                                    byte,
                                    Self::ascii_description(byte)
                                );
                                if address == pc {
                                    hover.push_str(" — PC");
                                }
                                if address == sp {
                                    hover.push_str(" — SP");
                                }
                                if protected {
                                    hover.push_str(" — protected 1 KiB block");
                                }
                                response.on_hover_text(hover);
                            }
                            None => {
                                ascii.push(' ');
                                ui.add_sized(
                                    [28.0, ROW_HEIGHT],
                                    egui::Label::new(
                                        egui::RichText::new("--").monospace().color(weak_color),
                                    ),
                                )
                                .on_hover_text(format!(
                                    "{:04X}h — no RAM installed; guest reads return 00h",
                                    address
                                ));
                            }
                        }
                    }
                    ui.separator();
                    ui.label(egui::RichText::new(ascii).monospace());
                });
            }
        });
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
                for bit in (0..8).rev() {
                    ui.small(format!("{}", 1u16 << bit));
                }
                ui.end_row();
            });
    }

    fn draw_memory_editor(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        let address = state.selected_address;
        let current = self.machine.bus.peek_memory(address);

        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("{:04X}h", address));
            ui.separator();
            match current {
                Some(byte) => {
                    ui.monospace(format!("{:02X}h", byte));
                    ui.label(format!("dec {byte}"));
                    ui.monospace(format!("{:08b}", byte));
                    ui.label(Self::ascii_description(byte));
                }
                None => {
                    ui.label("UNINSTALLED");
                }
            }
        });

        let Some(current_byte) = current else {
            ui.small("No RAM is fitted at this address. Increase the configured RAM size before editing it.");
            return;
        };

        let protected = self.machine.bus.is_protected(address);
        let block = address as usize / MEMORY_BOARD_SIZE;
        let block_start = block * MEMORY_BOARD_SIZE;
        let block_end = block_start + MEMORY_BOARD_SIZE - 1;
        ui.small(format!(
            "Block {block}: {block_start:04X}h–{block_end:04X}h — {}",
            if protected { "WRITE PROTECTED" } else { "writable" }
        ));
        if address == self.machine.cpu.pc {
            ui.strong("PC points here");
        }
        if address == self.machine.cpu.sp {
            ui.strong("SP points here");
        }

        ui.separator();
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
            if ui.small_button("Reload").clicked() {
                state.edit_value = current_byte;
                state.edit_input = format!("{current_byte:02X}");
                state.last_edit_message = None;
            }
        });
        ui.small(format!(
            "dec {}  |  {:08b}  |  ASCII {}",
            state.edit_value,
            state.edit_value,
            Self::ascii_description(state.edit_value)
        ));

        self.draw_bit_editor(ui, state);
        ui.separator();
        ui.checkbox(&mut state.respect_protection, "Respect write protection");
        let valid = Self::parse_memory_byte(&state.edit_input).is_some();
        let blocked = protected && state.respect_protection;
        if ui
            .add_enabled(valid && !blocked, egui::Button::new("Write byte to RAM"))
            .clicked()
        {
            let written = self.machine.bus.debugger_write_memory(
                address,
                state.edit_value,
                state.respect_protection,
            );
            state.last_edit_message = Some(if written {
                format!(
                    "Wrote {:02X}h to {:04X}h{}",
                    state.edit_value,
                    address,
                    if protected && !state.respect_protection {
                        " using debugger override"
                    } else {
                        ""
                    }
                )
            } else {
                "Write rejected by current memory configuration".into()
            });
        }
        if blocked {
            ui.small("Uncheck protection only when you deliberately want a debugger override.");
        } else if protected && !state.respect_protection {
            ui.small("Debugger override active: protection is being bypassed.");
        }
        if let Some(message) = &state.last_edit_message {
            ui.small(message);
        }
        if self.machine.running {
            ui.small("Machine is RUNNING; the CPU may overwrite this byte immediately.");
        }
    }

    fn draw_current_instruction_side(&self, ui: &mut egui::Ui) {
        let pc = self.machine.cpu.pc;
        let (instruction, bytes) = self.current_instruction();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("${pc:04X}")).monospace().strong());
            ui.label(egui::RichText::new(instruction).monospace().strong());
        });
        ui.small(egui::RichText::new(bytes).monospace().weak());
    }

    fn draw_memory_sidebar(&mut self, ui: &mut egui::Ui, state: &mut MemoryViewerUiState) {
        egui::ScrollArea::vertical()
            .id_salt("ram-inspector-sidebar-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.strong("CURRENT INSTRUCTION");
                self.draw_current_instruction_side(ui);
                ui.separator();

                egui::CollapsingHeader::new("Selected byte / editor")
                    .default_open(true)
                    .show(ui, |ui| self.draw_memory_editor(ui, state));
                ui.separator();

                egui::CollapsingHeader::new("1 KiB protection map")
                    .default_open(false)
                    .show(ui, |ui| self.draw_memory_block_map(ui, state));
                ui.separator();

                egui::CollapsingHeader::new("How to read this inspector")
                    .default_open(false)
                    .show(ui, |ui| self.draw_memory_help(ui));
            });
    }

    fn draw_memory_viewer_window(
        &mut self,
        ctx: &egui::Context,
        state: &mut MemoryViewerUiState,
    ) {
        egui::TopBottomPanel::top("memory-viewer-toolbar")
            .resizable(false)
            .show(ctx, |ui| self.draw_memory_toolbar(ui, state));

        egui::SidePanel::right("memory-viewer-sidebar")
            .resizable(true)
            .default_width(350.0)
            .width_range(290.0..=500.0)
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
                .with_title("RusTair — RAM Inspector / CPU Registers")
                .with_inner_size([1360.0, 780.0])
                .with_min_inner_size([1120.0, 600.0])
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

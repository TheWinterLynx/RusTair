use super::super::{egui, RusTairApp};
use crate::backend::{Intel8080State, InstructionTraceEntry};
use crate::config::SerialBoard;
use crate::debugger8080::{detect_simple_backward_loop, SimpleLoop};
use crate::decoder8080::{decode_8080, ControlFlow};
use crate::explain8080::explain_instruction;
use crate::trace8080::{CpuSnapshot8080, InstructionEffect8080};

const HISTORY_LIST_HEIGHT: f32 = 260.0;
const HISTORY_DETAIL_HEIGHT: f32 = 380.0;
const HISTORY_VISIBLE_ROWS: usize = 256;

#[derive(Clone)]
struct InstructionHistoryUiState {
    window_open: bool,
    capture: bool,
    follow_latest: bool,
    selected_sequence: Option<u64>,
    loop_inspector_open: bool,
    loop_snapshot: Option<SimpleLoop>,
    loop_iterations: u64,
    loop_last_sequence: u64,
    loop_trace_gap: bool,
    loop_exited: bool,
}

impl Default for InstructionHistoryUiState {
    fn default() -> Self {
        Self {
            window_open: false,
            capture: true,
            follow_latest: true,
            selected_sequence: None,
            loop_inspector_open: false,
            loop_snapshot: None,
            loop_iterations: 0,
            loop_last_sequence: 0,
            loop_trace_gap: false,
            loop_exited: false,
        }
    }
}

impl RusTairApp {
    fn instruction_history_state(ctx: &egui::Context) -> InstructionHistoryUiState {
        ctx.data(|data| {
            data.get_temp::<InstructionHistoryUiState>(egui::Id::new("rustair-instruction-history-state"))
                .unwrap_or_default()
        })
    }

    fn store_instruction_history_state(ctx: &egui::Context, state: InstructionHistoryUiState) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("rustair-instruction-history-state"), state);
        });
    }

    pub(in crate::app) fn open_instruction_history(&mut self, ctx: &egui::Context) {
        let mut state = Self::instruction_history_state(ctx);
        state.window_open = true;
        state.capture = true;
        state.follow_latest = true;
        self.machine.set_instruction_trace_enabled(true);
        Self::store_instruction_history_state(ctx, state);
    }

    fn cpu_state_from_trace(snapshot: CpuSnapshot8080) -> Intel8080State {
        Intel8080State {
            a: snapshot.a,
            b: snapshot.b,
            c: snapshot.c,
            d: snapshot.d,
            e: snapshot.e,
            h: snapshot.h,
            l: snapshot.l,
            flags: snapshot.flags,
            pc: snapshot.pc,
            sp: snapshot.sp,
            inte: snapshot.inte,
            halted: Some(snapshot.halted),
            total_t_states: None,
        }
    }

    fn register_deltas(entry: &InstructionTraceEntry) -> Vec<String> {
        let before = entry.before;
        let after = entry.after;
        let mut deltas = Vec::new();
        macro_rules! delta8 {
            ($name:literal, $field:ident) => {
                if before.$field != after.$field {
                    deltas.push(format!("{} {:02X} -> {:02X}", $name, before.$field, after.$field));
                }
            };
        }
        delta8!("A", a);
        delta8!("B", b);
        delta8!("C", c);
        delta8!("D", d);
        delta8!("E", e);
        delta8!("H", h);
        delta8!("L", l);
        delta8!("F", flags);
        if before.sp != after.sp {
            deltas.push(format!("SP {:04X} -> {:04X}", before.sp, after.sp));
        }
        if before.pc != after.pc {
            deltas.push(format!("PC {:04X} -> {:04X}", before.pc, after.pc));
        }
        if before.inte != after.inte {
            deltas.push(format!("INTE {} -> {}", u8::from(before.inte), u8::from(after.inte)));
        }
        if before.halted != after.halted {
            deltas.push(format!("HALT {} -> {}", u8::from(before.halted), u8::from(after.halted)));
        }
        deltas
    }

    fn observed_flow(entry: &InstructionTraceEntry) -> String {
        let decoded = decode_8080(entry.bytes[0], entry.bytes[1], entry.bytes[2]);
        let sequential = entry.address.wrapping_add(decoded.length as u16);
        match decoded.control_flow {
            ControlFlow::Jump { target, condition } => {
                let taken = entry.after.pc == target;
                match condition {
                    Some(condition) => format!(
                        "Observed branch: {} was {} -> {}",
                        condition.label(),
                        if taken { "TAKEN" } else { "NOT TAKEN" },
                        if taken { format!("PC=${target:04X}") } else { format!("PC=${sequential:04X}") }
                    ),
                    None => format!("Observed JMP -> PC=${:04X}", entry.after.pc),
                }
            }
            ControlFlow::Call { target, condition } => {
                let taken = entry.after.pc == target;
                match condition {
                    Some(condition) => format!(
                        "Observed call: {} was {} -> PC=${:04X}",
                        condition.label(),
                        if taken { "TAKEN" } else { "NOT TAKEN" },
                        entry.after.pc
                    ),
                    None => format!("Observed CALL -> PC=${:04X}, SP=${:04X}", entry.after.pc, entry.after.sp),
                }
            }
            ControlFlow::Return { condition } => match condition {
                Some(condition) => format!(
                    "Observed return: {} was {} -> PC=${:04X}",
                    condition.label(),
                    if entry.after.pc != sequential { "TAKEN" } else { "NOT TAKEN" },
                    entry.after.pc
                ),
                None => format!("Observed RET -> PC=${:04X}, SP=${:04X}", entry.after.pc, entry.after.sp),
            },
            ControlFlow::Restart { vector } => format!("Observed RST -> PC=${vector:04X}, SP=${:04X}", entry.after.sp),
            ControlFlow::IndirectJump => format!("Observed PCHL -> PC=${:04X}", entry.after.pc),
            ControlFlow::Halt => format!("Observed HLT -> HALT={}", u8::from(entry.after.halted)),
            ControlFlow::Linear => format!("Observed linear flow -> PC=${:04X}", entry.after.pc),
        }
    }

    fn io_port_context(&self, port: u8) -> Option<String> {
        if port == 0xff {
            return Some("Altair front-panel sense-switch input".into());
        }

        let board = self.config.machine.serial_board;
        match board {
            SerialBoard::Sio88 => {
                if port == board.status_port() {
                    Some("MITS 88-SIO status port".into())
                } else if port == board.data_port() {
                    Some("MITS 88-SIO data port".into())
                } else {
                    None
                }
            }
            SerialBoard::TwoSio88 => {
                if port == board.status_port() {
                    Some("MITS 88-2SIO Port 0 status/control".into())
                } else if port == board.data_port() {
                    Some("MITS 88-2SIO Port 0 data".into())
                } else if board.port1_status_port() == Some(port) {
                    Some("MITS 88-2SIO Port 1 status/control".into())
                } else if board.port1_data_port() == Some(port) {
                    Some("MITS 88-2SIO Port 1 data".into())
                } else {
                    None
                }
            }
        }
    }

    fn draw_effect(&self, ui: &mut egui::Ui, effect: InstructionEffect8080) {
        ui.horizontal_wrapped(|ui| {
            ui.monospace(effect.label());
            match effect {
                InstructionEffect8080::IoRead { port, .. }
                | InstructionEffect8080::IoWrite { port, .. } => {
                    if let Some(context) = self.io_port_context(port) {
                        ui.separator();
                        ui.small(context);
                    }
                }
                _ => {}
            }
        });
    }

    fn loop_for_entry(&mut self, entry: &InstructionTraceEntry) -> Option<SimpleLoop> {
        detect_simple_backward_loop(
            |address| self.machine.peek_memory(address),
            entry.after.pc,
            entry.after.flags,
        )
    }

    fn open_traced_loop_inspector(
        &mut self,
        state: &mut InstructionHistoryUiState,
        loop_info: SimpleLoop,
        latest_sequence: u64,
    ) {
        state.loop_snapshot = Some(loop_info);
        state.loop_inspector_open = true;
        state.loop_iterations = 0;
        state.loop_last_sequence = latest_sequence;
        state.loop_trace_gap = false;
        state.loop_exited = false;
        self.machine.set_instruction_trace_enabled(true);
    }

    fn update_loop_iteration_counter(
        state: &mut InstructionHistoryUiState,
        history: &[InstructionTraceEntry],
    ) {
        let Some(loop_info) = state.loop_snapshot.as_ref() else { return; };
        let Some(last) = history.last() else { return; };
        if last.sequence <= state.loop_last_sequence {
            return;
        }

        let mut new_entries = history
            .iter()
            .filter(|entry| entry.sequence > state.loop_last_sequence);
        if let Some(first) = new_entries.next() {
            if state.loop_last_sequence != 0
                && first.sequence > state.loop_last_sequence.saturating_add(1)
            {
                state.loop_trace_gap = true;
            }

            for entry in std::iter::once(first).chain(new_entries) {
                if entry.address == loop_info.back_edge {
                    if entry.after.pc == loop_info.start {
                        state.loop_iterations = state.loop_iterations.saturating_add(1);
                        state.loop_exited = false;
                    } else {
                        state.loop_exited = true;
                    }
                }
            }
        }
        state.loop_last_sequence = last.sequence;
    }

    fn draw_history_detail(
        &mut self,
        ui: &mut egui::Ui,
        entry: Option<&InstructionTraceEntry>,
        state: &mut InstructionHistoryUiState,
        latest_sequence: u64,
    ) {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), HISTORY_DETAIL_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let Some(entry) = entry else {
                    ui.label("No captured instruction selected yet.");
                    ui.small("Enable Capture and run or single-step the machine.");
                    return;
                };

                let decoded = decode_8080(entry.bytes[0], entry.bytes[1], entry.bytes[2]);
                let before_cpu = Self::cpu_state_from_trace(entry.before);
                let historical_m = entry.effects.iter().find_map(|effect| match effect {
                    InstructionEffect8080::MemoryRead { address, value }
                        if *address == entry.before.hl() => Some(*value),
                    _ => None,
                });
                let explanation = explain_instruction(&decoded, before_cpu, historical_m);
                let loop_candidate = self.loop_for_entry(entry);

                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!("#{:06}", entry.sequence));
                    ui.monospace(format!("${:04X}", entry.address));
                    ui.monospace(entry.bytes_text());
                    ui.monospace(decoded.text());
                    ui.separator();
                    ui.label(format!("{} T", entry.t_states));
                });
                ui.label(explanation.summary);
                ui.small(Self::observed_flow(entry));
                if let Some(loop_info) = loop_candidate {
                    if ui.button("Open independent Loop Inspector").clicked() {
                        self.open_traced_loop_inspector(state, loop_info, latest_sequence);
                    }
                }
                ui.separator();

                let deltas = Self::register_deltas(entry);
                ui.strong("State changes");
                if deltas.is_empty() {
                    ui.small("No visible CPU register/flag changes.");
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for delta in deltas {
                            ui.monospace(delta);
                            ui.separator();
                        }
                    });
                }

                ui.add_space(4.0);
                ui.strong("Memory / I/O effects");
                if entry.effects.is_empty() {
                    ui.small("No guest-visible data-memory, stack or I/O transfer for this instruction.");
                } else {
                    for effect in entry.effects.iter().copied() {
                        self.draw_effect(ui, effect);
                    }
                }

                ui.add_space(4.0);
                egui::Grid::new("instruction-history-before-after")
                    .num_columns(3)
                    .spacing([10.0, 3.0])
                    .show(ui, |ui| {
                        ui.strong(""); ui.strong("BEFORE"); ui.strong("AFTER"); ui.end_row();
                        ui.strong("AF"); ui.monospace(format!("{:04X}", entry.before.af())); ui.monospace(format!("{:04X}", entry.after.af())); ui.end_row();
                        ui.strong("BC"); ui.monospace(format!("{:04X}", entry.before.bc())); ui.monospace(format!("{:04X}", entry.after.bc())); ui.end_row();
                        ui.strong("DE"); ui.monospace(format!("{:04X}", entry.before.de())); ui.monospace(format!("{:04X}", entry.after.de())); ui.end_row();
                        ui.strong("HL"); ui.monospace(format!("{:04X}", entry.before.hl())); ui.monospace(format!("{:04X}", entry.after.hl())); ui.end_row();
                        ui.strong("SP"); ui.monospace(format!("{:04X}", entry.before.sp)); ui.monospace(format!("{:04X}", entry.after.sp)); ui.end_row();
                        ui.strong("PC"); ui.monospace(format!("{:04X}", entry.before.pc)); ui.monospace(format!("{:04X}", entry.after.pc)); ui.end_row();
                    });
            },
        );
    }

    fn draw_instruction_history_viewport_contents(
        &mut self,
        ctx: &egui::Context,
        state: &mut InstructionHistoryUiState,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let desired_backend_capture = state.capture || state.loop_inspector_open;
            let backend_capture = self.machine.instruction_trace_enabled();
            if backend_capture != desired_backend_capture {
                self.machine.set_instruction_trace_enabled(desired_backend_capture);
            }

            ui.horizontal_wrapped(|ui| {
                if ui.checkbox(&mut state.capture, "Capture").changed() {
                    self.machine
                        .set_instruction_trace_enabled(state.capture || state.loop_inspector_open);
                }
                ui.checkbox(&mut state.follow_latest, "Follow latest")
                    .on_hover_text("Following the newest entry is independent from Capture. Turn Follow off to inspect older entries while capture continues.");
                if ui.button("Clear").clicked() {
                    self.machine.clear_instruction_trace();
                    state.selected_sequence = None;
                }
                ui.separator();
                ui.small("Bounded history: last 4096 completed guest instructions.");
            });

            let history = self.machine.instruction_trace_snapshot();
            Self::update_loop_iteration_counter(state, &history);
            if state.follow_latest {
                state.selected_sequence = history.last().map(|entry| entry.sequence);
            }
            ui.small(format!("Captured: {} entries{}", history.len(), if state.capture { " | LIVE" } else { " | PAUSED" }));
            ui.separator();

            ui.strong("Completed instructions");
            egui::ScrollArea::vertical()
                .id_salt("instruction-history-list")
                .max_height(HISTORY_LIST_HEIGHT)
                .auto_shrink([false, false])
                .stick_to_bottom(state.follow_latest)
                .show(ui, |ui| {
                    let start = history.len().saturating_sub(HISTORY_VISIBLE_ROWS);
                    for entry in &history[start..] {
                        let decoded = decode_8080(entry.bytes[0], entry.bytes[1], entry.bytes[2]);
                        let selected = state.selected_sequence == Some(entry.sequence);
                        let effect_marker = if entry.effects.is_empty() { " " } else { "*" };
                        let text = format!(
                            "{} #{:06}  {:04X}  {:<8}  {:<18}  {:>2}T",
                            effect_marker,
                            entry.sequence,
                            entry.address,
                            entry.bytes_text(),
                            decoded.text(),
                            entry.t_states,
                        );
                        if ui.selectable_label(selected, egui::RichText::new(text).monospace()).clicked() {
                            state.selected_sequence = Some(entry.sequence);
                            state.follow_latest = false;
                        }
                    }
                });

            ui.separator();
            ui.strong("WHAT JUST HAPPENED?");
            let selected = state.selected_sequence
                .and_then(|sequence| history.iter().find(|entry| entry.sequence == sequence));
            let latest_sequence = history.last().map(|entry| entry.sequence).unwrap_or(0);
            self.draw_history_detail(ui, selected, state, latest_sequence);
        });
    }

    fn draw_loop_inspector_contents(
        &mut self,
        ctx: &egui::Context,
        state: &mut InstructionHistoryUiState,
    ) {
        let history = self.machine.instruction_trace_snapshot();
        Self::update_loop_iteration_counter(state, &history);
        let cpu = self.machine.intel8080_state();

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(loop_info) = state.loop_snapshot.as_ref() else {
                ui.label("No high-confidence loop snapshot is available.");
                return;
            };

            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("Loop {:04X}h -> {:04X}h", loop_info.start, loop_info.back_edge));
                ui.separator();
                ui.label(format!("{} instructions", loop_info.instructions.len()));
                ui.separator();
                ui.strong(format!("Iterations since opened: {}", state.loop_iterations));
            });
            if state.loop_trace_gap {
                ui.small("Trace buffer gap detected: iteration count is a lower bound because execution outran the retained 4096-instruction history.");
            } else {
                ui.small("Iteration count is exact for all trace entries observed since this inspector was opened.");
            }
            if state.loop_exited {
                ui.small("Last observed back-edge was NOT TAKEN: the loop exited.");
            }
            ui.separator();
            ui.small(format!("Entry: ${:04X} | back-edge: ${:04X} | branch target: ${:04X}", loop_info.start, loop_info.back_edge, loop_info.start));
            ui.small(loop_info.exit_description());
            if let Some(condition) = loop_info.condition {
                ui.small(format!(
                    "Current {}: {} ({})",
                    condition.label(),
                    if condition.evaluate(cpu.flags) { "TRUE -> TAKEN if back-edge executes now" } else { "FALSE -> EXIT if back-edge executes now" },
                    condition.description(),
                ));
            } else {
                ui.small("Back-edge is unconditional; structural loop has no conditional exit.");
            }
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("independent-loop-inspector-scroll")
                .show(ui, |ui| {
                    for instruction in &loop_info.instructions {
                        let is_pc = instruction.address == cpu.pc;
                        let is_back_edge = instruction.address == loop_info.back_edge;
                        let is_entry = instruction.address == loop_info.start;
                        let mut address_text = egui::RichText::new(format!("{:04X}", instruction.address)).monospace();
                        let mut instruction_text = egui::RichText::new(instruction.decoded.text()).monospace();
                        if is_pc {
                            address_text = address_text.strong().background_color(ui.visuals().widgets.active.bg_fill);
                            instruction_text = instruction_text.strong().background_color(ui.visuals().widgets.active.bg_fill);
                        }
                        ui.horizontal(|ui| {
                            ui.add_sized([56.0, 22.0], egui::Label::new(address_text));
                            ui.add_sized([96.0, 22.0], egui::Label::new(egui::RichText::new(instruction.decoded.bytes_text(instruction.bytes)).monospace().weak()));
                            ui.add_sized([230.0, 22.0], egui::Label::new(instruction_text));
                            if is_entry { ui.small("ENTRY"); }
                            if is_back_edge { ui.small("BACK-EDGE"); }
                            if is_pc { ui.strong("PC"); }
                        });
                    }
                });
        });
    }

    fn show_loop_inspector_viewport(
        &mut self,
        parent_ctx: &egui::Context,
        state: &mut InstructionHistoryUiState,
    ) {
        if !state.loop_inspector_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-8080-loop-inspector-viewport"),
            egui::ViewportBuilder::default()
                .with_title("RusTair - 8080 Loop Inspector")
                .with_inner_size([720.0, 520.0])
                .with_min_inner_size([560.0, 360.0])
                .with_resizable(true),
            |loop_ctx, _class| {
                self.draw_loop_inspector_contents(loop_ctx, state);
                if loop_ctx.input(|input| input.viewport().close_requested()) {
                    state.loop_inspector_open = false;
                    state.loop_snapshot = None;
                    if !state.window_open || !state.capture {
                        self.machine.set_instruction_trace_enabled(state.window_open && state.capture);
                    }
                }
            },
        );
    }

    pub(in crate::app) fn show_instruction_history_viewport(&mut self, parent_ctx: &egui::Context) {
        let mut state = Self::instruction_history_state(parent_ctx);

        if state.window_open {
            parent_ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("rustair-8080-execution-history-viewport"),
                egui::ViewportBuilder::default()
                    .with_title("RusTair - 8080 Execution History")
                    .with_inner_size([900.0, 820.0])
                    .with_min_inner_size([720.0, 600.0])
                    .with_resizable(true),
                |history_ctx, _class| {
                    self.draw_instruction_history_viewport_contents(history_ctx, &mut state);
                    if history_ctx.input(|input| input.viewport().close_requested()) {
                        state.window_open = false;
                        state.capture = false;
                        if !state.loop_inspector_open {
                            self.machine.set_instruction_trace_enabled(false);
                        }
                    }
                },
            );
        }

        self.show_loop_inspector_viewport(parent_ctx, &mut state);
        Self::store_instruction_history_state(parent_ctx, state);
    }
}

use super::super::{egui, RusTairApp};
use super::execution_position::current_instruction_address;
use crate::backend::{Intel8080State, InstructionTraceEntry};
use crate::config::SerialBoard;
use crate::debugger8080::detect_simple_backward_loop;
use crate::decoder8080::{decode_8080, ControlFlow};
use crate::explain8080::{explain_instruction, MemoryValue8080};
use crate::trace8080::{CpuSnapshot8080, InstructionEffect8080};

const HISTORY_LIST_HEIGHT: f32 = 260.0;
const HISTORY_DETAIL_HEIGHT: f32 = 380.0;
const HISTORY_DETAIL_SUMMARY_HEIGHT: f32 = 116.0;
const HISTORY_STATE_CHANGES_HEIGHT: f32 = 54.0;
const HISTORY_EFFECTS_HEIGHT: f32 = 88.0;
const HISTORY_REGISTERS_HEIGHT: f32 = 166.0;
const HISTORY_EFFECT_ROW_HEIGHT: f32 = 22.0;
const HISTORY_VISIBLE_ROWS: usize = 256;

#[derive(Clone)]
struct InstructionHistoryUiState {
    window_open: bool,
    capture: bool,
    follow_latest: bool,
    selected_sequence: Option<u64>,
}

impl Default for InstructionHistoryUiState {
    fn default() -> Self {
        Self {
            window_open: false,
            capture: true,
            follow_latest: true,
            selected_sequence: None,
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
        Self::store_instruction_history_state(ctx, state);
    }

    fn fixed_detail_body(
        ui: &mut egui::Ui,
        id: &'static str,
        height: f32,
        add_contents: impl FnOnce(&mut egui::Ui),
    ) {
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(id)
                    .auto_shrink([false, false])
                    .show(ui, add_contents);
            },
        );
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
        let sequential = entry.address.wrapping_add(u16::from(decoded.length));
        match decoded.control_flow {
            ControlFlow::Jump { condition, .. } => {
                let taken = condition
                    .map(|condition| condition.evaluate(entry.before.flags))
                    .unwrap_or(true);
                match condition {
                    Some(condition) => format!(
                        "Observed branch: {} was {} -> PC=${:04X}",
                        condition.label(),
                        if taken { "TAKEN" } else { "NOT TAKEN" },
                        entry.after.pc,
                    ),
                    None => format!("Observed JMP -> PC=${:04X}", entry.after.pc),
                }
            }
            ControlFlow::Call { condition, .. } => {
                let taken = condition
                    .map(|condition| condition.evaluate(entry.before.flags))
                    .unwrap_or(true);
                match condition {
                    Some(condition) => format!(
                        "Observed call: {} was {} -> PC=${:04X}",
                        condition.label(),
                        if taken { "TAKEN" } else { "NOT TAKEN" },
                        entry.after.pc,
                    ),
                    None => format!("Observed CALL -> PC=${:04X}, SP=${:04X}", entry.after.pc, entry.after.sp),
                }
            }
            ControlFlow::Return { condition } => {
                let taken = condition
                    .map(|condition| condition.evaluate(entry.before.flags))
                    .unwrap_or(true);
                match condition {
                    Some(condition) => format!(
                        "Observed return: {} was {} -> PC=${:04X}",
                        condition.label(),
                        if taken { "TAKEN" } else { "NOT TAKEN" },
                        entry.after.pc,
                    ),
                    None => format!("Observed RET -> PC=${:04X}, SP=${:04X}", entry.after.pc, entry.after.sp),
                }
            }
            ControlFlow::Restart { vector } => format!("Observed RST -> PC=${vector:04X}, SP=${:04X}", entry.after.sp),
            ControlFlow::IndirectJump => format!("Observed PCHL -> PC=${:04X}", entry.after.pc),
            ControlFlow::Halt => format!("Observed HLT -> HALT={}", u8::from(entry.after.halted)),
            ControlFlow::Linear => format!("Observed linear flow -> PC=${:04X} (sequential ${sequential:04X})", entry.after.pc),
        }
    }

    fn io_port_context(&self, port: u8) -> Option<String> {
        if port == 0xff {
            return Some("Altair front-panel sense-switch input".into());
        }

        let board = self.config.machine.serial_board;
        let mapped = match board {
            SerialBoard::Sio88 => {
                if port == board.status_port() {
                    Some("MITS 88-SIO status port")
                } else if port == board.data_port() {
                    Some("MITS 88-SIO data port")
                } else {
                    None
                }
            }
            SerialBoard::TwoSio88 => {
                if port == board.status_port() {
                    Some("MITS 88-2SIO Port 0 status/control")
                } else if port == board.data_port() {
                    Some("MITS 88-2SIO Port 0 data")
                } else if board.port1_status_port() == Some(port) {
                    Some("MITS 88-2SIO Port 1 status/control")
                } else if board.port1_data_port() == Some(port) {
                    Some("MITS 88-2SIO Port 1 data")
                } else {
                    None
                }
            }
        };
        mapped.map(|label| format!("Current board mapping: {label}"))
    }

    fn effect_context(&self, effect: InstructionEffect8080) -> String {
        match effect {
            InstructionEffect8080::IoRead { port, .. }
            | InstructionEffect8080::IoWrite { port, .. } => self.io_port_context(port).unwrap_or_default(),
            InstructionEffect8080::MemoryWrite { .. }
            | InstructionEffect8080::StackWrite { .. } => "guest write transfer/attempt".into(),
            _ => String::new(),
        }
    }

    fn draw_effect(&self, ui: &mut egui::Ui, effect: InstructionEffect8080) {
        let context = self.effect_context(effect);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), HISTORY_EFFECT_ROW_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add_sized(
                    [178.0, HISTORY_EFFECT_ROW_HEIGHT],
                    egui::Label::new(egui::RichText::new(effect.label()).monospace()),
                );
                ui.separator();
                let response = ui.add_sized(
                    [ui.available_width(), HISTORY_EFFECT_ROW_HEIGHT],
                    egui::Label::new(egui::RichText::new(&context).small()),
                );
                if !context.is_empty() {
                    response.on_hover_text(context);
                }
            },
        );
    }

    fn draw_history_detail(
        &mut self,
        ui: &mut egui::Ui,
        entry: Option<&InstructionTraceEntry>,
        latest_sequence: u64,
    ) {
        let Some(entry) = entry else {
            ui.label("No captured instruction selected yet.");
            ui.small("Enable Capture and run or single-step the machine.");
            return;
        };

        let decoded = decode_8080(entry.bytes[0], entry.bytes[1], entry.bytes[2]);
        let before_cpu = Self::cpu_state_from_trace(entry.before);
        let memory_context = entry
            .effects
            .iter()
            .find_map(|effect| match effect {
                InstructionEffect8080::MemoryRead { address, value }
                    if *address == entry.before.hl() => Some(*value),
                _ => None,
            })
            .map(MemoryValue8080::Known)
            .unwrap_or(MemoryValue8080::Unknown);
        let explanation = explain_instruction(&decoded, before_cpu, memory_context);

        Self::fixed_detail_body(
            ui,
            "instruction-history-summary-body",
            HISTORY_DETAIL_SUMMARY_HEIGHT,
            |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized([82.0, 20.0], egui::Label::new(egui::RichText::new(format!("#{:06}", entry.sequence)).strong()));
                    ui.add_sized([64.0, 20.0], egui::Label::new(egui::RichText::new(format!("${:04X}", entry.address)).monospace()));
                    ui.add_sized([92.0, 20.0], egui::Label::new(egui::RichText::new(entry.bytes_text()).monospace()));
                    ui.add_sized([180.0, 20.0], egui::Label::new(egui::RichText::new(decoded.text()).monospace()));
                    ui.separator();
                    ui.add_sized([56.0, 20.0], egui::Label::new(format!("{} T", entry.t_states)));
                });
                ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::Label::new(&explanation.summary),
                );
                ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::Label::new(egui::RichText::new(Self::observed_flow(entry)).small()),
                );

                ui.horizontal(|ui| {
                    if entry.sequence == latest_sequence {
                        let live_cpu = self.machine.intel8080_state();
                        let execution_address = current_instruction_address(self);
                        let loop_info = if execution_address == entry.after.pc {
                            detect_simple_backward_loop(
                                |address| self.machine.peek_memory(address),
                                execution_address,
                                live_cpu.flags,
                            )
                        } else {
                            None
                        };
                        if let Some(loop_info) = loop_info {
                            if ui.add_sized([238.0, 22.0], egui::Button::new("Open independent Loop Inspector")).clicked() {
                                self.open_loop_inspector(ui.ctx(), loop_info);
                            }
                            ui.add_sized([ui.available_width(), 22.0], egui::Label::new(""));
                        } else {
                            ui.add_enabled_ui(false, |ui| {
                                ui.add_sized([238.0, 22.0], egui::Button::new("Open independent Loop Inspector"));
                            });
                            ui.add_sized(
                                [ui.available_width(), 22.0],
                                egui::Label::new(egui::RichText::new("No simple live loop is demonstrated at this boundary.").small()),
                            );
                        }
                    } else {
                        ui.add_sized(
                            [ui.available_width(), 22.0],
                            egui::Label::new(egui::RichText::new("Historical loop reconstruction is not attempted because RAM snapshots are not retained per instruction.").small()),
                        );
                    }
                });
            },
        );

        ui.separator();
        let deltas = Self::register_deltas(entry);
        super::collapsible_section(ui, "State changes", true, |ui| {
            Self::fixed_detail_body(
                ui,
                "instruction-history-state-changes-body",
                HISTORY_STATE_CHANGES_HEIGHT,
                |ui| {
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
                },
            );
        });

        ui.separator();
        super::collapsible_section(ui, "Memory / I/O effects", true, |ui| {
            Self::fixed_detail_body(
                ui,
                "instruction-history-effects-body",
                HISTORY_EFFECTS_HEIGHT,
                |ui| {
                    if entry.effects.is_empty() {
                        ui.add_sized(
                            [ui.available_width(), HISTORY_EFFECT_ROW_HEIGHT],
                            egui::Label::new(egui::RichText::new("No guest-visible data-memory, stack or I/O transfer for this instruction.").small()),
                        );
                    } else {
                        for effect in entry.effects.iter().copied() {
                            self.draw_effect(ui, effect);
                        }
                    }
                },
            );
        });

        ui.separator();
        super::collapsible_section(ui, "Before / after registers", true, |ui| {
            Self::fixed_detail_body(
                ui,
                "instruction-history-before-after-body",
                HISTORY_REGISTERS_HEIGHT,
                |ui| {
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
        });
    }

    fn draw_instruction_history_viewport_contents(
        &mut self,
        ctx: &egui::Context,
        state: &mut InstructionHistoryUiState,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let history = self.machine.instruction_trace_snapshot();
            let metadata = self.machine.instruction_trace_metadata();
            let backend_capture_active = self.machine.instruction_trace_enabled();
            if state.follow_latest {
                state.selected_sequence = history.last().map(|entry| entry.sequence);
            }

            super::collapsible_section(ui, "Capture controls", true, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut state.capture, "Capture")
                        .on_hover_text("Request instruction capture for this window. The shared trace can remain active if Call Stack, Memory Activity or Loop Inspector still needs it.");
                    ui.checkbox(&mut state.follow_latest, "Follow latest")
                        .on_hover_text("Following the newest entry is independent from Capture. Turn Follow off to inspect older entries while capture continues.");
                    if ui.button("Clear shared history")
                        .on_hover_text("Clears the shared instruction ring and starts a new trace generation for Execution History, Call Stack, Memory Activity and Loop Inspector.")
                        .clicked()
                    {
                        self.machine.clear_instruction_trace();
                        state.selected_sequence = None;
                    }
                    ui.separator();
                    ui.small("Bounded history: last 4096 completed guest instructions.");
                });

                let capture_status = if state.capture {
                    "LIVE · requested by Execution History"
                } else if backend_capture_active {
                    "LIVE · required by another debugger view"
                } else {
                    "PAUSED"
                };
                ui.small(format!(
                    "Captured: {} entries | {} | dropped this generation: {}",
                    history.len(),
                    capture_status,
                    metadata.dropped_entries,
                ));
            });

            ui.separator();
            super::collapsible_section(ui, "Completed instructions", true, |ui| {
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
            });

            ui.separator();
            super::collapsible_section(ui, "WHAT JUST HAPPENED?", true, |ui| {
                let selected = state.selected_sequence
                    .and_then(|sequence| history.iter().find(|entry| entry.sequence == sequence));
                let latest_sequence = history.last().map(|entry| entry.sequence).unwrap_or(0);
                egui::ScrollArea::vertical()
                    .id_salt("instruction-history-detail")
                    .max_height(HISTORY_DETAIL_HEIGHT)
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.draw_history_detail(ui, selected, latest_sequence));
            });
        });
    }

    pub(in crate::app) fn show_instruction_history_viewport(&mut self, parent_ctx: &egui::Context) {
        let mut state = Self::instruction_history_state(parent_ctx);
        if !state.window_open {
            return;
        }

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
                }
            },
        );

        Self::store_instruction_history_state(parent_ctx, state);
    }
}

pub(super) fn trace_requested(ctx: &egui::Context) -> bool {
    let state = RusTairApp::instruction_history_state(ctx);
    state.window_open && state.capture
}

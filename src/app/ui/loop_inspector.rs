use super::super::{egui, RusTairApp};
use super::execution_position::current_instruction_address;
use crate::debugger8080::SimpleLoop;
use crate::trace8080::{InstructionTraceEntry, InstructionTraceMetadata};

const LOOP_STATUS_LINE_HEIGHT: f32 = 20.0;

#[derive(Clone, Default)]
struct LoopInspectorUiState {
    window_open: bool,
    snapshot: Option<SimpleLoop>,
    baseline_generation: u64,
    last_sequence: u64,
    iterations: u64,
    trace_gap: bool,
    trace_reset: bool,
    exited: bool,
}

impl RusTairApp {
    fn loop_inspector_state(ctx: &egui::Context) -> LoopInspectorUiState {
        ctx.data(|data| {
            data.get_temp::<LoopInspectorUiState>(egui::Id::new("rustair-shared-loop-inspector-state"))
                .unwrap_or_default()
        })
    }

    fn store_loop_inspector_state(ctx: &egui::Context, state: LoopInspectorUiState) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("rustair-shared-loop-inspector-state"), state);
        });
    }

    pub(in crate::app) fn open_loop_inspector(
        &mut self,
        ctx: &egui::Context,
        loop_info: SimpleLoop,
    ) {
        let metadata = self.machine.instruction_trace_metadata();
        let latest_sequence = self
            .machine
            .instruction_trace_snapshot()
            .last()
            .map(|entry| entry.sequence)
            .unwrap_or(0);
        let state = LoopInspectorUiState {
            window_open: true,
            snapshot: Some(loop_info),
            baseline_generation: metadata.generation,
            last_sequence: latest_sequence,
            iterations: 0,
            trace_gap: false,
            trace_reset: false,
            exited: false,
        };
        Self::store_loop_inspector_state(ctx, state);
    }

    fn update_shared_loop_counter(
        state: &mut LoopInspectorUiState,
        history: &[InstructionTraceEntry],
        metadata: InstructionTraceMetadata,
    ) {
        let Some(loop_info) = state.snapshot.as_ref() else { return; };

        if metadata.generation != state.baseline_generation {
            state.baseline_generation = metadata.generation;
            state.last_sequence = history.last().map(|entry| entry.sequence).unwrap_or(0);
            state.iterations = 0;
            state.trace_reset = true;
            state.trace_gap = false;
            state.exited = false;
            return;
        }

        let Some(last) = history.last() else { return; };
        if last.sequence <= state.last_sequence {
            return;
        }

        let mut new_entries = history.iter().filter(|entry| entry.sequence > state.last_sequence);
        if let Some(first) = new_entries.next() {
            if state.last_sequence != 0
                && first.sequence > state.last_sequence.saturating_add(1)
            {
                state.trace_gap = true;
            }
            for entry in std::iter::once(first).chain(new_entries) {
                if entry.address == loop_info.back_edge {
                    if entry.after.pc == loop_info.start {
                        state.iterations = state.iterations.saturating_add(1);
                        state.exited = false;
                    } else {
                        state.exited = true;
                    }
                }
            }
        }
        state.last_sequence = last.sequence;
    }

    fn draw_shared_loop_inspector_contents(
        &mut self,
        ctx: &egui::Context,
        state: &mut LoopInspectorUiState,
    ) {
        let history = self.machine.instruction_trace_snapshot();
        let metadata = self.machine.instruction_trace_metadata();
        Self::update_shared_loop_counter(state, &history, metadata);
        let cpu = self.machine.intel8080_state();
        let execution_address = current_instruction_address(self);

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(loop_info) = state.snapshot.as_ref() else {
                ui.label("No high-confidence loop snapshot is available.");
                return;
            };

            ui.horizontal(|ui| {
                ui.strong(format!("Loop {:04X}h -> {:04X}h", loop_info.start, loop_info.back_edge));
                ui.separator();
                ui.label(format!("{} instructions", loop_info.instructions.len()));
                ui.separator();
                ui.strong(format!(
                    "Iterations since opened: {}{}",
                    if state.trace_gap { ">=" } else { "" },
                    state.iterations
                ));
                ui.separator();
                ui.monospace(format!("PC=${:04X} EXEC=${execution_address:04X}", cpu.pc));
            });

            let capture_status = if state.trace_reset {
                "Instruction history was cleared/reset while this inspector was open; the iteration counter restarted from zero."
            } else if state.trace_gap {
                "Execution outran the retained trace between two observations; the iteration count is therefore a lower bound."
            } else {
                "Iteration count is exact for all trace sequences observed since this inspector was opened."
            };
            ui.add_sized(
                [ui.available_width(), LOOP_STATUS_LINE_HEIGHT],
                egui::Label::new(egui::RichText::new(capture_status).small()),
            );
            ui.add_sized(
                [ui.available_width(), LOOP_STATUS_LINE_HEIGHT],
                egui::Label::new(egui::RichText::new(if state.exited {
                    "Last observed execution of the back-edge did not return to the loop entry: the loop exited."
                } else {
                    ""
                }).small()),
            );

            ui.separator();
            ui.small(format!(
                "Entry: ${:04X} | back-edge: ${:04X}",
                loop_info.start, loop_info.back_edge
            ));
            ui.small(loop_info.exit_description());
            let condition_text = if let Some(condition) = loop_info.condition {
                let flag_value = condition.evaluate(cpu.flags);
                if execution_address == loop_info.back_edge {
                    format!(
                        "At the back-edge now: {} is {} -> branch {}.",
                        condition.label(),
                        if flag_value { "TRUE" } else { "FALSE" },
                        if flag_value { "TAKEN" } else { "NOT TAKEN / EXIT" },
                    )
                } else {
                    format!(
                        "Current flags make {} {}; instructions before the back-edge may still change those flags.",
                        condition.label(),
                        if flag_value { "TRUE" } else { "FALSE" },
                    )
                }
            } else {
                "Back-edge is unconditional; the structural loop has no conditional exit.".into()
            };
            ui.add_sized(
                [ui.available_width(), LOOP_STATUS_LINE_HEIGHT],
                egui::Label::new(egui::RichText::new(condition_text).small()),
            );
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("shared-8080-loop-inspector-scroll")
                .show(ui, |ui| {
                    for instruction in &loop_info.instructions {
                        let is_exec = instruction.address == execution_address;
                        let is_back_edge = instruction.address == loop_info.back_edge;
                        let is_entry = instruction.address == loop_info.start;
                        let mut address_text = egui::RichText::new(format!("{:04X}", instruction.address)).monospace();
                        let mut instruction_text = egui::RichText::new(instruction.decoded.text()).monospace();
                        if is_exec {
                            address_text = address_text.strong().background_color(ui.visuals().widgets.active.bg_fill);
                            instruction_text = instruction_text.strong().background_color(ui.visuals().widgets.active.bg_fill);
                        }
                        let markers = format!(
                            "{} {} {}",
                            if is_entry { "ENTRY" } else { "     " },
                            if is_back_edge { "BACK-EDGE" } else { "         " },
                            if is_exec { "EXEC" } else { "    " },
                        );
                        ui.horizontal(|ui| {
                            ui.add_sized([56.0, 22.0], egui::Label::new(address_text));
                            ui.add_sized([96.0, 22.0], egui::Label::new(
                                egui::RichText::new(instruction.decoded.bytes_text(instruction.bytes)).monospace().weak(),
                            ));
                            ui.add_sized([230.0, 22.0], egui::Label::new(instruction_text));
                            ui.add_sized([210.0, 22.0], egui::Label::new(egui::RichText::new(markers).small()));
                        });
                    }
                });
        });
    }

    pub(in crate::app) fn show_loop_inspector_viewport(&mut self, parent_ctx: &egui::Context) {
        let mut state = Self::loop_inspector_state(parent_ctx);
        if !state.window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-shared-8080-loop-inspector-viewport"),
            egui::ViewportBuilder::default()
                .with_title("RusTair - 8080 Loop Inspector")
                .with_inner_size([760.0, 520.0])
                .with_min_inner_size([620.0, 360.0])
                .with_resizable(true),
            |loop_ctx, _class| {
                self.draw_shared_loop_inspector_contents(loop_ctx, &mut state);
                if loop_ctx.input(|input| input.viewport().close_requested()) {
                    state.window_open = false;
                    state.snapshot = None;
                }
            },
        );

        Self::store_loop_inspector_state(parent_ctx, state);
    }
}

pub(super) fn trace_requested(ctx: &egui::Context) -> bool {
    RusTairApp::loop_inspector_state(ctx).window_open
}

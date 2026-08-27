use super::super::{egui, RusTairApp};
use crate::debugger8080::SimpleLoop;
use crate::trace8080::{InstructionTraceEntry, InstructionTraceMetadata};

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
            // Ring eviction by itself does not imply that this inspector lost
            // anything: when the buffer is full, every new instruction evicts
            // one old entry. We only lose countable execution when the oldest
            // newly retained sequence has jumped past the sequence we last
            // processed.
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

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(loop_info) = state.snapshot.as_ref() else {
                ui.label("No high-confidence loop snapshot is available.");
                return;
            };

            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("Loop {:04X}h -> {:04X}h", loop_info.start, loop_info.back_edge));
                ui.separator();
                ui.label(format!("{} instructions", loop_info.instructions.len()));
                ui.separator();
                ui.strong(format!(
                    "Iterations since opened: {}{}",
                    if state.trace_gap { ">=" } else { "" },
                    state.iterations
                ));
            });
            if state.trace_reset {
                ui.small("Instruction history was cleared/reset while this inspector was open; the iteration counter restarted from zero.");
            } else if state.trace_gap {
                ui.small("Execution outran the retained trace between two observations; the iteration count is therefore a lower bound.");
            } else {
                ui.small("Iteration count is exact for all trace sequences observed since this inspector was opened.");
            }
            if state.exited {
                ui.small("Last observed execution of the back-edge did not return to the loop entry: the loop exited.");
            }

            ui.separator();
            ui.small(format!(
                "Entry: ${:04X} | back-edge: ${:04X}",
                loop_info.start, loop_info.back_edge
            ));
            ui.small(loop_info.exit_description());
            if let Some(condition) = loop_info.condition {
                let flag_value = condition.evaluate(cpu.flags);
                if cpu.pc == loop_info.back_edge {
                    ui.small(format!(
                        "At the back-edge now: {} is {} -> branch {}.",
                        condition.label(),
                        if flag_value { "TRUE" } else { "FALSE" },
                        if flag_value { "TAKEN" } else { "NOT TAKEN / EXIT" },
                    ));
                } else {
                    ui.small(format!(
                        "Current flags make {} {}; instructions before the back-edge may still change those flags.",
                        condition.label(),
                        if flag_value { "TRUE" } else { "FALSE" },
                    ));
                }
            } else {
                ui.small("Back-edge is unconditional; the structural loop has no conditional exit.");
            }
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("shared-8080-loop-inspector-scroll")
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
                            ui.add_sized([96.0, 22.0], egui::Label::new(
                                egui::RichText::new(instruction.decoded.bytes_text(instruction.bytes)).monospace().weak(),
                            ));
                            ui.add_sized([230.0, 22.0], egui::Label::new(instruction_text));
                            if is_entry { ui.small("ENTRY"); }
                            if is_back_edge { ui.small("BACK-EDGE"); }
                            if is_pc { ui.strong("PC"); }
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
                .with_inner_size([720.0, 520.0])
                .with_min_inner_size([560.0, 360.0])
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

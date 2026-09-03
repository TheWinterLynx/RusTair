use super::super::{egui, RusTairApp};
use super::execution_position::current_instruction_address;
use super::s100_memory_inspection::{mapping_detail, mapping_summary};
use crate::backend::MemoryWatchAccess;
use crate::memory_activity8080::summarize_memory_activity_8080;

const ACTIVITY_STATUS_LINE_HEIGHT: f32 = 20.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivitySort {
    Recent,
    Total,
    Execute,
    Read,
    Write,
    Address,
}

#[derive(Clone)]
struct MemoryActivityUiState {
    window_open: bool,
    sort: ActivitySort,
    descending: bool,
    message: Option<String>,
}

impl Default for MemoryActivityUiState {
    fn default() -> Self {
        Self {
            window_open: false,
            sort: ActivitySort::Address,
            descending: false,
            message: None,
        }
    }
}

impl RusTairApp {
    fn memory_activity_state(ctx: &egui::Context) -> MemoryActivityUiState {
        ctx.data(|data| {
            data.get_temp::<MemoryActivityUiState>(egui::Id::new("rustair-memory-activity-state"))
                .unwrap_or_default()
        })
    }

    fn store_memory_activity_state(ctx: &egui::Context, state: MemoryActivityUiState) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("rustair-memory-activity-state"), state);
        });
    }

    pub(in crate::app) fn open_memory_activity(&mut self, ctx: &egui::Context) {
        let mut state = Self::memory_activity_state(ctx);
        state.window_open = true;
        Self::store_memory_activity_state(ctx, state);
    }

    fn draw_memory_activity_contents(
        &mut self,
        ctx: &egui::Context,
        state: &mut MemoryActivityUiState,
    ) {
        let history = self.machine.instruction_trace_snapshot();
        let metadata = self.machine.instruction_trace_metadata();
        let activity = summarize_memory_activity_8080(&history, metadata);
        let cpu = self.machine.intel8080_state();
        let execution_address = current_instruction_address(self);

        let mut rows: Vec<_> = activity.iter().collect();
        rows.sort_by(|(address_a, a), (address_b, b)| {
            let ordering = match state.sort {
                ActivitySort::Recent => a.last_sequence().cmp(&b.last_sequence()),
                ActivitySort::Total => a.total().cmp(&b.total()),
                ActivitySort::Execute => a.execute_count.cmp(&b.execute_count),
                ActivitySort::Read => a.read_count.cmp(&b.read_count),
                ActivitySort::Write => a.write_count.cmp(&b.write_count),
                ActivitySort::Address => address_a.cmp(address_b),
            };
            if state.descending { ordering.reverse() } else { ordering }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("8080 MEMORY ACTIVITY");
                ui.separator();
                ui.label(format!("{} active address(es)", activity.active_addresses()));
                ui.separator();
                ui.monospace(format!(
                    "PC=${:04X} EXEC=${execution_address:04X} HL=${:04X} SP=${:04X}",
                    cpu.pc,
                    cpu.hl(),
                    cpu.sp
                ));
            });
            ui.separator();

            super::collapsible_section(ui, "Activity meaning / capture status", true, |ui| {
                ui.small("EXECUTE counts instruction starts; READ/WRITE count guest data-memory and stack bus transfers. Opcode/operand fetches are intentionally excluded.");
                ui.small("WRITE means the 8080 attempted the bus transfer; an unmapped address, physical protection, or overlapping cards can prevent one unique RAM cell from changing.");
                ui.small("S-100 NOW is the current physical S-100 mapping when this window is drawn. It is not a reconstructed historical mapping for the retained transfer; the activity counters are historical, the mapping column is live instrumentation.");
                ui.add_sized(
                    [ui.available_width(), ACTIVITY_STATUS_LINE_HEIGHT],
                    egui::Label::new(egui::RichText::new(if activity.dropped_entries != 0 {
                        format!(
                            "{} older trace entr{} were evicted; displayed activity counts are lower bounds for the current capture generation.",
                            activity.dropped_entries,
                            if activity.dropped_entries == 1 { "y" } else { "ies" },
                        )
                    } else {
                        String::new()
                    }).small()),
                );
                ui.add_sized(
                    [ui.available_width(), ACTIVITY_STATUS_LINE_HEIGHT],
                    egui::Label::new(egui::RichText::new(if activity.sequence_gap {
                        "A sequence gap exists inside the retained trace; displayed activity counts are incomplete."
                    } else {
                        ""
                    }).small()),
                );
            });

            ui.separator();
            super::collapsible_section(ui, "Sort / controls", true, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Sort:");
                    for (value, label) in [
                        (ActivitySort::Recent, "Recent"),
                        (ActivitySort::Total, "Total"),
                        (ActivitySort::Execute, "Exec"),
                        (ActivitySort::Read, "Read"),
                        (ActivitySort::Write, "Write"),
                        (ActivitySort::Address, "Address"),
                    ] {
                        ui.selectable_value(&mut state.sort, value, label);
                    }
                    ui.checkbox(&mut state.descending, "Descending");
                    if ui.button("Clear trace").clicked() {
                        self.machine.clear_instruction_trace();
                        state.message = Some("Instruction trace/activity counters cleared; this starts a new capture generation.".into());
                    }
                });
            });

            ui.separator();
            super::collapsible_section(ui, "Activity table", true, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized([64.0, 20.0], egui::Label::new(egui::RichText::new("ADDR").monospace().strong()));
                    ui.add_sized([70.0, 20.0], egui::Label::new(egui::RichText::new("EXEC").monospace().strong()));
                    ui.add_sized([70.0, 20.0], egui::Label::new(egui::RichText::new("READ").monospace().strong()));
                    ui.add_sized([70.0, 20.0], egui::Label::new(egui::RichText::new("WRITE").monospace().strong()));
                    ui.add_sized([88.0, 20.0], egui::Label::new(egui::RichText::new("LAST #").monospace().strong()));
                    ui.add_sized([128.0, 20.0], egui::Label::new(egui::RichText::new("MARKERS").strong()));
                    ui.add_sized([260.0, 20.0], egui::Label::new(egui::RichText::new("S-100 NOW").strong()));
                    ui.add_sized([ui.available_width(), 20.0], egui::Label::new(egui::RichText::new("ACTIONS").strong()));
                });

                egui::ScrollArea::vertical()
                    .id_salt("memory-activity-list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (address, counters) in rows {
                            let markers = format!(
                                "{} {} {} {}",
                                if address == execution_address { "EXEC" } else { "    " },
                                if address == cpu.pc { "PC" } else { "  " },
                                if address == cpu.hl() { "HL/M" } else { "    " },
                                if address == cpu.sp { "SP" } else { "  " },
                            );
                            let inspection = self.machine.inspect_memory_mapping(address);
                            let mapping = mapping_summary(&inspection);
                            ui.horizontal(|ui| {
                                ui.add_sized([64.0, 20.0], egui::Label::new(egui::RichText::new(format!("${address:04X}")).monospace()));
                                ui.add_sized([70.0, 20.0], egui::Label::new(egui::RichText::new(format!("{}", counters.execute_count)).monospace()));
                                ui.add_sized([70.0, 20.0], egui::Label::new(egui::RichText::new(format!("{}", counters.read_count)).monospace()));
                                ui.add_sized([70.0, 20.0], egui::Label::new(egui::RichText::new(format!("{}", counters.write_count)).monospace()));
                                ui.add_sized([88.0, 20.0], egui::Label::new(egui::RichText::new(
                                    counters.last_sequence().map(|value| value.to_string()).unwrap_or_else(|| "-".into())
                                ).monospace()));
                                ui.add_sized([128.0, 20.0], egui::Label::new(egui::RichText::new(markers).small()));
                                ui.add_sized(
                                    [260.0, 20.0],
                                    egui::Label::new(egui::RichText::new(&mapping).small()),
                                )
                                .on_hover_text(mapping_detail(address, &inspection));
                                if ui.add_sized([82.0, 20.0], egui::Button::new("R/W watch")).clicked() {
                                    self.machine.debugger_set_watchpoint(address, Some(MemoryWatchAccess::ReadWrite));
                                    state.message = Some(format!("READ/WRITE watchpoint armed at ${address:04X} · {mapping}."));
                                }
                                if ui.add_sized([62.0, 20.0], egui::Button::new("Run to")).clicked() {
                                    self.machine.debugger_run_to(address);
                                    state.message = Some(format!("Running to ${address:04X}."));
                                }
                            });
                        }
                    });
            });

            ui.separator();
            ui.add_sized(
                [ui.available_width(), ACTIVITY_STATUS_LINE_HEIGHT],
                egui::Label::new(egui::RichText::new(state.message.as_deref().unwrap_or("")).small()),
            );
        });
    }

    pub(in crate::app) fn show_memory_activity_viewport(&mut self, parent_ctx: &egui::Context) {
        let mut state = Self::memory_activity_state(parent_ctx);
        if !state.window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-8080-memory-activity-viewport"),
            egui::ViewportBuilder::default()
                .with_title("RusTair - 8080 Memory Activity")
                .with_inner_size([1260.0, 660.0])
                .with_min_inner_size([1080.0, 480.0])
                .with_resizable(true),
            |activity_ctx, _class| {
                self.draw_memory_activity_contents(activity_ctx, &mut state);
                if activity_ctx.input(|input| input.viewport().close_requested()) {
                    state.window_open = false;
                }
            },
        );

        Self::store_memory_activity_state(parent_ctx, state);
    }
}

pub(super) fn trace_requested(ctx: &egui::Context) -> bool {
    RusTairApp::memory_activity_state(ctx).window_open
}

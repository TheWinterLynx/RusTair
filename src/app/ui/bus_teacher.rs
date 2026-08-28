use super::super::{egui, RusTairApp};
use crate::backend::{BusMachineCycle, BusTeachingSnapshot, BusTState};
use crate::decoder8080::decode_8080;

const SIGNAL_ROW_HEIGHT: f32 = 20.0;
const TIMING_ROW_HEIGHT: f32 = 22.0;
const TIMING_LEFT_LABEL_WIDTH: f32 = 105.0;
const TIMING_LEFT_VALUE_WIDTH: f32 = 220.0;
const TIMING_RIGHT_LABEL_WIDTH: f32 = 88.0;
const TIMING_RIGHT_VALUE_WIDTH: f32 = 90.0;
const WHY_HEIGHT: f32 = 132.0;
const BUS_TEACHER_WIDTH: f32 = 1220.0;
const BUS_TEACHER_HEIGHT: f32 = 760.0;

#[derive(Clone, Copy, Default)]
struct BusTeacherUiState {
    window_open: bool,
    freeze_display: bool,
    frozen_snapshot: Option<BusTeachingSnapshot>,
}

impl RusTairApp {
    fn bus_teacher_state(ctx: &egui::Context) -> BusTeacherUiState {
        ctx.data(|data| {
            data.get_temp::<BusTeacherUiState>(egui::Id::new("rustair-bus-teacher-state"))
                .unwrap_or_default()
        })
    }

    fn store_bus_teacher_state(ctx: &egui::Context, state: BusTeacherUiState) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("rustair-bus-teacher-state"), state);
        });
    }

    pub(in crate::app) fn open_bus_teacher(&mut self, ctx: &egui::Context) {
        let mut state = Self::bus_teacher_state(ctx);
        state.window_open = true;
        Self::store_bus_teacher_state(ctx, state);
    }

    fn bool_signal(value: Option<bool>) -> &'static str {
        match value {
            Some(true) => "ON / HIGH",
            Some(false) => "OFF / LOW",
            None => "?",
        }
    }

    fn wr_signal(value: Option<bool>) -> &'static str {
        match value {
            Some(false) => "LOW / ASSERTED",
            Some(true) => "HIGH / inactive",
            None => "?",
        }
    }

    fn hex16(value: Option<u16>) -> String {
        value
            .map(|value| format!("${value:04X}"))
            .unwrap_or_else(|| "----".into())
    }

    fn hex8(value: Option<u8>) -> String {
        value
            .map(|value| format!("${value:02X}"))
            .unwrap_or_else(|| "--".into())
    }

    fn visible_percent(value: f32) -> String {
        format!("{:>5.1}%", value.clamp(0.0, 1.0) * 100.0)
    }

    fn instruction_text_for_bus_snapshot(&mut self, snapshot: BusTeachingSnapshot) -> String {
        let Some(address) = snapshot.instruction_address else {
            return "Instruction: --".into();
        };
        let b0 = snapshot
            .opcode
            .or_else(|| self.machine.peek_memory(address))
            .unwrap_or(0);
        let b1 = self
            .machine
            .peek_memory(address.wrapping_add(1))
            .unwrap_or(0);
        let b2 = self
            .machine
            .peek_memory(address.wrapping_add(2))
            .unwrap_or(0);
        let decoded = decode_8080(b0, b1, b2);
        format!(
            "${address:04X}  {:<8}  {}",
            decoded.bytes_text([b0, b1, b2]),
            decoded.text(),
        )
    }

    fn draw_bus_teacher_header(
        &mut self,
        ctx: &egui::Context,
        live: Option<BusTeachingSnapshot>,
        state: &mut BusTeacherUiState,
    ) {
        egui::TopBottomPanel::top("bus-teacher-toolbar")
            .exact_height(38.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.strong("8080 BUS / T-STATE TEACHER");
                    ui.separator();
                    ui.label(self.machine.engine().label());
                    ui.separator();
                    let displayed = state.frozen_snapshot.or(live);
                    ui.strong(
                        displayed
                            .map(|snapshot| snapshot.accuracy.label())
                            .unwrap_or("NO SAMPLE"),
                    );
                    ui.separator();
                    if ui
                        .checkbox(&mut state.freeze_display, "Freeze sample")
                        .changed()
                    {
                        state.frozen_snapshot = if state.freeze_display { live } else { None };
                    }
                    ui.weak("Freeze affects this viewport only.");
                });
            });
    }

    fn draw_bus_teacher_source(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<BusTeachingSnapshot>,
    ) {
        let capabilities = self.machine.capabilities();
        ui.horizontal(|ui| {
            ui.strong("Engine");
            ui.label(self.machine.engine().label());
            ui.separator();
            let accuracy = snapshot.map(|snapshot| snapshot.accuracy.label()).unwrap_or(
                if capabilities.exact_t_state_timing {
                    "EXACT - no sample yet"
                } else {
                    "RECONSTRUCTED / APPROXIMATE"
                },
            );
            ui.strong(accuracy);
        });
        ui.small(if capabilities.exact_t_state_timing {
            "Cycle Accurate exposes the real machine-cycle, T-state, 8080 pins and latched S-100 status driven by the CPU-board sample."
        } else {
            "Fast 8080 is instruction-level: address/data and visible lamps are useful observations, while exact T-state/pin/status-latch values remain unknown."
        });
    }

    fn draw_bus_teacher_controls(&mut self, ui: &mut egui::Ui) {
        let panel = self.machine.front_panel_state();
        let cpu = self.machine.intel8080_state();
        let exact = self.machine.capabilities().exact_t_state_timing;
        let can_step = panel.powered && !panel.running && !cpu.halted.unwrap_or(false);

        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    panel.powered && !panel.running && !cpu.halted.unwrap_or(false),
                    egui::Button::new("Continue"),
                )
                .clicked()
            {
                self.machine.set_running(true);
            }
            if ui
                .add_enabled(
                    panel.powered && panel.running,
                    egui::Button::new("Pause"),
                )
                .clicked()
            {
                self.machine.set_running(false);
            }
            ui.separator();
            if ui
                .add_enabled(can_step && exact, egui::Button::new("Step T"))
                .on_hover_text("Advance exactly one 8080 T-state (Cycle Accurate only).")
                .clicked()
            {
                self.machine.debugger_step_t_state();
            }
            if ui
                .add_enabled(can_step && exact, egui::Button::new("Step M-cycle"))
                .on_hover_text("Advance one complete 8080 machine cycle (Cycle Accurate only).")
                .clicked()
            {
                self.machine.debugger_step_machine_cycle();
            }
            if ui
                .add_enabled(can_step, egui::Button::new("Step instruction"))
                .clicked()
            {
                self.machine.debugger_step_instruction();
            }
        });
        ui.small(
            "Debugger T/M stepping is educational. The physical Altair SINGLE STEP control remains one machine cycle in Cycle Accurate mode.",
        );
        if !exact {
            ui.small("Fast mode disables exact T-state and machine-cycle stepping.");
        }
    }

    fn draw_timing_row(
        ui: &mut egui::Ui,
        left_label: &str,
        left_value: &str,
        right_label: &str,
        right_value: &str,
    ) {
        ui.horizontal(|ui| {
            ui.add_sized(
                [TIMING_LEFT_LABEL_WIDTH, TIMING_ROW_HEIGHT],
                egui::Label::new(egui::RichText::new(left_label).strong()),
            );
            ui.add_sized(
                [TIMING_LEFT_VALUE_WIDTH, TIMING_ROW_HEIGHT],
                egui::Label::new(egui::RichText::new(left_value).monospace()),
            );
            ui.add_sized(
                [TIMING_RIGHT_LABEL_WIDTH, TIMING_ROW_HEIGHT],
                egui::Label::new(egui::RichText::new(right_label).strong()),
            );
            ui.add_sized(
                [TIMING_RIGHT_VALUE_WIDTH, TIMING_ROW_HEIGHT],
                egui::Label::new(egui::RichText::new(right_value).monospace()),
            );
        });
    }

    fn draw_bus_teacher_timing(&mut self, ui: &mut egui::Ui, snapshot: BusTeachingSnapshot) {
        ui.add_sized(
            [ui.available_width(), TIMING_ROW_HEIGHT],
            egui::Label::new(
                egui::RichText::new(self.instruction_text_for_bus_snapshot(snapshot)).monospace(),
            ),
        );
        ui.add_space(3.0);

        let machine_cycle = format!(
            "M{}  {}",
            snapshot
                .machine_cycle_index
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".into()),
            snapshot.machine_cycle.label(),
        );
        let address = Self::hex16(snapshot.address);
        let data = Self::hex8(snapshot.data);
        let status = Self::hex8(snapshot.status_word);
        let total_t_states = snapshot
            .total_t_states
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into());
        let instruction_t_states = snapshot
            .instruction_t_states
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into());

        // Do not use egui::Grid here. Grid column widths are content-driven and
        // a live transition such as INSTRUCTION FETCH -> MEMORY READ would move
        // the right-hand fields. These four fixed slots keep every X coordinate
        // stable while only the text inside each slot changes.
        Self::draw_timing_row(
            ui,
            "Machine cycle",
            &machine_cycle,
            "T-state",
            snapshot.t_state.label(),
        );
        Self::draw_timing_row(ui, "Address bus", &address, "Data bus", &data);
        Self::draw_timing_row(
            ui,
            "Latched status",
            &status,
            "Complete",
            Self::bool_signal(snapshot.instruction_complete),
        );
        Self::draw_timing_row(
            ui,
            "Total T-states",
            &total_t_states,
            "In instruction",
            &instruction_t_states,
        );
    }

    fn draw_pin_group(
        ui: &mut egui::Ui,
        id: &'static str,
        rows: &[(&'static str, &'static str, &'static str)],
    ) {
        egui::Grid::new(id)
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                for (name, value, note) in rows {
                    ui.add_sized(
                        [62.0, SIGNAL_ROW_HEIGHT],
                        egui::Label::new(egui::RichText::new(*name).strong().monospace()),
                    )
                    .on_hover_text(*note);
                    ui.add_sized(
                        [118.0, SIGNAL_ROW_HEIGHT],
                        egui::Label::new(egui::RichText::new(*value).monospace()),
                    )
                    .on_hover_text(*note);
                    ui.end_row();
                }
            });
    }

    fn draw_bus_teacher_pins(ui: &mut egui::Ui, snapshot: BusTeachingSnapshot) {
        let left = [
            ("SYNC", Self::bool_signal(snapshot.pins.sync), "T1 status synchronization"),
            ("DBIN", Self::bool_signal(snapshot.pins.dbin), "CPU is accepting input data"),
            ("/WR", Self::wr_signal(snapshot.pins.wr_n), "active-low CPU write output"),
            ("INTE", Self::bool_signal(snapshot.pins.inte), "interrupt-enable output"),
            ("WAIT", Self::bool_signal(snapshot.pins.wait), "processor wait output"),
        ];
        let right = [
            ("HLDA", Self::bool_signal(snapshot.pins.hlda), "bus hold acknowledged"),
            ("READY", Self::bool_signal(snapshot.ready), "S-100 READY presented to CPU"),
            ("HOLD", Self::bool_signal(snapshot.hold), "S-100 HOLD request"),
            ("RESET", Self::bool_signal(snapshot.reset), "CPU RESET input"),
        ];

        ui.columns(2, |columns| {
            let (left_column, right_column) = columns.split_at_mut(1);
            Self::draw_pin_group(&mut left_column[0], "bus-teacher-pins-left", &left);
            Self::draw_pin_group(&mut right_column[0], "bus-teacher-pins-right", &right);
        });
        ui.small("Hover a pin name/value for its electrical meaning.");
    }

    fn draw_status_group(
        ui: &mut egui::Ui,
        id: &'static str,
        rows: &[(&'static str, Option<bool>, f32)],
    ) {
        egui::Grid::new(id)
            .num_columns(3)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                ui.strong("SIGNAL");
                ui.strong("RAW");
                ui.strong("LED");
                ui.end_row();
                for (name, raw, visible) in rows {
                    ui.add_sized(
                        [62.0, SIGNAL_ROW_HEIGHT],
                        egui::Label::new(egui::RichText::new(*name).strong().monospace()),
                    );
                    ui.add_sized(
                        [96.0, SIGNAL_ROW_HEIGHT],
                        egui::Label::new(egui::RichText::new(Self::bool_signal(*raw)).monospace()),
                    );
                    ui.add_sized(
                        [58.0, SIGNAL_ROW_HEIGHT],
                        egui::Label::new(
                            egui::RichText::new(Self::visible_percent(*visible)).monospace(),
                        ),
                    );
                    ui.end_row();
                }
            });
    }

    fn draw_bus_teacher_status(ui: &mut egui::Ui, snapshot: BusTeachingSnapshot) {
        let lamps = snapshot.visible_lamps;
        let rows = [
            ("INTE", snapshot.status.inte, lamps.inte),
            ("PROT", snapshot.status.prot, lamps.prot),
            ("MEMR", snapshot.status.memr, lamps.memr),
            ("INP", snapshot.status.inp, lamps.inp),
            ("M1", snapshot.status.m1, lamps.m1),
            ("OUT", snapshot.status.out, lamps.out),
            ("HLTA", snapshot.status.hlta, lamps.hlta),
            ("STACK", snapshot.status.stack, lamps.stack),
            ("W/O", snapshot.status.wo, lamps.wo),
            ("INT", snapshot.status.int_ack, lamps.int_ack),
            ("WAIT", snapshot.status.wait, lamps.wait),
            ("HLDA", snapshot.status.hlda, lamps.hlda),
        ];

        ui.columns(2, |columns| {
            let (left_column, right_column) = columns.split_at_mut(1);
            Self::draw_status_group(
                &mut left_column[0],
                "bus-teacher-status-left",
                &rows[..6],
            );
            Self::draw_status_group(
                &mut right_column[0],
                "bus-teacher-status-right",
                &rows[6..],
            );
        });
        ui.small(
            "RAW is the captured S-100 status latch/electrical state. LED is optical persistence, so it may remain non-zero after RAW changes. W/O ON means read/input; OFF means write/output.",
        );
    }

    fn why_lines(snapshot: BusTeachingSnapshot) -> Vec<String> {
        let mut lines = Vec::new();
        match snapshot.machine_cycle {
            BusMachineCycle::InstructionFetch => {
                lines.push("M1 fetch: the processor is obtaining an opcode from memory.".into())
            }
            BusMachineCycle::MemoryRead => lines.push(
                "Memory read: the processor is obtaining an operand/data byte from memory.".into(),
            ),
            BusMachineCycle::MemoryWrite => lines.push(
                "Memory write: the processor is transferring a data byte to memory.".into(),
            ),
            BusMachineCycle::StackRead => lines.push(
                "Stack read: the processor is reading through SP, typically for POP/RET.".into(),
            ),
            BusMachineCycle::StackWrite => lines.push(
                "Stack write: the processor is writing through SP, typically for PUSH/CALL/RST."
                    .into(),
            ),
            BusMachineCycle::InputRead => lines.push(
                "Input read: the address bus encodes an I/O port and the peripheral supplies the data byte."
                    .into(),
            ),
            BusMachineCycle::OutputWrite => lines.push(
                "Output write: the address bus encodes an I/O port and the processor drives the output byte."
                    .into(),
            ),
            BusMachineCycle::InterruptAck | BusMachineCycle::InterruptAckWhileHalt => lines.push(
                "Interrupt acknowledge: the CPU is accepting an externally supplied instruction/vector."
                    .into(),
            ),
            BusMachineCycle::HaltAck => {
                lines.push("HALT acknowledge: the CPU has entered its halted bus sequence.".into())
            }
            BusMachineCycle::Internal => {
                lines.push(
                    "Internal cycle: this part of execution does not represent an external memory/I/O transfer."
                        .into(),
                );
                lines.push(
                    "No new S-100 status byte is emitted here, so the Display/Control status latch retains the previous value until the next status/SYNC update."
                        .into(),
                );
            }
            BusMachineCycle::Unknown => lines.push(
                "Fast mode cannot identify the exact machine cycle from an instruction-level snapshot."
                    .into(),
            ),
        }
        match snapshot.t_state {
            BusTState::T1 => lines.push(
                "T1: address is presented and SYNC/status identify the new machine cycle.".into(),
            ),
            BusTState::T2 => lines.push(
                "T2: bus control settles; READY determines whether execution can continue or must enter TW."
                    .into(),
            ),
            BusTState::Tw => lines.push(
                "TW: the CPU is waiting because READY was not accepted high for this transfer.".into(),
            ),
            BusTState::T3 => lines.push(
                "T3: the memory/I/O data transfer is sampled or committed.".into(),
            ),
            BusTState::T4 | BusTState::T5 => lines.push(format!(
                "{}: internal completion work for the current instruction/fetch cycle.",
                snapshot.t_state.label()
            )),
            BusTState::Halt => lines.push(
                "THALT: the processor is dwelling in HALT rather than executing a new instruction."
                    .into(),
            ),
            BusTState::Hold => lines.push(
                "THOLD: the processor has relinquished the bus after acknowledging HOLD.".into(),
            ),
            BusTState::Unknown => lines.push(
                "Exact T-state is unavailable in the instruction-level Fast core.".into(),
            ),
        }
        if snapshot.status.m1 == Some(true) {
            lines.push("M1 is ON because the retained S-100 status latch currently has M1 set.".into());
        }
        if snapshot.status.memr == Some(true) {
            lines.push("MEMR is ON because the retained S-100 status latch currently has MEMR set.".into());
        }
        if snapshot.status.inp == Some(true) {
            lines.push("INP is ON because the retained S-100 status latch identifies an input-port cycle.".into());
        }
        if snapshot.status.out == Some(true) {
            lines.push("OUT is ON because the retained S-100 status latch identifies an output-port cycle.".into());
        }
        if snapshot.status.stack == Some(true) {
            lines.push("STACK is ON because the retained S-100 status latch identifies stack activity.".into());
        }
        if snapshot.status.wo == Some(false) {
            lines.push("W/O is OFF: the physical /WO convention identifies a write/output cycle.".into());
        }
        if snapshot.status.wait == Some(true) {
            lines.push("WAIT is ON because the CPU is stopped/waiting on READY.".into());
        }
        if snapshot.status.hlda == Some(true) {
            lines.push("HLDA is ON because the CPU has granted the external HOLD request.".into());
        }
        if snapshot.status.prot == Some(true) {
            lines.push("PROT is ON because the currently latched S-100 address selects a write-protected 1 KiB memory block.".into());
        }
        lines
    }

    fn draw_bus_teacher_why(ui: &mut egui::Ui, snapshot: BusTeachingSnapshot) {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), WHY_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("bus-teacher-why-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in Self::why_lines(snapshot) {
                            ui.small(format!("- {line}"));
                        }
                    });
            },
        );
    }

    fn draw_bus_teacher_left_column(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<BusTeachingSnapshot>,
    ) {
        super::collapsible_section(ui, "Teaching source / accuracy", false, |ui| {
            self.draw_bus_teacher_source(ui, snapshot);
        });
        ui.separator();
        super::collapsible_section(ui, "Execution stepping", true, |ui| {
            self.draw_bus_teacher_controls(ui);
        });
        ui.separator();
        super::collapsible_section(ui, "Instruction / machine cycle / T-state", true, |ui| {
            if let Some(snapshot) = snapshot {
                self.draw_bus_teacher_timing(ui, snapshot);
            } else {
                ui.label("No sample yet. Pause Cycle Accurate and press Step T, or run briefly.");
            }
        });
        ui.separator();
        super::collapsible_section(ui, "Why are these signals active?", true, |ui| {
            if let Some(snapshot) = snapshot {
                Self::draw_bus_teacher_why(ui, snapshot);
            } else {
                ui.label("No captured signal sample to explain yet.");
            }
        });
    }

    fn draw_bus_teacher_right_column(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<BusTeachingSnapshot>,
    ) {
        super::collapsible_section(ui, "Intel 8080 pins", true, |ui| {
            if let Some(snapshot) = snapshot {
                Self::draw_bus_teacher_pins(ui, snapshot);
            } else {
                ui.label("No pin sample yet.");
            }
        });
        ui.separator();
        super::collapsible_section(ui, "S-100 status / front-panel LEDs", true, |ui| {
            if let Some(snapshot) = snapshot {
                Self::draw_bus_teacher_status(ui, snapshot);
            } else {
                ui.label("No S-100 sample yet.");
            }
        });
    }

    fn draw_bus_teacher_viewport_contents(
        &mut self,
        ctx: &egui::Context,
        state: &mut BusTeacherUiState,
    ) {
        let live = self.machine.bus_teaching_snapshot();
        self.draw_bus_teacher_header(ctx, live, state);

        if state.freeze_display {
            if state.frozen_snapshot.is_none() {
                state.frozen_snapshot = live;
            }
        } else {
            state.frozen_snapshot = None;
        }
        let snapshot = state.frozen_snapshot.or(live);

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("bus-teacher-main-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.columns(2, |columns| {
                        let (left_column, right_column) = columns.split_at_mut(1);
                        self.draw_bus_teacher_left_column(&mut left_column[0], snapshot);
                        self.draw_bus_teacher_right_column(&mut right_column[0], snapshot);
                    });
                });
        });
    }

    pub(in crate::app) fn show_bus_teacher_viewport(&mut self, parent_ctx: &egui::Context) {
        let mut state = Self::bus_teacher_state(parent_ctx);
        if !state.window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-8080-bus-t-state-teacher-viewport"),
            egui::ViewportBuilder::default()
                .with_title("RusTair - 8080 Bus / T-state Teacher")
                .with_inner_size([BUS_TEACHER_WIDTH, BUS_TEACHER_HEIGHT])
                .with_min_inner_size([900.0, 560.0])
                .with_resizable(true),
            |teacher_ctx, _class| {
                self.draw_bus_teacher_viewport_contents(teacher_ctx, &mut state);
                if teacher_ctx.input(|input| input.viewport().close_requested()) {
                    state.window_open = false;
                    state.freeze_display = false;
                    state.frozen_snapshot = None;
                }
            },
        );
        Self::store_bus_teacher_state(parent_ctx, state);
    }
}

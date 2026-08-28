use super::super::{egui, RusTairApp};
use crate::backend::{
    BusMachineCycle, BusTeachingAccuracy, BusTeachingSnapshot, BusTState,
};
use crate::decoder8080::decode_8080;

const SIGNAL_ROW_HEIGHT: f32 = 20.0;
const WHY_HEIGHT: f32 = 156.0;

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
        value.map(|value| format!("${value:04X}")).unwrap_or_else(|| "----".into())
    }

    fn hex8(value: Option<u8>) -> String {
        value.map(|value| format!("${value:02X}")).unwrap_or_else(|| "--".into())
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
        let b1 = self.machine.peek_memory(address.wrapping_add(1)).unwrap_or(0);
        let b2 = self.machine.peek_memory(address.wrapping_add(2)).unwrap_or(0);
        let decoded = decode_8080(b0, b1, b2);
        format!(
            "${address:04X}  {:<8}  {}",
            decoded.bytes_text([b0, b1, b2]),
            decoded.text(),
        )
    }

    fn draw_bus_teacher_source(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<BusTeachingSnapshot>,
        state: &mut BusTeacherUiState,
    ) {
        let capabilities = self.machine.capabilities();
        ui.horizontal(|ui| {
            ui.strong("Engine");
            ui.label(self.machine.engine().label());
            ui.separator();
            let accuracy = snapshot
                .map(|snapshot| snapshot.accuracy.label())
                .unwrap_or(if capabilities.exact_t_state_timing { "EXACT - no sample yet" } else { "RECONSTRUCTED / APPROXIMATE" });
            ui.strong(accuracy);
        });
        ui.small(if capabilities.exact_t_state_timing {
            "Cycle Accurate: machine cycle, T-state and 8080 pins come from the real TickTrace that drove the S-100 adapter."
        } else {
            "Fast 8080 is instruction-level. Address/data and visible lamps are useful observations, but exact T-state/pin values cannot be recovered and remain unknown."
        });
        ui.horizontal(|ui| {
            if ui.checkbox(&mut state.freeze_display, "Freeze displayed sample").changed() {
                state.frozen_snapshot = None;
            }
            ui.small("Freeze affects this viewport only; it never pauses the CPU.");
        });
    }

    fn draw_bus_teacher_controls(&mut self, ui: &mut egui::Ui) {
        let panel = self.machine.front_panel_state();
        let cpu = self.machine.intel8080_state();
        let exact = self.machine.capabilities().exact_t_state_timing;
        let can_step = panel.powered && !panel.running && !cpu.halted.unwrap_or(false);

        ui.horizontal(|ui| {
            if ui.add_enabled(panel.powered && !panel.running && !cpu.halted.unwrap_or(false), egui::Button::new("Continue")).clicked() {
                self.machine.set_running(true);
            }
            if ui.add_enabled(panel.powered && panel.running, egui::Button::new("Pause CPU")).clicked() {
                self.machine.set_running(false);
            }
            ui.separator();
            if ui.add_enabled(can_step && exact, egui::Button::new("Step T-state")).clicked() {
                self.machine.debugger_step_t_state();
            }
            if ui.add_enabled(can_step && exact, egui::Button::new("Step machine cycle")).clicked() {
                self.machine.debugger_step_machine_cycle();
            }
            if ui.add_enabled(can_step, egui::Button::new("Step instruction")).clicked() {
                self.machine.debugger_step_instruction();
            }
        });
        ui.small("Step T-state is a debugger teaching control, not an original Altair front-panel switch. The physical SINGLE STEP switch remains one machine cycle in Cycle Accurate mode.");
        if !exact {
            ui.small("Fast mode disables T-state and machine-cycle debugger stepping because those boundaries are not physically represented by the instruction-level core.");
        }
    }

    fn draw_bus_teacher_timing(&mut self, ui: &mut egui::Ui, snapshot: BusTeachingSnapshot) {
        ui.monospace(self.instruction_text_for_bus_snapshot(snapshot));
        egui::Grid::new("bus-teacher-timing-grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Machine cycle");
                ui.monospace(format!(
                    "M{}  {}",
                    snapshot.machine_cycle_index.map(|value| value.to_string()).unwrap_or_else(|| "?".into()),
                    snapshot.machine_cycle.label(),
                ));
                ui.end_row();
                ui.strong("T-state"); ui.monospace(snapshot.t_state.label()); ui.end_row();
                ui.strong("Address bus"); ui.monospace(Self::hex16(snapshot.address)); ui.end_row();
                ui.strong("Data bus"); ui.monospace(Self::hex8(snapshot.data)); ui.end_row();
                ui.strong("Status word"); ui.monospace(Self::hex8(snapshot.status_word)); ui.end_row();
                ui.strong("Total T-states"); ui.monospace(snapshot.total_t_states.map(|value| value.to_string()).unwrap_or_else(|| "?".into())); ui.end_row();
                ui.strong("T-states in instruction"); ui.monospace(snapshot.instruction_t_states.map(|value| value.to_string()).unwrap_or_else(|| "?".into())); ui.end_row();
                ui.strong("Instruction complete"); ui.monospace(Self::bool_signal(snapshot.instruction_complete)); ui.end_row();
            });
    }

    fn draw_bus_teacher_pins(ui: &mut egui::Ui, snapshot: BusTeachingSnapshot) {
        egui::Grid::new("bus-teacher-pins-grid")
            .num_columns(3)
            .spacing([12.0, 3.0])
            .show(ui, |ui| {
                for (name, value, note) in [
                    ("SYNC", Self::bool_signal(snapshot.pins.sync), "T1 status synchronization"),
                    ("DBIN", Self::bool_signal(snapshot.pins.dbin), "CPU is accepting input data"),
                    ("/WR", Self::wr_signal(snapshot.pins.wr_n), "active-low CPU write output"),
                    ("INTE", Self::bool_signal(snapshot.pins.inte), "interrupt-enable output"),
                    ("WAIT", Self::bool_signal(snapshot.pins.wait), "processor wait output"),
                    ("HLDA", Self::bool_signal(snapshot.pins.hlda), "bus hold acknowledged"),
                    ("READY", Self::bool_signal(snapshot.ready), "S-100 READY presented to CPU"),
                    ("HOLD", Self::bool_signal(snapshot.hold), "S-100 HOLD request"),
                    ("RESET", Self::bool_signal(snapshot.reset), "CPU RESET input"),
                ] {
                    ui.add_sized([70.0, SIGNAL_ROW_HEIGHT], egui::Label::new(egui::RichText::new(name).strong().monospace()));
                    ui.add_sized([132.0, SIGNAL_ROW_HEIGHT], egui::Label::new(egui::RichText::new(value).monospace()));
                    ui.add_sized([ui.available_width(), SIGNAL_ROW_HEIGHT], egui::Label::new(egui::RichText::new(note).small()));
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
        egui::Grid::new("bus-teacher-status-grid")
            .num_columns(3)
            .spacing([14.0, 3.0])
            .show(ui, |ui| {
                ui.strong("S-100 / PANEL"); ui.strong("RAW"); ui.strong("VISIBLE LED"); ui.end_row();
                for (name, raw, visible) in rows {
                    ui.add_sized([80.0, SIGNAL_ROW_HEIGHT], egui::Label::new(egui::RichText::new(name).strong().monospace()));
                    ui.add_sized([110.0, SIGNAL_ROW_HEIGHT], egui::Label::new(egui::RichText::new(Self::bool_signal(raw)).monospace()));
                    ui.add_sized([90.0, SIGNAL_ROW_HEIGHT], egui::Label::new(egui::RichText::new(Self::visible_percent(visible)).monospace()));
                    ui.end_row();
                }
            });
        ui.small("RAW is the electrical/status interpretation for the captured sample. VISIBLE LED is the optical/presentation integrator, so it may remain non-zero after the raw signal changed.");
        ui.small("W/O follows the original active-low /WO convention used by the panel: ON means read/input; OFF means write/output.");
    }

    fn why_lines(snapshot: BusTeachingSnapshot) -> Vec<String> {
        let mut lines = Vec::new();
        match snapshot.machine_cycle {
            BusMachineCycle::InstructionFetch => lines.push("M1 fetch: the processor is obtaining an opcode from memory.".into()),
            BusMachineCycle::MemoryRead => lines.push("Memory read: the processor is obtaining an operand/data byte from memory.".into()),
            BusMachineCycle::MemoryWrite => lines.push("Memory write: the processor is transferring a data byte to memory.".into()),
            BusMachineCycle::StackRead => lines.push("Stack read: the processor is reading through SP, typically for POP/RET.".into()),
            BusMachineCycle::StackWrite => lines.push("Stack write: the processor is writing through SP, typically for PUSH/CALL/RST.".into()),
            BusMachineCycle::InputRead => lines.push("Input read: the address bus encodes an I/O port and the peripheral supplies the data byte.".into()),
            BusMachineCycle::OutputWrite => lines.push("Output write: the address bus encodes an I/O port and the processor drives the output byte.".into()),
            BusMachineCycle::InterruptAck | BusMachineCycle::InterruptAckWhileHalt => lines.push("Interrupt acknowledge: the CPU is accepting an externally supplied instruction/vector.".into()),
            BusMachineCycle::HaltAck => lines.push("HALT acknowledge: the CPU has entered its halted bus sequence.".into()),
            BusMachineCycle::Internal => lines.push("Internal cycle: this part of execution does not represent an external memory/I/O transfer.".into()),
            BusMachineCycle::Unknown => lines.push("Fast mode cannot identify the exact machine cycle from an instruction-level snapshot.".into()),
        }
        match snapshot.t_state {
            BusTState::T1 => lines.push("T1: address is presented and SYNC/status identify the new machine cycle.".into()),
            BusTState::T2 => lines.push("T2: bus control settles; READY determines whether execution can continue or must enter TW.".into()),
            BusTState::Tw => lines.push("TW: the CPU is waiting because READY was not accepted high for this transfer.".into()),
            BusTState::T3 => lines.push("T3: the memory/I/O data transfer is sampled or committed.".into()),
            BusTState::T4 | BusTState::T5 => lines.push(format!("{}: internal completion work for the current instruction/fetch cycle.", snapshot.t_state.label())),
            BusTState::Halt => lines.push("THALT: the processor is dwelling in HALT rather than executing a new instruction.".into()),
            BusTState::Hold => lines.push("THOLD: the processor has relinquished the bus after acknowledging HOLD.".into()),
            BusTState::Unknown => lines.push("Exact T-state is unavailable in the instruction-level Fast core.".into()),
        }
        if snapshot.status.m1 == Some(true) { lines.push("M1 is ON because this is an instruction-fetch/status cycle.".into()); }
        if snapshot.status.memr == Some(true) { lines.push("MEMR is ON because memory is being read during this machine cycle.".into()); }
        if snapshot.status.inp == Some(true) { lines.push("INP is ON because this machine cycle reads an input port.".into()); }
        if snapshot.status.out == Some(true) { lines.push("OUT is ON because this machine cycle writes an output port.".into()); }
        if snapshot.status.stack == Some(true) { lines.push("STACK is ON because this transfer is classified as stack activity.".into()); }
        if snapshot.status.wo == Some(false) { lines.push("W/O is OFF: the physical /WO convention identifies a write/output cycle.".into()); }
        if snapshot.status.wait == Some(true) { lines.push("WAIT is ON because the CPU is stopped/waiting on READY.".into()); }
        if snapshot.status.hlda == Some(true) { lines.push("HLDA is ON because the CPU has granted the external HOLD request.".into()); }
        if snapshot.status.prot == Some(true) { lines.push("PROT is ON because the addressed 1 KiB memory block is write-protected.".into()); }
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

    fn draw_bus_teacher_viewport_contents(
        &mut self,
        ctx: &egui::Context,
        state: &mut BusTeacherUiState,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let live = self.machine.bus_teaching_snapshot();
            if state.freeze_display && state.frozen_snapshot.is_none() {
                state.frozen_snapshot = live;
            }
            if !state.freeze_display {
                state.frozen_snapshot = None;
            }
            let snapshot = state.frozen_snapshot.or(live);

            ui.horizontal(|ui| {
                ui.strong("8080 BUS / T-STATE TEACHER");
                ui.separator();
                ui.label(self.machine.engine().label());
                ui.separator();
                ui.label(snapshot.map(|snapshot| snapshot.accuracy.label()).unwrap_or("NO SAMPLE"));
            });
            ui.separator();

            super::collapsible_section(ui, "Teaching source / accuracy", true, |ui| {
                self.draw_bus_teacher_source(ui, snapshot, state);
            });
            ui.separator();
            super::collapsible_section(ui, "Execution stepping", true, |ui| {
                self.draw_bus_teacher_controls(ui);
            });

            let Some(snapshot) = snapshot else {
                ui.separator();
                super::collapsible_section(ui, "Instruction / machine cycle / T-state", true, |ui| {
                    ui.label("No exact Cycle sample exists yet. Pause the CPU and press Step T-state, or run the CPU briefly.");
                });
                return;
            };

            ui.separator();
            super::collapsible_section(ui, "Instruction / machine cycle / T-state", true, |ui| {
                self.draw_bus_teacher_timing(ui, snapshot);
            });
            ui.separator();
            super::collapsible_section(ui, "Intel 8080 pins", true, |ui| {
                Self::draw_bus_teacher_pins(ui, snapshot);
            });
            ui.separator();
            super::collapsible_section(ui, "S-100 status / front-panel LEDs", true, |ui| {
                Self::draw_bus_teacher_status(ui, snapshot);
            });
            ui.separator();
            super::collapsible_section(ui, "Why are these signals active?", true, |ui| {
                Self::draw_bus_teacher_why(ui, snapshot);
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
                .with_inner_size([920.0, 840.0])
                .with_min_inner_size([720.0, 600.0])
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

use super::super::{egui, RusTairApp};
use super::execution_position::current_instruction_address;
use crate::backend::{DebugStopReason, MemoryWatchAccess};
use crate::callstack8080::{infer_call_stack_8080, CallKind8080};
use crate::decoder8080::{decode_8080, ControlFlow};

const DEBUG_STATUS_LINE_HEIGHT: f32 = 20.0;
const DEBUG_LIST_HEIGHT: f32 = 105.0;

#[derive(Clone)]
struct DebuggerControlsUiState {
    window_open: bool,
    call_stack_open: bool,
    run_to_input: String,
    breakpoint_input: String,
    watchpoint_input: String,
    watchpoint_access: MemoryWatchAccess,
    message: Option<String>,
}

impl Default for DebuggerControlsUiState {
    fn default() -> Self {
        Self {
            window_open: false,
            call_stack_open: false,
            run_to_input: "0000".into(),
            breakpoint_input: "0000".into(),
            watchpoint_input: "0000".into(),
            watchpoint_access: MemoryWatchAccess::ReadWrite,
            message: None,
        }
    }
}

impl RusTairApp {
    fn debugger_controls_state(ctx: &egui::Context) -> DebuggerControlsUiState {
        ctx.data(|data| {
            data.get_temp::<DebuggerControlsUiState>(egui::Id::new("rustair-debugger-controls-state"))
                .unwrap_or_default()
        })
    }

    fn store_debugger_controls_state(ctx: &egui::Context, state: DebuggerControlsUiState) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("rustair-debugger-controls-state"), state);
        });
    }

    pub(in crate::app) fn open_debugger_controls(&mut self, ctx: &egui::Context) {
        let mut state = Self::debugger_controls_state(ctx);
        state.window_open = true;
        let cpu = self.machine.intel8080_state();
        let execution_address = current_instruction_address(self);
        state.run_to_input = format!("{execution_address:04X}");
        state.breakpoint_input = format!("{execution_address:04X}");
        state.watchpoint_input = format!("{:04X}", cpu.hl());
        Self::store_debugger_controls_state(ctx, state);
    }

    fn parse_debug_address(text: &str) -> Option<u16> {
        let text = text.trim();
        let text = text
            .strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .unwrap_or(text);
        let text = text
            .strip_suffix('h')
            .or_else(|| text.strip_suffix('H'))
            .unwrap_or(text);
        (!text.is_empty())
            .then(|| u16::from_str_radix(text, 16).ok())
            .flatten()
    }

    fn sanitize_debug_address_input(text: &mut String) {
        *text = text
            .chars()
            .filter(|c| c.is_ascii_hexdigit() || matches!(c, 'x' | 'X' | 'h' | 'H'))
            .collect::<String>()
            .to_uppercase();
    }

    fn current_debug_instruction(&mut self) -> Option<(u16, [u8; 3], crate::decoder8080::DecodedInstruction)> {
        let address = current_instruction_address(self);
        let b0 = self.machine.peek_memory(address)?;
        let b1 = self.machine.peek_memory(address.wrapping_add(1)).unwrap_or(0);
        let b2 = self.machine.peek_memory(address.wrapping_add(2)).unwrap_or(0);
        let decoded = decode_8080(b0, b1, b2);
        Some((address, [b0, b1, b2], decoded))
    }

    fn candidate_return_address(&mut self) -> Option<u16> {
        let sp = self.machine.intel8080_state().sp;
        let lo = self.machine.peek_memory(sp)?;
        let hi = self.machine.peek_memory(sp.wrapping_add(1))?;
        Some(u16::from_le_bytes([lo, hi]))
    }

    fn debugger_step_over(&mut self) -> String {
        let Some((pc, _, decoded)) = self.current_debug_instruction() else {
            return "Cannot decode the current instruction from installed RAM.".into();
        };
        let start_sp = self.machine.intel8080_state().sp;
        let return_address = pc.wrapping_add(u16::from(decoded.length));
        match decoded.control_flow {
            ControlFlow::Call { .. } | ControlFlow::Restart { .. } => {
                self.machine.debugger_run_to_with_sp(return_address, start_sp);
                format!(
                    "Step over: running to ${return_address:04X} only after SP returns to ${start_sp:04X}."
                )
            }
            _ => {
                self.machine.debugger_step_instruction();
                format!("Step over: {} is not a call, so execution advanced to the next instruction boundary.", decoded.text())
            }
        }
    }

    fn debugger_step_out(&mut self) -> String {
        let sp = self.machine.intel8080_state().sp;
        let Some(target) = self.candidate_return_address() else {
            return format!("Cannot read a two-byte return candidate from stack at SP=${sp:04X}.");
        };
        let caller_sp = sp.wrapping_add(2);
        self.machine.debugger_run_to_with_sp(target, caller_sp);
        format!(
            "Step out: running to candidate ${target:04X} only after SP advances from ${sp:04X} to ${caller_sp:04X}."
        )
    }

    fn debug_stop_reason_text(
        &mut self,
        running: bool,
        execution_address: u16,
    ) -> String {
        if running {
            return String::new();
        }
        let Some(reason) = self.machine.debugger_stop_reason() else {
            return String::new();
        };
        let relevant = match reason {
            DebugStopReason::ExecuteBreakpoint(address) | DebugStopReason::RunTo(address) => {
                execution_address == address
            }
            DebugStopReason::MemoryReadWatchpoint { .. }
            | DebugStopReason::MemoryWriteWatchpoint { .. } => true,
        };
        if relevant {
            format!("Stopped by debugger: {}.", reason.label())
        } else {
            String::new()
        }
    }

    fn draw_debugger_controls_viewport_contents(
        &mut self,
        ctx: &egui::Context,
        state: &mut DebuggerControlsUiState,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let cpu = self.machine.intel8080_state();
            let execution_address = current_instruction_address(self);
            let panel = self.machine.front_panel_state();
            let powered = panel.powered;
            let running = panel.running;
            let halted = cpu.halted.unwrap_or(false);
            let stopped_for_step = powered && !running && !halted;

            ui.horizontal(|ui| {
                ui.strong("8080 DEBUGGER");
                ui.separator();
                ui.label(format!("Core: {}", self.machine.engine().label()));
                ui.separator();
                ui.monospace(format!(
                    "PC(reg)=${:04X}  EXEC=${execution_address:04X}  SP=${:04X}",
                    cpu.pc, cpu.sp
                ));
                ui.separator();
                ui.label(if !powered { "POWER OFF" } else if halted { "HALTED" } else if running { "RUNNING" } else { "STOPPED" });
            });

            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(width, 44.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if let Some((pc, bytes, decoded)) = self.current_debug_instruction() {
                        ui.horizontal(|ui| {
                            ui.add_sized([70.0, 20.0], egui::Label::new(egui::RichText::new(format!("${pc:04X}")).monospace()));
                            ui.add_sized([92.0, 20.0], egui::Label::new(egui::RichText::new(decoded.bytes_text(bytes)).monospace()));
                            ui.add_sized([260.0, 20.0], egui::Label::new(egui::RichText::new(decoded.text()).monospace()));
                            ui.add_sized([ui.available_width(), 20.0], egui::Label::new(egui::RichText::new(decoded.flow_label()).small()));
                        });
                    } else {
                        ui.add_sized([width, 20.0], egui::Label::new("Current execution address is outside installed RAM."));
                    }
                    ui.add_sized(
                        [width, DEBUG_STATUS_LINE_HEIGHT],
                        egui::Label::new(egui::RichText::new(if execution_address != cpu.pc {
                            format!("Cycle state: PC register is ${:04X}; current instruction began at EXEC=${execution_address:04X}.", cpu.pc)
                        } else {
                            String::new()
                        }).small()),
                    );
                },
            );

            let stop_text = self.debug_stop_reason_text(running, execution_address);
            ui.add_sized([width, DEBUG_STATUS_LINE_HEIGHT], egui::Label::new(stop_text));
            let run_to_text = self
                .machine
                .debugger_run_to_target()
                .map(|target| format!("Active run-to/step target: ${target:04X}."))
                .unwrap_or_default();
            ui.add_sized([width, DEBUG_STATUS_LINE_HEIGHT], egui::Label::new(run_to_text));

            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(powered && !running && !halted, egui::Button::new("Continue")).clicked() {
                    self.machine.set_running(true);
                    state.message = Some("Execution resumed.".into());
                }
                if ui.add_enabled(powered && running, egui::Button::new("Pause")).clicked() {
                    self.machine.set_running(false);
                    state.message = Some("Execution paused; active run-to cancelled.".into());
                }
                if ui.add_enabled(stopped_for_step, egui::Button::new("Step instruction")).clicked() {
                    self.machine.debugger_step_instruction();
                    state.message = Some("Advanced to the next guest instruction boundary.".into());
                }
                if ui.add_enabled(stopped_for_step, egui::Button::new("Step over")).clicked() {
                    state.message = Some(self.debugger_step_over());
                }
                if ui.add_enabled(stopped_for_step, egui::Button::new("Step out")).clicked() {
                    state.message = Some(self.debugger_step_out());
                }
                if ui.button("Call stack").clicked() {
                    state.call_stack_open = true;
                    state.message = Some("Call stack opened in an independent window.".into());
                }
                if ui.button("Memory activity").clicked() {
                    self.open_memory_activity(ui.ctx());
                    state.message = Some("Memory activity opened in an independent window.".into());
                }
            });
            ui.small("Debugger Step instruction advances to the next 8080 instruction boundary. Step over/out also require the expected stack depth, so revisiting the same PC inside a deeper call does not stop early. On Cycle Accurate, the architectural PC may move during an instruction; EXEC remains the stable opcode boundary used by debugger controls.");

            let candidate_text = self.candidate_return_address()
                .map(|candidate| format!("Stack top candidate return: [SP=${:04X}] -> ${candidate:04X}. This is a conservative candidate, not a guaranteed call frame.", cpu.sp))
                .unwrap_or_else(|| format!("Stack top candidate return: [SP=${:04X}] -> -- (outside installed RAM).", cpu.sp));
            ui.add_sized([width, DEBUG_STATUS_LINE_HEIGHT], egui::Label::new(egui::RichText::new(candidate_text).small()));

            ui.separator();
            ui.strong("Run to address");
            ui.horizontal(|ui| {
                let response = ui.add_sized([90.0, 24.0], egui::TextEdit::singleline(&mut state.run_to_input).font(egui::TextStyle::Monospace).char_limit(6));
                if response.changed() { Self::sanitize_debug_address_input(&mut state.run_to_input); }
                if ui.add_enabled(powered && !halted, egui::Button::new("Run to")).clicked() {
                    if let Some(target) = Self::parse_debug_address(&state.run_to_input) {
                        self.machine.debugger_run_to(target);
                        state.message = Some(format!("Running to ${target:04X}; stop occurs before that opcode executes."));
                    } else { state.message = Some("Invalid run-to address.".into()); }
                }
                if ui.add_enabled(self.machine.debugger_run_to_target().is_some(), egui::Button::new("Cancel run-to")).clicked() {
                    self.machine.debugger_cancel_run_to();
                    state.message = Some("Run-to target cancelled; current RUN/STOP state was not changed.".into());
                }
                if ui.small_button("Use instruction").clicked() { state.run_to_input = format!("{execution_address:04X}"); }
            });

            ui.separator();
            ui.strong("Execute breakpoints");
            ui.horizontal(|ui| {
                let response = ui.add_sized([90.0, 24.0], egui::TextEdit::singleline(&mut state.breakpoint_input).font(egui::TextStyle::Monospace).char_limit(6));
                if response.changed() { Self::sanitize_debug_address_input(&mut state.breakpoint_input); }
                if ui.button("Add").clicked() {
                    if let Some(address) = Self::parse_debug_address(&state.breakpoint_input) {
                        self.machine.debugger_set_breakpoint(address, true);
                        state.message = Some(format!("Execute breakpoint armed at ${address:04X}. Breakpoints trigger only when that address is an opcode-fetch boundary."));
                    } else { state.message = Some("Invalid breakpoint address.".into()); }
                }
                if ui.button("Break at instruction").clicked() {
                    self.machine.debugger_set_breakpoint(execution_address, true);
                    state.breakpoint_input = format!("{execution_address:04X}");
                    state.message = Some(format!("Execute breakpoint armed at current instruction boundary ${execution_address:04X}."));
                }
                if ui.button("Clear all").clicked() {
                    self.machine.debugger_clear_breakpoints();
                    state.message = Some("All execute breakpoints cleared.".into());
                }
            });

            let breakpoints = self.machine.debugger_breakpoints();
            ui.allocate_ui_with_layout(
                egui::vec2(width, DEBUG_LIST_HEIGHT),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if breakpoints.is_empty() {
                        ui.small("No execute breakpoints.");
                    } else {
                        egui::ScrollArea::vertical().id_salt("debugger-breakpoint-list").auto_shrink([false, false]).show(ui, |ui| {
                            for address in breakpoints {
                                ui.horizontal(|ui| {
                                    ui.add_sized([72.0, 20.0], egui::Label::new(egui::RichText::new(format!("${address:04X}")).monospace()));
                                    let marker = match (address == execution_address, address == cpu.pc) {
                                        (true, true) => "EXEC/PC",
                                        (true, false) => "EXEC",
                                        (false, true) => "PC(reg)",
                                        (false, false) => "",
                                    };
                                    ui.add_sized([68.0, 20.0], egui::Label::new(egui::RichText::new(marker).small()));
                                    if ui.add_sized([66.0, 20.0], egui::Button::new("Remove")).clicked() {
                                        self.machine.debugger_set_breakpoint(address, false);
                                    }
                                });
                            }
                        });
                    }
                },
            );
            ui.small("Execute breakpoints stop at the true instruction boundary before fetching the opcode. In Cycle Accurate, PC(reg) can temporarily point inside the current instruction; EXEC is the stable opcode address.");

            ui.separator();
            ui.strong("Memory watchpoints");
            ui.horizontal(|ui| {
                let response = ui.add_sized([90.0, 24.0], egui::TextEdit::singleline(&mut state.watchpoint_input).font(egui::TextStyle::Monospace).char_limit(6));
                if response.changed() { Self::sanitize_debug_address_input(&mut state.watchpoint_input); }
                for (access, label) in [
                    (MemoryWatchAccess::Read, "Read"),
                    (MemoryWatchAccess::Write, "Write"),
                    (MemoryWatchAccess::ReadWrite, "R/W"),
                ] {
                    ui.selectable_value(&mut state.watchpoint_access, access, label);
                }
                if ui.button("Add / update").clicked() {
                    if let Some(address) = Self::parse_debug_address(&state.watchpoint_input) {
                        self.machine.debugger_set_watchpoint(address, Some(state.watchpoint_access));
                        state.message = Some(format!("{} watchpoint armed at ${address:04X}.", state.watchpoint_access.label()));
                    } else { state.message = Some("Invalid watchpoint address.".into()); }
                }
                if ui.small_button("Use HL").clicked() { state.watchpoint_input = format!("{:04X}", cpu.hl()); }
                if ui.small_button("Use SP").clicked() { state.watchpoint_input = format!("{:04X}", cpu.sp); }
                if ui.button("Clear all").clicked() {
                    self.machine.debugger_clear_watchpoints();
                    state.message = Some("All memory watchpoints cleared.".into());
                }
            });

            let watchpoints = self.machine.debugger_watchpoints();
            ui.allocate_ui_with_layout(
                egui::vec2(width, DEBUG_LIST_HEIGHT),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if watchpoints.is_empty() {
                        ui.small("No memory watchpoints.");
                    } else {
                        egui::ScrollArea::vertical().id_salt("debugger-watchpoint-list").auto_shrink([false, false]).show(ui, |ui| {
                            for (address, access) in watchpoints {
                                ui.horizontal(|ui| {
                                    ui.add_sized([72.0, 20.0], egui::Label::new(egui::RichText::new(format!("${address:04X}")).monospace()));
                                    ui.add_sized([88.0, 20.0], egui::Label::new(egui::RichText::new(access.label()).strong()));
                                    let markers = format!(
                                        "{}{}",
                                        if address == cpu.hl() { "HL/M " } else { "     " },
                                        if address == cpu.sp { "SP" } else { "  " },
                                    );
                                    ui.add_sized([72.0, 20.0], egui::Label::new(egui::RichText::new(markers).small()));
                                    if ui.add_sized([66.0, 20.0], egui::Button::new("Remove")).clicked() {
                                        self.machine.debugger_set_watchpoint(address, None);
                                    }
                                });
                            }
                        });
                    }
                },
            );
            ui.small("Watchpoints cover guest data-memory and stack accesses, not opcode/operand fetches. WRITE watchpoints observe the attempted bus transfer even if protection or missing RAM prevents a cell change. They stop after the causing instruction.");

            ui.separator();
            ui.add_sized(
                [width, DEBUG_STATUS_LINE_HEIGHT],
                egui::Label::new(egui::RichText::new(state.message.as_deref().unwrap_or("")).small()),
            );
        });
    }

    fn draw_call_stack_viewport_contents(&mut self, ctx: &egui::Context) {
        let history = self.machine.instruction_trace_snapshot();
        let metadata = self.machine.instruction_trace_metadata();
        let inferred = infer_call_stack_8080(&history, metadata);
        let cpu = self.machine.intel8080_state();
        let execution_address = current_instruction_address(self);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("INFERRED 8080 CALL STACK");
                ui.separator();
                ui.monospace(format!("PC=${:04X} EXEC=${execution_address:04X} SP=${:04X}", cpu.pc, cpu.sp));
                ui.separator();
                ui.label(format!("{} retained frame(s)", inferred.frames.len()));
            });
            ui.small("Frames are reconstructed only from observed CALL/RST/RET instructions in the retained capture window. No symbols or speculative frames are invented; callers that predate capture are never claimed as known.");
            ui.small("Asynchronous interrupt entry is not yet reconstructed as a call frame; a later unmatched RET will conservatively mark the inference incomplete.");
            ui.add_sized(
                [ui.available_width(), DEBUG_STATUS_LINE_HEIGHT],
                egui::Label::new(if inferred.incomplete {
                    "INCOMPLETE: a known trace loss or unmatched return prevents a complete reconstruction of the retained window."
                } else {
                    ""
                }),
            );
            ui.add_sized(
                [ui.available_width(), DEBUG_STATUS_LINE_HEIGHT],
                egui::Label::new(inferred.diagnostic.as_ref().map(|diagnostic| format!("Reason: {diagnostic}." )).unwrap_or_default()),
            );
            ui.separator();

            if inferred.frames.is_empty() {
                ui.label("No live call frames can be proven from the retained trace.");
            } else {
                egui::ScrollArea::vertical().id_salt("debugger-inferred-call-stack").show(ui, |ui| {
                    for (depth, frame) in inferred.frames.iter().enumerate().rev() {
                        let kind = match frame.kind { CallKind8080::Call => "CALL", CallKind8080::Restart => "RST" };
                        ui.horizontal(|ui| {
                            ui.add_sized([42.0, 20.0], egui::Label::new(egui::RichText::new(format!("#{depth:02}")).monospace()));
                            ui.add_sized([44.0, 20.0], egui::Label::new(egui::RichText::new(kind).strong()));
                            ui.add_sized([132.0, 20.0], egui::Label::new(egui::RichText::new(format!("${:04X} -> ${:04X}", frame.call_site, frame.target)).monospace()));
                            ui.add_sized([112.0, 20.0], egui::Label::new(egui::RichText::new(format!("return ${:04X}", frame.return_address)).monospace()));
                            ui.add_sized([142.0, 20.0], egui::Label::new(egui::RichText::new(format!("SP after ${:04X}", frame.stack_pointer_after_push)).monospace()));
                            ui.add_sized([ui.available_width(), 20.0], egui::Label::new(egui::RichText::new(format!("trace #{}", frame.sequence)).small()));
                        });
                    }
                });
            }

            ui.separator();
            if let Some(candidate) = self.candidate_return_address() {
                ui.small(format!("Current raw stack top: [SP=${:04X}] -> ${candidate:04X}. This value is shown independently from inferred frames.", cpu.sp));
            } else { ui.small("Current raw stack top cannot be read from installed RAM."); }
        });
    }

    fn show_call_stack_viewport(
        &mut self,
        parent_ctx: &egui::Context,
        state: &mut DebuggerControlsUiState,
    ) {
        if !state.call_stack_open { return; }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-8080-call-stack-viewport"),
            egui::ViewportBuilder::default()
                .with_title("RusTair - Intel 8080 Call Stack")
                .with_inner_size([760.0, 500.0])
                .with_min_inner_size([600.0, 360.0])
                .with_resizable(true),
            |stack_ctx, _class| {
                self.draw_call_stack_viewport_contents(stack_ctx);
                if stack_ctx.input(|input| input.viewport().close_requested()) { state.call_stack_open = false; }
            },
        );
    }

    pub(in crate::app) fn show_debugger_controls_viewport(&mut self, parent_ctx: &egui::Context) {
        let mut state = Self::debugger_controls_state(parent_ctx);

        if state.window_open {
            parent_ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("rustair-8080-debugger-controls-viewport"),
                egui::ViewportBuilder::default()
                    .with_title("RusTair - Intel 8080 Debugger")
                    .with_inner_size([900.0, 780.0])
                    .with_min_inner_size([760.0, 620.0])
                    .with_resizable(true),
                |debugger_ctx, _class| {
                    self.draw_debugger_controls_viewport_contents(debugger_ctx, &mut state);
                    if debugger_ctx.input(|input| input.viewport().close_requested()) { state.window_open = false; }
                },
            );
        }

        self.show_call_stack_viewport(parent_ctx, &mut state);
        self.show_memory_activity_viewport(parent_ctx);
        Self::store_debugger_controls_state(parent_ctx, state);
    }
}

pub(super) fn trace_requested(ctx: &egui::Context) -> bool {
    let state = RusTairApp::debugger_controls_state(ctx);
    state.window_open || state.call_stack_open
}

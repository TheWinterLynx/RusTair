from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"expected migration anchor not found in {path}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


# Exact samples remain historical. The current chassis plane is attached only
# when the host reads the observation.
replace_once(
    "src/backend/cycle.rs",
    "            instruction_complete: Some(trace.instruction_complete),\n            visible_lamps: self.machine.panel_lamps(),\n        });",
    "            instruction_complete: Some(trace.instruction_complete),\n            visible_lamps: self.machine.panel_lamps(),\n            current_chassis: None,\n        });",
)

replace_once(
    "src/backend/cycle_host.rs",
    "    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, BusCpuPins,\n    BusMachineCycle, BusStatusLines, BusTeachingAccuracy, BusTeachingSnapshot, BusTState, CpuState,",
    "    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, BusChassisSnapshot,\n    BusCpuPins, BusMachineCycle, BusStatusLines, BusTeachingAccuracy, BusTeachingSnapshot, BusTState, CpuState,",
)
replace_once(
    "src/backend/cycle_host.rs",
    "            instruction_complete: None,\n            visible_lamps: lamps,\n        }",
    "            instruction_complete: None,\n            visible_lamps: lamps,\n            current_chassis: None,\n        }",
)
replace_once(
    "src/backend/cycle_host.rs",
    "    fn bus_teaching_snapshot(&mut self) -> BackendResult<Option<BusTeachingSnapshot>> {\n        Ok(Some(\n            self.inner\n                .teaching_snapshot()\n                .unwrap_or_else(|| self.control_teaching_snapshot()),\n        ))\n    }",
    "    fn bus_teaching_snapshot(&mut self) -> BackendResult<Option<BusTeachingSnapshot>> {\n        let mut snapshot = self\n            .inner\n            .teaching_snapshot()\n            .unwrap_or_else(|| self.control_teaching_snapshot());\n        snapshot.current_chassis = Some(BusChassisSnapshot::from_altair_machine(\n            EmulationEngine::RustCycleAccurate8080,\n            self.inner.machine(),\n        ));\n        Ok(Some(snapshot))\n    }",
)

replace_once(
    "src/backend/mod.rs",
    "pub use bus_teaching::{\n    BusCpuPins, BusMachineCycle, BusStatusLines, BusTeachingAccuracy, BusTeachingSnapshot, BusTState,\n};",
    "pub use bus_teaching::{\n    BusChassisSnapshot, BusCpuPins, BusMachineCycle, BusStatusLines, BusTeachingAccuracy,\n    BusTeachingSnapshot, BusTState,\n};",
)

replace_once(
    "src/app/ui/bus_teacher.rs",
    "use crate::backend::{\n    BusMachineCycle, BusTeachingAccuracy, BusTeachingSnapshot, BusTState,\n};",
    "use crate::backend::{\n    BusChassisSnapshot, BusMachineCycle, BusTeachingAccuracy, BusTeachingSnapshot, BusTState,\n};",
)
replace_once(
    "src/app/ui/bus_teacher.rs",
    "                    ui.weak(\"Freeze affects this viewport only.\");",
    "                    ui.weak(\"Freeze locks LAST CPU SAMPLE only; CURRENT CHASSIS stays live.\");",
)

source_anchor = '''    fn draw_bus_teacher_controls(&mut self, ui: &mut egui::Ui) {'''
current_helper = '''    fn draw_current_chassis(ui: &mut egui::Ui, chassis: BusChassisSnapshot) {
        let run = if !chassis.powered {
            "POWER OFF"
        } else if chassis.running {
            "RUN"
        } else {
            "STOP"
        };
        let address = Self::hex16(chassis.address);
        let cpu_data = Self::hex8(chassis.cpu_data);
        let s100_di = Self::hex8(chassis.s100_di);
        let s100_do = Self::hex8(chassis.s100_do);
        let panel_data = Self::hex8(chassis.panel_data);
        let status = Self::hex8(chassis.status_word);

        ui.strong("CURRENT CHASSIS / S-100 (NOW)");
        Self::draw_timing_row(ui, "RUN latch", run, "READY", Self::bool_signal(chassis.ready));
        Self::draw_timing_row(ui, "INT/PINT", Self::bool_signal(chassis.interrupt), "HOLD", Self::bool_signal(chassis.hold));
        Self::draw_timing_row(ui, "RESET", Self::bool_signal(chassis.reset), "EXT CLR", Self::bool_signal(chassis.ext_clear));
        Self::draw_timing_row(ui, "S-100 address", &address, "Status", &status);
        Self::draw_timing_row(ui, "CPU D0-D7", &cpu_data, "S-100 DI", &s100_di);
        Self::draw_timing_row(ui, "S-100 DO", &s100_do, "Panel DATA", &panel_data);
        ui.small("This block is the present chassis instant. It is refreshed even when LAST CPU SAMPLE is frozen, and is never projected backward onto the historical DIP-40 T-state.");
    }

    fn draw_bus_teacher_controls(&mut self, ui: &mut egui::Ui) {'''
replace_once("src/app/ui/bus_teacher.rs", source_anchor, current_helper)

replace_once(
    "src/app/ui/bus_teacher.rs",
    '''    fn draw_bus_teacher_left_column(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<BusTeachingSnapshot>,
    ) {
        super::collapsible_section(ui, "Teaching source / accuracy", false, |ui| {
            self.draw_bus_teacher_source(ui, snapshot);
        });
        ui.separator();
        super::collapsible_section(ui, "Execution stepping", true, |ui| {''',
    '''    fn draw_bus_teacher_left_column(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<BusTeachingSnapshot>,
        current_chassis: Option<BusChassisSnapshot>,
    ) {
        super::collapsible_section(ui, "Teaching source / accuracy", false, |ui| {
            self.draw_bus_teacher_source(ui, snapshot);
        });
        ui.separator();
        super::collapsible_section(ui, "Current chassis / S-100 (now)", true, |ui| {
            if let Some(chassis) = current_chassis {
                Self::draw_current_chassis(ui, chassis);
            } else {
                ui.label("No current chassis state is available.");
            }
        });
        ui.separator();
        super::collapsible_section(ui, "Execution stepping", true, |ui| {''',
)

replace_once(
    "src/app/ui/bus_teacher.rs",
    '''        let snapshot = state.frozen_snapshot.or(live);

        egui::CentralPanel::default().show(ctx, |ui| {''',
    '''        // The displayed CPU/T-state sample may be frozen. The present-time
        // chassis plane is intentionally always taken from the fresh observation.
        let current_chassis = live.and_then(|snapshot| snapshot.current_chassis);
        let snapshot = state.frozen_snapshot.or(live);

        egui::CentralPanel::default().show(ctx, |ui| {''',
)
replace_once(
    "src/app/ui/bus_teacher.rs",
    "                        self.draw_bus_teacher_left_column(&mut left_column[0], snapshot);",
    "                        self.draw_bus_teacher_left_column(&mut left_column[0], snapshot, current_chassis);",
)
replace_once(
    "src/app/ui/bus_teacher.rs",
    '            _ if capabilities.exact_t_state_timing => ui.small(\n                "Cycle Accurate shows the last real CPU-board T-state sample. CPU D0-D7, S-100 DI0-DI7, S-100 DO0-DO7, status and CPU/control pins are captured as separate domains from the canonical backplane model.",\n            ),',
    '            _ if capabilities.exact_t_state_timing => ui.small(\n                "LAST CPU SAMPLE is the retained real CPU-board T-state. CURRENT CHASSIS is shown separately and may already differ after debugger pause, HOLD/PINT changes or another host-side control mutation.",\n            ),',
)

# Dynamic regressions prove the two time planes can disagree without either
# becoming false or mutating the exact CPU sample.
replace_once(
    "tests/bus_teaching.rs",
    '''#[test]
fn hold_request_after_exact_sample_does_not_rewrite_captured_input() {''',
    '''#[test]
fn exact_sample_and_current_chassis_are_distinct_after_debugger_pause() {
    let mut host = prepared(EmulationEngine::RustCycleAccurate8080, &[0x00]);
    host.debugger_step_t_state();

    let view = host.bus_teaching_snapshot().expect("dual-state teaching view");
    let current = view.current_chassis.expect("current chassis plane");
    assert_eq!(view.accuracy, BusTeachingAccuracy::Exact);
    assert_eq!(view.t_state, BusTState::T1);
    assert_eq!(view.ready, Some(true), "exact T1 retains sampled READY HIGH");
    assert!(!current.running, "debugger has already returned the chassis to STOP");
    assert_eq!(current.ready, Some(false), "present chassis READY follows STOP");
    assert_eq!(current.reset, Some(false));
}

#[test]
fn hold_request_after_exact_sample_does_not_rewrite_captured_input() {''',
)
replace_once(
    "tests/bus_teaching.rs",
    '''    let retained = host.bus_teaching_snapshot().expect("retained exact T1 sample");
    assert_eq!(retained.hold, Some(false), "HOLD is the value sampled at displayed T1, not a later request");
    assert_eq!(retained.t_state, BusTState::T1);''',
    '''    let retained = host.bus_teaching_snapshot().expect("retained exact T1 sample");
    assert_eq!(retained.hold, Some(false), "HOLD is the value sampled at displayed T1, not a later request");
    assert_eq!(retained.t_state, BusTState::T1);
    assert_eq!(retained.current_chassis.expect("current chassis").hold, Some(true), "live chassis must expose the later HOLD request separately");''',
)

# Static UI guard: freeze is sample-only and current chassis comes from the live
# observation, not from the frozen exact sample.
layout = Path("tests/bus_teacher_layout.rs")
layout_text = layout.read_text(encoding="utf-8")
marker = "fn bus_teacher_keeps_frozen_cpu_sample_separate_from_live_chassis"
if marker not in layout_text:
    layout_text += '''\n#[test]\nfn bus_teacher_keeps_frozen_cpu_sample_separate_from_live_chassis() {\n    assert!(BUS_TEACHER_SOURCE.contains("CURRENT CHASSIS / S-100 (NOW)"));\n    assert!(BUS_TEACHER_SOURCE.contains("Freeze locks LAST CPU SAMPLE only; CURRENT CHASSIS stays live."));\n    assert!(BUS_TEACHER_SOURCE.contains("let current_chassis = live.and_then(|snapshot| snapshot.current_chassis)"));\n    assert!(BUS_TEACHER_SOURCE.contains("state.frozen_snapshot.or(live)"));\n}\n'''
    layout.write_text(layout_text, encoding="utf-8")

use super::super::egui;
use crate::backend::{
    BusMachineCycle, BusTeachingAccuracy, BusTeachingSnapshot, BusTState,
};

const DIAGRAM_HEIGHT: f32 = 424.0;
const BODY_MIN_WIDTH: f32 = 150.0;
const BODY_MAX_WIDTH: f32 = 190.0;
const PIN_WIRE_MIN: f32 = 24.0;
const PIN_WIRE_MAX: f32 = 42.0;
const PIN_RADIUS: f32 = 3.6;

#[derive(Clone, Copy)]
enum ControlPin {
    Reset,
    Hold,
    Interrupt,
    Inte,
    Dbin,
    WrN,
    Sync,
    Wait,
    Ready,
    Hlda,
}

#[derive(Clone, Copy)]
enum PinKind {
    Address(u8),
    Data(u8),
    Control(ControlPin),
    Power(&'static str),
    Clock(&'static str),
    Ground,
}

#[derive(Clone, Copy)]
struct PinDef {
    number: u8,
    label: &'static str,
    kind: PinKind,
}

// Intel 8080A DIP-40 pinout, top to bottom on each package side.
const LEFT_PINS: [PinDef; 20] = [
    PinDef { number: 1, label: "A10", kind: PinKind::Address(10) },
    PinDef { number: 2, label: "GND", kind: PinKind::Ground },
    PinDef { number: 3, label: "D4", kind: PinKind::Data(4) },
    PinDef { number: 4, label: "D5", kind: PinKind::Data(5) },
    PinDef { number: 5, label: "D6", kind: PinKind::Data(6) },
    PinDef { number: 6, label: "D7", kind: PinKind::Data(7) },
    PinDef { number: 7, label: "D3", kind: PinKind::Data(3) },
    PinDef { number: 8, label: "D2", kind: PinKind::Data(2) },
    PinDef { number: 9, label: "D1", kind: PinKind::Data(1) },
    PinDef { number: 10, label: "D0", kind: PinKind::Data(0) },
    PinDef { number: 11, label: "-5V", kind: PinKind::Power("-5 V supply rail") },
    PinDef { number: 12, label: "RESET", kind: PinKind::Control(ControlPin::Reset) },
    PinDef { number: 13, label: "HOLD", kind: PinKind::Control(ControlPin::Hold) },
    PinDef { number: 14, label: "INT", kind: PinKind::Control(ControlPin::Interrupt) },
    PinDef { number: 15, label: "PHI2", kind: PinKind::Clock("Clock phase PHI2 is physically present when powered, but phase edges are below the emulator's T-state abstraction.") },
    PinDef { number: 16, label: "INTE", kind: PinKind::Control(ControlPin::Inte) },
    PinDef { number: 17, label: "DBIN", kind: PinKind::Control(ControlPin::Dbin) },
    PinDef { number: 18, label: "/WR", kind: PinKind::Control(ControlPin::WrN) },
    PinDef { number: 19, label: "SYNC", kind: PinKind::Control(ControlPin::Sync) },
    PinDef { number: 20, label: "+5V", kind: PinKind::Power("+5 V supply rail") },
];

const RIGHT_PINS: [PinDef; 20] = [
    PinDef { number: 40, label: "A11", kind: PinKind::Address(11) },
    PinDef { number: 39, label: "A14", kind: PinKind::Address(14) },
    PinDef { number: 38, label: "A13", kind: PinKind::Address(13) },
    PinDef { number: 37, label: "A12", kind: PinKind::Address(12) },
    PinDef { number: 36, label: "A15", kind: PinKind::Address(15) },
    PinDef { number: 35, label: "A9", kind: PinKind::Address(9) },
    PinDef { number: 34, label: "A8", kind: PinKind::Address(8) },
    PinDef { number: 33, label: "A7", kind: PinKind::Address(7) },
    PinDef { number: 32, label: "A6", kind: PinKind::Address(6) },
    PinDef { number: 31, label: "A5", kind: PinKind::Address(5) },
    PinDef { number: 30, label: "A4", kind: PinKind::Address(4) },
    PinDef { number: 29, label: "A3", kind: PinKind::Address(3) },
    PinDef { number: 28, label: "+12V", kind: PinKind::Power("+12 V supply rail") },
    PinDef { number: 27, label: "A2", kind: PinKind::Address(2) },
    PinDef { number: 26, label: "A1", kind: PinKind::Address(1) },
    PinDef { number: 25, label: "A0", kind: PinKind::Address(0) },
    PinDef { number: 24, label: "WAIT", kind: PinKind::Control(ControlPin::Wait) },
    PinDef { number: 23, label: "READY", kind: PinKind::Control(ControlPin::Ready) },
    PinDef { number: 22, label: "PHI1", kind: PinKind::Clock("Clock phase PHI1 is physically present when powered, but phase edges are below the emulator's T-state abstraction.") },
    PinDef { number: 21, label: "HLDA", kind: PinKind::Control(ControlPin::Hlda) },
];

struct PinState {
    level: Option<bool>,
    asserted: Option<bool>,
    state_text: String,
    note: &'static str,
    modeled: bool,
    static_pin: bool,
    released: bool,
}

/// The package renderer is deliberately view-only. CPU control-pin truth is
/// decided by the backend teaching snapshot; this UI never reconstructs a
/// signal from S-100 lamps, machine-cycle names or other presentation state.
fn control_state(snapshot: BusTeachingSnapshot, pin: ControlPin) -> (Option<bool>, bool, &'static str) {
    match pin {
        ControlPin::Reset => (snapshot.reset, false, "RESET input; active HIGH."),
        ControlPin::Hold => (snapshot.hold, false, "HOLD input requests that the 8080 relinquish the bus; active HIGH."),
        ControlPin::Interrupt => (snapshot.interrupt, false, "INT is the active-HIGH 8080 interrupt-request input. On the Altair it is driven by the canonical S-100 PINT line; it is distinct from the front-panel INT/SINTA interrupt-acknowledge status."),
        ControlPin::Inte => (snapshot.pins.inte, false, "INTE output indicates that maskable interrupts are enabled; active HIGH."),
        ControlPin::Dbin => (snapshot.pins.dbin, false, "DBIN output indicates that the CPU is accepting data from the external data bus; it remains HIGH through TW during a read wait."),
        ControlPin::WrN => (snapshot.pins.wr_n, true, "/WR is the active-LOW CPU write output. LOW means the write signal is asserted."),
        ControlPin::Sync => (snapshot.pins.sync, false, "SYNC marks the T1 status/synchronization portion of a machine cycle; active HIGH."),
        ControlPin::Wait => (snapshot.pins.wait, false, "WAIT output indicates that the processor is waiting; active HIGH."),
        ControlPin::Ready => (snapshot.ready, false, "READY input controls wait-state insertion; active HIGH."),
        ControlPin::Hlda => (snapshot.pins.hlda, false, "HLDA output acknowledges HOLD and bus relinquishment; active HIGH."),
    }
}

/// ADDRESS/DATA package-pin truth exists only for a real cycle-core sample, or
/// for the one lifecycle condition whose electrical ownership is independently
/// determined: RESET RELEASED / STOP-WAIT. A reconstructed Fast snapshot may
/// contain useful S-100/front-panel observations, but those values are never
/// projected back into the 8080 package.
fn cpu_bus_pin_levels_available(snapshot: BusTeachingSnapshot) -> bool {
    snapshot.accuracy == BusTeachingAccuracy::Exact
        || (snapshot.accuracy == BusTeachingAccuracy::ControlState
            && snapshot.machine_cycle == BusMachineCycle::ResetReleasedStopped)
}

fn exact_bus_is_released(snapshot: BusTeachingSnapshot, level: Option<bool>) -> bool {
    snapshot.accuracy == BusTeachingAccuracy::Exact && level.is_none()
}

fn pin_state(snapshot: BusTeachingSnapshot, pin: PinDef, powered: bool) -> PinState {
    match pin.kind {
        PinKind::Address(bit) => {
            let level = if cpu_bus_pin_levels_available(snapshot) {
                snapshot.address.map(|value| value & (1u16 << bit) != 0)
            } else {
                None
            };
            let released = exact_bus_is_released(snapshot, level);
            PinState {
                level,
                asserted: None,
                state_text: if released {
                    "HI-Z / RELEASED".into()
                } else {
                    level.map(|v| if v { "1" } else { "0" }).unwrap_or("NO T-STATE SAMPLE").into()
                },
                note: "8080 address-output pin. In an exact sample with no driven address the pin is HI-Z/released. RESET RELEASED / STOP-WAIT is a special stable control state: the CPU owns the address bus at PC=0000h, so those electrical levels are known even though no numbered T-state sample is fabricated.",
                modeled: level.is_some() || released,
                static_pin: false,
                released,
            }
        }
        PinKind::Data(bit) => {
            let level = if cpu_bus_pin_levels_available(snapshot) {
                snapshot.cpu_data.map(|value| value & (1u8 << bit) != 0)
            } else {
                None
            };
            let released = exact_bus_is_released(snapshot, level);
            PinState {
                level,
                asserted: None,
                state_text: if released {
                    "HI-Z / RELEASED".into()
                } else {
                    level.map(|v| if v { "1" } else { "0" }).unwrap_or("NO T-STATE SAMPLE").into()
                },
                note: "Intel 8080 bidirectional D0-D7 package pin. This level comes only from the backend's CPU-data domain, never from S-100 DI/DO or optical DATA-lamp persistence. During STOP-WAIT, memory DI passes through the CPU-board input buffer onto the processor D bus while DBIN is active.",
                modeled: level.is_some() || released,
                static_pin: false,
                released,
            }
        }
        PinKind::Control(control) => {
            let (level, active_low, note) = control_state(snapshot, control);
            let asserted = level.map(|value| if active_low { !value } else { value });
            let state_text = match (level, asserted) {
                (Some(true), Some(true)) => "HIGH ASSERTED".into(),
                (Some(false), Some(true)) => "LOW ASSERTED".into(),
                (Some(true), Some(false)) => "HIGH inactive".into(),
                (Some(false), Some(false)) => "LOW inactive".into(),
                _ => if powered { "UNKNOWN / NO SAMPLE".into() } else { "UNPOWERED".into() },
            };
            PinState { level, asserted, state_text, note, modeled: level.is_some(), static_pin: false, released: false }
        }
        PinKind::Power(note) => PinState {
            level: Some(powered),
            asserted: None,
            state_text: if powered { "POWER ON".into() } else { "POWER OFF".into() },
            note,
            modeled: true,
            static_pin: true,
            released: false,
        },
        PinKind::Clock(note) => PinState {
            level: Some(powered),
            asserted: None,
            state_text: if powered {
                "CLOCK PRESENT - phase not modeled".into()
            } else {
                "CLOCK OFF".into()
            },
            note,
            modeled: true,
            static_pin: true,
            released: false,
        },
        PinKind::Ground => PinState {
            level: Some(false),
            asserted: None,
            state_text: "0 V reference".into(),
            note: "Ground reference connection.",
            modeled: true,
            static_pin: true,
            released: false,
        },
        PinKind::Unmodeled(note) => PinState {
            level: None,
            asserted: None,
            state_text: if powered { "NOT WIRED / NOT MODELED".into() } else { "UNPOWERED".into() },
            note,
            modeled: false,
            static_pin: false,
            released: false,
        },
    }
}

fn address_bus_context(snapshot: BusTeachingSnapshot) -> &'static str {
    match snapshot.machine_cycle {
        BusMachineCycle::PowerOff => "bus unpowered",
        BusMachineCycle::PowerOnUndefined => "S-100 power-on value; CPU A pins undefined",
        BusMachineCycle::ResetAsserted => "front panel owns S-100 during RESET",
        BusMachineCycle::ResetReleasedStopped => "CPU -> S-100; stable STOP-WAIT fetch address",
        BusMachineCycle::ResetReleasedRunning => "S-100 reset-release state; first CPU T-state not sampled yet",
        _ if snapshot.pins.hlda == Some(true) || snapshot.t_state == BusTState::Hold => {
            "CPU bus released"
        }
        _ => "CPU -> S-100",
    }
}

fn draw_pin(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    body: egui::Rect,
    row: usize,
    left_side: bool,
    wire_len: f32,
    pin: PinDef,
    snapshot: BusTeachingSnapshot,
    powered: bool,
) {
    let spacing = body.height() / 21.0;
    let y = body.top() + spacing * (row as f32 + 1.0);
    let body_x = if left_side { body.left() } else { body.right() };
    let terminal_x = if left_side { body_x - wire_len } else { body_x + wire_len };
    let state = pin_state(snapshot, pin, powered);

    let visuals = ui.visuals();
    let high_color = visuals.selection.stroke.color;
    let low_color = visuals.widgets.inactive.fg_stroke.color.gamma_multiply(0.36);
    let neutral_color = visuals.widgets.noninteractive.fg_stroke.color.gamma_multiply(0.72);
    let unknown_color = visuals.widgets.inactive.fg_stroke.color.gamma_multiply(0.55);
    let released_color = visuals.widgets.noninteractive.fg_stroke.color.gamma_multiply(0.52);
    let asserted_color = visuals.warn_fg_color;

    let line_color = if state.released {
        released_color
    } else if state.static_pin {
        match state.level {
            Some(true) => high_color,
            Some(false) => low_color,
            None => neutral_color,
        }
    } else if !state.modeled {
        unknown_color
    } else {
        match state.level {
            Some(true) => high_color,
            Some(false) => low_color,
            None => unknown_color,
        }
    };
    let stroke = egui::Stroke::new(if state.asserted == Some(true) { 2.0_f32 } else if state.released { 1.0_f32 } else { 1.35_f32 }, line_color);
    painter.line_segment([egui::pos2(body_x, y), egui::pos2(terminal_x, y)], stroke);
    painter.circle_filled(egui::pos2(terminal_x, y), PIN_RADIUS, visuals.panel_fill);
    painter.circle_stroke(egui::pos2(terminal_x, y), PIN_RADIUS, egui::Stroke::new(1.5_f32, line_color));
    if state.asserted == Some(true) {
        painter.circle_stroke(
            egui::pos2(terminal_x, y),
            PIN_RADIUS + 3.0,
            egui::Stroke::new(1.25_f32, asserted_color),
        );
    }

    let pin_number_x = if left_side { body.left() + 8.0 } else { body.right() - 8.0 };
    painter.text(
        egui::pos2(pin_number_x, y),
        if left_side { egui::Align2::LEFT_CENTER } else { egui::Align2::RIGHT_CENTER },
        pin.number.to_string(),
        egui::FontId::monospace(10.0),
        visuals.widgets.noninteractive.fg_stroke.color.gamma_multiply(0.65),
    );

    let level_suffix = if state.static_pin {
        ""
    } else if state.released {
        " Z"
    } else if !state.modeled {
        " ?"
    } else {
        match state.level {
            Some(true) => " 1",
            Some(false) => " 0",
            None => " ?",
        }
    };
    let label = format!("{}{}", pin.label, level_suffix);
    let label_x = if left_side { terminal_x - 7.0 } else { terminal_x + 7.0 };
    painter.text(
        egui::pos2(label_x, y),
        if left_side { egui::Align2::RIGHT_CENTER } else { egui::Align2::LEFT_CENTER },
        label,
        egui::FontId::monospace(11.0),
        if state.asserted == Some(true) { asserted_color } else { line_color },
    );

    let hit_min_x = if left_side { terminal_x - 92.0 } else { body_x - 4.0 };
    let hit_max_x = if left_side { body_x + 4.0 } else { terminal_x + 92.0 };
    let hit = egui::Rect::from_min_max(
        egui::pos2(hit_min_x, y - spacing * 0.45),
        egui::pos2(hit_max_x, y + spacing * 0.45),
    );
    ui.interact(hit, ui.id().with(("8080-pin", pin.number)), egui::Sense::hover())
        .on_hover_ui(|ui| {
            ui.strong(format!("Pin {} - {}", pin.number, pin.label));
            ui.monospace(&state.state_text);
            ui.label(state.note);
            if state.released {
                ui.label("Pin is electrically released (high impedance) in this exact T-state.");
            } else if state.asserted == Some(true) {
                ui.label("Signal is ASSERTED in this sample.");
            } else if state.asserted == Some(false) {
                ui.label("Signal is inactive in this sample.");
            }
        });
}

fn hex8(value: Option<u8>) -> String {
    value
        .map(|value| format!("${value:02X}  {value:08b}"))
        .unwrap_or_else(|| "--  --------".into())
}

fn draw_bus_summary(ui: &mut egui::Ui, snapshot: BusTeachingSnapshot) {
    let address = snapshot.address.map(|value| format!("${value:04X}  {value:016b}")).unwrap_or_else(|| "----  ----------------".into());

    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.add_sized([78.0, 20.0], egui::Label::new(egui::RichText::new("STATE").strong()));
        ui.monospace(snapshot.machine_cycle.label());
    });
    ui.horizontal(|ui| {
        ui.add_sized([78.0, 20.0], egui::Label::new(egui::RichText::new("ADDRESS").strong()));
        ui.add_sized([196.0, 20.0], egui::Label::new(egui::RichText::new(address).monospace()));
        ui.weak(address_bus_context(snapshot));
    });
    ui.horizontal(|ui| {
        ui.add_sized([78.0, 20.0], egui::Label::new(egui::RichText::new("CPU D").strong()));
        ui.add_sized([128.0, 20.0], egui::Label::new(egui::RichText::new(hex8(snapshot.cpu_data)).monospace()));
        ui.weak("Intel 8080 package D0-D7");
    });
    ui.horizontal(|ui| {
        ui.add_sized([78.0, 20.0], egui::Label::new(egui::RichText::new("S-100 DI").strong()));
        ui.add_sized([128.0, 20.0], egui::Label::new(egui::RichText::new(hex8(snapshot.s100_di)).monospace()));
        ui.weak("toward processor / memory or I/O -> CPU board");
    });
    ui.horizontal(|ui| {
        ui.add_sized([78.0, 20.0], egui::Label::new(egui::RichText::new("S-100 DO").strong()));
        ui.add_sized([128.0, 20.0], egui::Label::new(egui::RichText::new(hex8(snapshot.s100_do)).monospace()));
        ui.weak("away from processor / CPU board -> memory or I/O");
    });
    ui.horizontal(|ui| {
        ui.add_sized([78.0, 20.0], egui::Label::new(egui::RichText::new("PANEL DATA").strong()));
        ui.add_sized([128.0, 20.0], egui::Label::new(egui::RichText::new(hex8(snapshot.panel_data)).monospace()));
        ui.weak("front-panel DATA display path; presentation may retain/integrate activity");
    });

    if snapshot.accuracy == BusTeachingAccuracy::ControlState {
        if snapshot.machine_cycle == BusMachineCycle::ResetReleasedStopped {
            ui.small("CONTROL STATE: stable STOP-WAIT. Memory drives S-100 DI, the CPU-board input buffer presents that byte on 8080 D0-D7 while DBIN is active, and DO is not the read-data source. No numbered T-state is fabricated.");
        } else {
            ui.small("CONTROL STATE: S-100/control levels above are current chassis observations. CPU package D0-D7 are shown only where that lifecycle state determines them without inventing a T-state.");
        }
    } else if snapshot.accuracy == BusTeachingAccuracy::Reconstructed {
        ui.small("RECONSTRUCTED: Fast mode can show the front-panel DATA observation, but DI, DO and 8080 D0-D7 remain unknown rather than being inferred from it.");
    } else if snapshot.accuracy == BusTeachingAccuracy::Exact && snapshot.cpu_data.is_none() {
        ui.small("Exact sample: 8080 D0-D7 are HI-Z/released now. S-100 DI/DO and the front-panel DATA presentation are separate domains and may legitimately show different values or retention.");
    } else {
        ui.small("Bright lead = HIGH, dim lead = LOW, Z = HI-Z/released, outer amber ring = signal ASSERTED. /WR demonstrates why LOW can still mean asserted. Gray '?' pins are deliberately not fabricated.");
    }
}

/// Draw the live Intel 8080A package from code so every pin remains tied to the
/// teaching contract rather than to a decorative/static image asset.
pub(super) fn draw_8080a_package(
    ui: &mut egui::Ui,
    snapshot: BusTeachingSnapshot,
    powered: bool,
) {
    let width = ui.available_width().max(360.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, DIAGRAM_HEIGHT), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();

    let body_width = (width * 0.34).clamp(BODY_MIN_WIDTH, BODY_MAX_WIDTH);
    let body_height = DIAGRAM_HEIGHT - 44.0;
    let body = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 20.0 + body_height * 0.5),
        egui::vec2(body_width, body_height),
    );
    let available_wire = ((width - body_width) * 0.5 - 104.0).max(PIN_WIRE_MIN);
    let wire_len = available_wire.clamp(PIN_WIRE_MIN, PIN_WIRE_MAX);

    painter.rect_filled(body, egui::CornerRadius::same(8), visuals.extreme_bg_color);
    painter.rect_stroke(
        body,
        egui::CornerRadius::same(8),
        egui::Stroke::new(
            1.7_f32,
            if powered {
                visuals.widgets.noninteractive.fg_stroke.color
            } else {
                visuals.widgets.inactive.fg_stroke.color.gamma_multiply(0.45)
            },
        ),
        egui::StrokeKind::Inside,
    );

    // DIP orientation notch and pin-1 locator echo the physical 8080A package.
    let notch = egui::pos2(body.center().x, body.top());
    painter.circle_filled(notch, 12.0, visuals.panel_fill);
    painter.circle_stroke(
        notch,
        12.0,
        egui::Stroke::new(1.3_f32, visuals.widgets.noninteractive.fg_stroke.color),
    );
    painter.circle_filled(
        egui::pos2(body.left() + 22.0, body.top() + 22.0),
        3.2,
        visuals.widgets.noninteractive.fg_stroke.color.gamma_multiply(0.7),
    );

    painter.text(
        body.center() - egui::vec2(0.0, 12.0),
        egui::Align2::CENTER_CENTER,
        "INTEL 8080A",
        egui::FontId::monospace(19.0),
        if powered { visuals.strong_text_color() } else { visuals.weak_text_color() },
    );
    painter.text(
        body.center() + egui::vec2(0.0, 12.0),
        egui::Align2::CENTER_CENTER,
        if powered { "DIP-40" } else { "POWER OFF" },
        egui::FontId::monospace(11.0),
        visuals.widgets.noninteractive.fg_stroke.color.gamma_multiply(0.7),
    );

    for (row, pin) in LEFT_PINS.iter().copied().enumerate() {
        draw_pin(ui, &painter, body, row, true, wire_len, pin, snapshot, powered);
    }
    for (row, pin) in RIGHT_PINS.iter().copied().enumerate() {
        draw_pin(ui, &painter, body, row, false, wire_len, pin, snapshot, powered);
    }

    draw_bus_summary(ui, snapshot);
}

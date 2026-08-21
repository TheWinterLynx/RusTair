mod asr33_controller;
mod commands;
mod runtime;
mod terminal_serial;
mod terminal_state;
mod ui;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};

use self::terminal_state::{TerminalSpeed, TerminalState};
use self::ui::assets::Tex;
use crate::audio::AudioEngine;
use crate::io::serial_router::{SerialEndpoint, SerialRouter};
use crate::machine::{AltairMachine, CLOCK_HZ};
use crate::peripherals::asr33::{
    self as teletype, Answerback, KeyKind, Mode as TtyMode, PrintEvent, Teletype,
};

const PANEL_W: f32 = 1935.0;
const PANEL_H: f32 = 813.0;
const TTY_W: f32 = teletype::IMAGE_W;
const TTY_H: f32 = teletype::IMAGE_H;

const PANEL_FRAME: Duration = Duration::from_millis(16);
const TTY_CHAR_TIME: Duration = Duration::from_millis(100);
const KEY_TAP_TIME: Duration = Duration::from_millis(50);
const PRINT_HEAD_STRIKE_TIME: Duration = Duration::from_millis(84);
const PRINT_HEAD_IMPACT_DELAY: Duration = Duration::from_millis(20);
const PRINT_HEAD_CARRIAGE_RETURN_TIME: Duration = Duration::from_millis(160);
const PAPER_FEED_TIME: Duration = Duration::from_millis(74);

const ADDR_LED_X: [f32; 16] = [
    1666.2, 1596.5, 1527.9, 1427.7, 1359.1, 1289.1, 1189.6, 1121.0,
    1052.7, 953.6, 884.8, 817.5, 718.5, 649.7, 579.9, 480.0,
];
const ADDR_LED_Y: f32 = 290.3;

const DATA_LED_X: [f32; 8] = [
    1666.9, 1597.6, 1528.6, 1427.6, 1358.9, 1289.4, 1191.0, 1121.1,
];
const DATA_LED_Y: f32 = 153.4;

const STATUS_LED_X: [f32; 10] = [
    277.0, 345.1, 410.8, 479.2, 548.5, 616.6, 683.6, 750.5, 818.4, 885.4,
];
const STATUS_LED_Y: f32 = 153.8;

const WAIT_LED: (f32, f32) = (276.6, 290.7);
const HLDA_LED: (f32, f32) = (344.9, 290.7);

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RusTair — MITS Altair 8800")
            .with_inner_size([1500.0, 820.0])
            .with_min_inner_size([950.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RusTair",
        options,
        Box::new(|cc| Ok(Box::new(RusTairApp::new(cc)))),
    )
}

struct RusTairApp {
    machine: AltairMachine,
    serial_router: SerialRouter,
    tex: Tex,
    tty: Teletype,
    tty_window_open: bool,
    terminal: TerminalState,
    tty_tx_started: Option<Instant>,
    tty_answerback: Answerback,
    audio: AudioEngine,
    last_tick: Instant,
    last_tape_tick: Instant,
    reset_flash_until: Option<Instant>,
    print_head_raise_until: Option<Instant>,
    print_head_impact_at: Option<Instant>,
    print_head_auto_return_at: Option<Instant>,
    print_head_glyph: u8,
    print_head_carriage_return_until: Option<Instant>,
    paper_feed_until: Option<Instant>,
    tty_power_flash_until: Option<Instant>,
    animated_key: Option<usize>,
    pressed_key: Option<usize>,
    key_auto_release_at: Option<Instant>,
    key_displacement: f32,
    key_anim_tick: Instant,
    status: String,
}

impl RusTairApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Tex::install_teletype_font(&cc.egui_ctx);
        let now = Instant::now();
        let switch_sprites = Self::load_switch_textures(&cc.egui_ctx);
        Self {
            machine: AltairMachine::default(),
            serial_router: SerialRouter::default(),
            tex: Tex::load(&cc.egui_ctx, switch_sprites),
            tty: Teletype::default(),
            tty_window_open: false,
            terminal: TerminalState::default(),
            tty_tx_started: None,
            tty_answerback: Answerback::default(),
            audio: AudioEngine::new(),
            last_tick: now,
            last_tape_tick: now,
            reset_flash_until: None,
            print_head_raise_until: None,
            print_head_impact_at: None,
            print_head_auto_return_at: None,
            print_head_glyph: b' ',
            print_head_carriage_return_until: None,
            paper_feed_until: None,
            tty_power_flash_until: None,
            animated_key: None,
            pressed_key: None,
            key_auto_release_at: None,
            key_displacement: 0.0,
            key_anim_tick: now,
            status: "Ready — modular front-panel switches".into(),
        }
    }

    fn image(ui: &mut egui::Ui, texture: &egui::TextureHandle, rect: Rect) {
        ui.painter().image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    fn centered_rect(origin: Pos2, scale: f32, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_center_size(
            origin + Vec2::new(x * scale, y * scale),
            Vec2::new(w * scale, h * scale),
        )
    }
}

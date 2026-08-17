mod cpu8080;
mod machine;

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};
use machine::{AltairMachine, CLOCK_HZ};
use rustair::audio::AudioEngine;
use rustair::teletype::{self, KeyKind, Mode as TtyMode, PrintEvent, Teletype};

const PANEL_W: f32 = 2048.0;
const PANEL_H: f32 = 869.0;
const TTY_W: f32 = teletype::IMAGE_W;
const TTY_H: f32 = teletype::IMAGE_H;

const PANEL_FRAME: Duration = Duration::from_millis(16);
const TTY_CHAR_TIME: Duration = Duration::from_millis(90);
const KEY_TAP_TIME: Duration = Duration::from_millis(75);

// Exact centres measured from the Wikimedia/Cromemco front-panel photograph.
// Arrays are indexed by the emulated bit number, hence bit 0 is the rightmost entry on the panel.
const SENSE_X: [f32; 16] = [
    1768., 1697., 1625., 1517., 1445., 1373., 1265., 1195.,
    1124., 1016., 946., 874., 766., 694., 624., 518.,
];
const SENSE_Y: f32 = 461.0;

const ADDR_LED_X: [f32; 16] = [
    1768., 1697., 1625., 1517., 1445., 1374., 1266., 1194.,
    1123., 1016., 944., 873., 766., 694., 623., 516.,
];
const ADDR_LED_Y: f32 = 322.0;

const DATA_LED_X: [f32; 8] = [1769., 1697., 1625., 1516., 1445., 1373., 1264., 1193.];
const DATA_LED_Y: f32 = 179.0;

const STATUS_LED_X: [f32; 10] = [300., 370., 442., 513., 585., 656., 727., 799., 871., 942.];
const STATUS_LED_Y: f32 = 179.0;

const WAIT_LED: (f32, f32) = (303., 321.);
const HLDA_LED: (f32, f32) = (374., 321.);

const POWER: (f32, f32) = (177., 602.);
const RUN_STOP: (f32, f32) = (520., 602.);
const SINGLE_STEP: (f32, f32) = (659., 602.);
const EXAMINE: (f32, f32) = (803., 602.);
const DEPOSIT: (f32, f32) = (943., 604.);
const RESET: (f32, f32) = (1087., 603.);
const PROTECT: (f32, f32) = (1231., 603.);
const AUX1: (f32, f32) = (1373., 603.);
const AUX2: (f32, f32) = (1516., 604.);

struct Tex {
    panel: Option<egui::TextureHandle>,
    panel_sprites: Option<egui::TextureHandle>,

    tty_body: Option<egui::TextureHandle>,
    tty_keys: Option<egui::TextureHandle>,
    tty_head: Option<egui::TextureHandle>,
    tty_line_local: Option<egui::TextureHandle>,
    tty_knob: Option<egui::TextureHandle>,
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RusTair — MITS Altair 8800")
            .with_inner_size([1500.0, 760.0])
            .with_min_inner_size([950.0, 480.0]),
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
    tex: Tex,
    tty: Teletype,
    tty_window_open: bool,
    audio: AudioEngine,

    last_tick: Instant,
    last_tape_tick: Instant,
    reset_flash_until: Option<Instant>,

    tty_tx_started: Option<Instant>,
    print_head_raise_until: Option<Instant>,
    tty_power_flash_until: Option<Instant>,

    animated_key: Option<usize>,
    pressed_key: Option<usize>,
    key_auto_release_at: Option<Instant>,
    key_displacement: f32,
    key_anim_tick: Instant,

    status: String,
}

impl RusTairApp {
    fn load_texture(ctx: &egui::Context, name: &str, path: &str) -> Option<egui::TextureHandle> {
        let bytes = std::fs::read(path).ok()?;
        let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        Some(ctx.load_texture(
            name,
            egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw()),
            egui::TextureOptions::LINEAR,
        ))
    }

    fn install_teletype_font(ctx: &egui::Context) {
        let Ok(bytes) = std::fs::read("assets/teletype.ttf") else { return; };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "teletype".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts.families.insert(
            FontFamily::Name("teletype".into()),
            vec!["teletype".to_owned()],
        );
        ctx.set_fonts(fonts);
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self::install_teletype_font(&cc.egui_ctx);
        let now = Instant::now();
        Self {
            machine: AltairMachine::default(),
            tex: Tex {
                panel: Self::load_texture(&cc.egui_ctx, "blue-panel", "assets/panels/blue/panel.jpg"),
                panel_sprites: Self::load_texture(&cc.egui_ctx, "blue-panel-sprites", "assets/panels/blue/sprites.png"),

                tty_body: Self::load_texture(&cc.egui_ctx, "tty-body", "assets/asr33 body.jpg"),
                tty_keys: Self::load_texture(&cc.egui_ctx, "tty-keys", "assets/asr33 keys.png"),
                tty_head: Self::load_texture(&cc.egui_ctx, "tty-head", "assets/asr33head.png"),
                tty_line_local: Self::load_texture(&cc.egui_ctx, "tty-line-local", "assets/asrlinelocal.png"),
                tty_knob: Self::load_texture(&cc.egui_ctx, "tty-knob", "assets/asrlinelocalknob.png"),
            },
            tty: Teletype::default(),
            tty_window_open: false,
            audio: AudioEngine::new(),
            last_tick: now,
            last_tape_tick: now,
            reset_flash_until: None,
            tty_tx_started: None,
            print_head_raise_until: None,
            tty_power_flash_until: None,
            animated_key: None,
            pressed_key: None,
            key_auto_release_at: None,
            key_displacement: 0.0,
            key_anim_tick: now,
            status: "Ready — Cromemco photographic panel (CC BY-SA 4.0)".into(),
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

    fn image_uv(ui: &mut egui::Ui, texture: &egui::TextureHandle, rect: Rect, uv: Rect) {
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    }

    fn sprite_uv(col: usize, row: usize) -> Rect {
        const COLS: f32 = 4.0;
        const ROWS: f32 = 3.0;
        let x0 = col as f32 / COLS;
        let y0 = row as f32 / ROWS;
        Rect::from_min_max(
            Pos2::new(x0, y0),
            Pos2::new((col + 1) as f32 / COLS, (row + 1) as f32 / ROWS),
        )
    }

    fn centered_rect(origin: Pos2, scale: f32, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_center_size(
            origin + Vec2::new(x * scale, y * scale),
            Vec2::new(w * scale, h * scale),
        )
    }
}

include!("app3_panel.rs");
include!("app3_tty_core.rs");
include!("app3_tty_draw.rs");
include!("app3_tty_io.rs");
include!("app3_update.rs");

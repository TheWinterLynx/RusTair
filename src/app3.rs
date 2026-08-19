mod cpu8080;
mod machine;

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};
use machine::{AltairMachine, CLOCK_HZ};
use rustair::audio::AudioEngine;
use rustair::teletype::{self, KeyKind, Mode as TtyMode, PrintEvent, Teletype};

const PANEL_W: f32 = 1774.0;
const PANEL_H: f32 = 887.0;
const TTY_W: f32 = teletype::IMAGE_W;
const TTY_H: f32 = teletype::IMAGE_H;

const PANEL_FRAME: Duration = Duration::from_millis(16);
const TTY_CHAR_TIME: Duration = Duration::from_millis(90);
const KEY_TAP_TIME: Duration = Duration::from_millis(75);

const SENSE_X: [f32; 16] = [
    1568., 1503., 1438., 1345., 1279., 1214., 1119., 1053.,
    987., 890., 826., 760., 661., 593., 526., 425.,
];
const SENSE_Y: f32 = 455.0;

const ADDR_LED_X: [f32; 16] = [
    1568., 1503., 1438., 1343., 1278., 1213., 1118., 1053.,
    988., 890., 827., 761., 661., 595., 527., 425.,
];
const ADDR_LED_Y: f32 = 306.0;

const DATA_LED_X: [f32; 8] = [1568., 1503., 1438., 1344., 1278., 1213., 1118., 1052.];
const DATA_LED_Y: f32 = 163.0;

const STATUS_LED_X: [f32; 10] = [215., 286., 354., 426., 493., 559., 626., 693., 760., 825.];
const STATUS_LED_Y: f32 = 164.0;

const WAIT_LED: (f32, f32) = (216., 307.);
const HLDA_LED: (f32, f32) = (287., 306.);

const POWER: (f32, f32) = (133., 597.);
const RUN_STOP: (f32, f32) = (430., 597.);
const SINGLE_STEP: (f32, f32) = (562., 597.);
const EXAMINE: (f32, f32) = (694., 597.);
const DEPOSIT: (f32, f32) = (826., 597.);
const RESET: (f32, f32) = (956., 597.);
const PROTECT: (f32, f32) = (1087., 597.);
const AUX1: (f32, f32) = (1214., 597.);
const AUX2: (f32, f32) = (1343., 597.);

#[derive(Clone, Copy)]
enum SwitchFamily {
    Red,
    White,
    Blue,
    Grey,
}

#[derive(Clone, Copy)]
enum SwitchPosition {
    Up,
    Center,
    Down,
}

struct Tex {
    panel: Option<egui::TextureHandle>,
    // The legacy atlas is retained only for the illuminated LED overlay.
    panel_sprites: Option<egui::TextureHandle>,

    // Every moving switch state is its own PNG. All states are rendered into
    // exactly the same destination rectangle, so changing state never changes
    // the runtime scale of the switch.
    switch_red: [Option<egui::TextureHandle>; 2],
    switch_white: [Option<egui::TextureHandle>; 2],
    switch_blue: [Option<egui::TextureHandle>; 3],
    switch_grey: [Option<egui::TextureHandle>; 3],

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

                switch_red: [
                    Self::load_texture(&cc.egui_ctx, "switch-red-up", "assets/panels/blue/switches/up_red.png"),
                    Self::load_texture(&cc.egui_ctx, "switch-red-down", "assets/panels/blue/switches/down_red.png"),
                ],
                switch_white: [
                    Self::load_texture(&cc.egui_ctx, "switch-white-up", "assets/panels/blue/switches/up_white.png"),
                    Self::load_texture(&cc.egui_ctx, "switch-white-down", "assets/panels/blue/switches/down_white.png"),
                ],
                switch_blue: [
                    Self::load_texture(&cc.egui_ctx, "switch-blue-up", "assets/panels/blue/switches/up_blue.png"),
                    Self::load_texture(&cc.egui_ctx, "switch-blue-center", "assets/panels/blue/switches/center_blue.png"),
                    Self::load_texture(&cc.egui_ctx, "switch-blue-down", "assets/panels/blue/switches/down_blue.png"),
                ],
                switch_grey: [
                    Self::load_texture(&cc.egui_ctx, "switch-grey-up", "assets/panels/blue/switches/up_grey.png"),
                    Self::load_texture(&cc.egui_ctx, "switch-grey-center", "assets/panels/blue/switches/center_grey.png"),
                    Self::load_texture(&cc.egui_ctx, "switch-grey-down", "assets/panels/blue/switches/down_grey.png"),
                ],

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
            status: "Ready — clean photographic Altair panel".into(),
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

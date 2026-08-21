#[allow(dead_code)]
mod cpu8080;
mod altair_machine;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};
use altair_machine::{AltairMachine, CLOCK_HZ};
use rustair::audio::AudioEngine;
use rustair::teletype::{self, KeyKind, Mode as TtyMode, PrintEvent, Teletype};

const PANEL_W: f32 = 1935.0;
const PANEL_H: f32 = 813.0;
const TTY_W: f32 = teletype::IMAGE_W;
const TTY_H: f32 = teletype::IMAGE_H;

const PANEL_FRAME: Duration = Duration::from_millis(16);
const TTY_CHAR_TIME: Duration = Duration::from_millis(90);
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

struct Tex {
    panel: Option<egui::TextureHandle>,
    switch_sprites: HashMap<SwitchSpriteId, egui::TextureHandle>,
    tty_body: Option<egui::TextureHandle>,
    tty_keys: Option<egui::TextureHandle>,
    tty_key_up: Option<egui::TextureHandle>,
    tty_key_mid: Option<egui::TextureHandle>,
    tty_key_down: Option<egui::TextureHandle>,
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
    terminal_window_open: bool,
    terminal_output: String,
    terminal_command: String,
    terminal_program: String,
    terminal_uppercase: bool,
    terminal_last_was_cr: bool,
    terminal_speed: TerminalSpeed,
    terminal_input_queue: VecDeque<u8>,
    terminal_rx_next_at: Option<Instant>,
    terminal_tx_started: Option<Instant>,
    tty_tx_started: Option<Instant>,
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

    /// Load one ASR-33 key pose while removing only transparent vertical padding.
    ///
    /// The GIMP UP/MID/DOWN files encode the press correctly by shortening the
    /// visible key from the top while retaining a common mechanical bottom. We
    /// therefore trim transparent rows so that the renderer can anchor that real
    /// lower edge. The original canvas width is deliberately preserved: cropping
    /// left/right made the visible cap suddenly fill the whole 170 px render box,
    /// enlarged every key by roughly one third and also discarded the horizontal
    /// registration authored in GIMP.
    fn load_key_pose_texture(
        ctx: &egui::Context,
        name: &str,
        path: &str,
    ) -> Option<egui::TextureHandle> {
        let bytes = std::fs::read(path).ok()?;
        let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let (width, height) = image.dimensions();

        let mut min_y = height;
        let mut max_y = 0u32;
        let mut found = false;

        // Ignore the almost-transparent anti-aliased fringe so a single halo
        // pixel cannot enlarge the vertical crop and reintroduce pose padding.
        const ALPHA_THRESHOLD: u8 = 8;
        for (_x, y, pixel) in image.enumerate_pixels() {
            if pixel[3] <= ALPHA_THRESHOLD {
                continue;
            }
            found = true;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }

        let cropped = if found {
            image::imageops::crop_imm(
                &image,
                0,
                min_y,
                width,
                max_y - min_y + 1,
            )
            .to_image()
        } else {
            image
        };

        let size = [cropped.width() as usize, cropped.height() as usize];
        Some(ctx.load_texture(
            name,
            egui::ColorImage::from_rgba_unmultiplied(size, &cropped.into_raw()),
            egui::TextureOptions::LINEAR,
        ))
    }

    fn load_switch_sprite_texture(ctx: &egui::Context, sprite: SwitchSpriteId) -> Option<egui::TextureHandle> {
        let asset = sprite.asset();
        let bytes = std::fs::read(asset.path).ok()?;
        let mut image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        if matches!(asset.alpha_mode, SwitchAlphaMode::RemoveBlack) {
            for pixel in image.pixels_mut() {
                let brightness = pixel[0].max(pixel[1]).max(pixel[2]);
                if brightness <= 2 {
                    pixel[3] = 0;
                } else if brightness < 16 {
                    pixel[3] = (((brightness - 2) as u16 * 255) / 14) as u8;
                }
            }
        }
        let size = [image.width() as usize, image.height() as usize];
        Some(ctx.load_texture(
            asset.path,
            egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw()),
            egui::TextureOptions::LINEAR,
        ))
    }

    fn load_switch_textures(ctx: &egui::Context) -> HashMap<SwitchSpriteId, egui::TextureHandle> {
        let mut textures = HashMap::new();
        for switch in SENSE_SWITCHES.iter().chain(CONTROL_SWITCHES.iter()) {
            for position in [SwitchPosition::Up, SwitchPosition::Center, SwitchPosition::Down] {
                let Some(pose) = switch.pose(position) else { continue; };
                let sprite = pose.sprite;
                if textures.contains_key(&sprite) { continue; }
                if let Some(texture) = Self::load_switch_sprite_texture(ctx, sprite) {
                    textures.insert(sprite, texture);
                }
            }
        }
        textures
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
                panel: Self::load_texture(&cc.egui_ctx, "front-panel", "assets/panels/white-pivot/panel.png"),
                switch_sprites: Self::load_switch_textures(&cc.egui_ctx),
                tty_body: Self::load_texture(&cc.egui_ctx, "tty-body", "assets/asr33_body_clean.png"),
                // The old full-keyboard overlay is deliberately disabled. The
                // clean body contains the key wells and each key is now painted
                // independently from three aligned photographic poses.
                tty_keys: None,
                tty_key_up: Self::load_key_pose_texture(&cc.egui_ctx, "tty-key-up", "assets/asr33_key_up.png"),
                tty_key_mid: Self::load_key_pose_texture(&cc.egui_ctx, "tty-key-mid", "assets/asr33_key_mid.png"),
                tty_key_down: Self::load_key_pose_texture(&cc.egui_ctx, "tty-key-down", "assets/asr33_key_down.png"),
                tty_head: Self::load_texture(&cc.egui_ctx, "tty-head", "assets/asr33head.png"),
                tty_line_local: Self::load_texture(&cc.egui_ctx, "tty-line-local", "assets/asrlinelocal.png"),
                tty_knob: Self::load_texture(&cc.egui_ctx, "tty-knob", "assets/asrlinelocalknob.png"),
            },
            tty: Teletype::default(),
            tty_window_open: false,
            terminal_window_open: false,
            terminal_output: String::new(),
            terminal_command: String::new(),
            terminal_program: String::new(),
            terminal_uppercase: true,
            terminal_last_was_cr: false,
            terminal_speed: TerminalSpeed::Baud9600,
            terminal_input_queue: VecDeque::new(),
            terminal_rx_next_at: None,
            terminal_tx_started: None,
            tty_tx_started: None,
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

include!("front_panel.rs");
// Keep the optional black-background cleanup path exercised so the enum variant
// remains part of the supported switch-asset pipeline without triggering dead-code warnings.
const _: SwitchAlphaMode = SwitchAlphaMode::RemoveBlack;
include!("teletype_controller.rs");
include!("teletype_renderer.rs");
include!("teletype_io.rs");
include!("terminal.rs");
include!("application_loop.rs");

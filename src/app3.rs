mod cpu8080;
mod machine;

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};
use machine::{AltairMachine, CLOCK_HZ};
use rustair::audio::AudioEngine;
use rustair::teletype::{self, KeyKind, Mode as TtyMode, PrintEvent, Teletype};

// Exact dimensions of the supplied clean panel asset. All panel coordinates
// below are expressed directly in this image's pixel coordinate system.
const PANEL_W: f32 = 1935.0;
const PANEL_H: f32 = 813.0;
const TTY_W: f32 = teletype::IMAGE_W;
const TTY_H: f32 = teletype::IMAGE_H;

const PANEL_FRAME: Duration = Duration::from_millis(16);
const TTY_CHAR_TIME: Duration = Duration::from_millis(90);
const KEY_TAP_TIME: Duration = Duration::from_millis(75);

// These coordinates are the previous proven hit/render locations transformed
// into the 1935 x 813 supplied panel coordinate system.
const SENSE_X: [f32; 16] = [
    1710.3, 1639.4, 1568.5, 1467.1, 1395.1, 1324.2, 1220.6, 1148.6,
    1076.6, 970.8, 901.0, 829.0, 721.0, 646.8, 573.7, 463.6,
];
const SENSE_Y: f32 = 417.0;

const ADDR_LED_X: [f32; 16] = [
    1710.3, 1639.4, 1568.5, 1464.9, 1394.0, 1323.1, 1219.5, 1148.6,
    1077.7, 970.8, 902.1, 830.1, 721.0, 649.0, 574.8, 463.6,
];
const ADDR_LED_Y: f32 = 280.5;

const DATA_LED_X: [f32; 8] = [1710.3, 1639.4, 1568.5, 1466.0, 1394.0, 1323.1, 1219.5, 1147.5];
const DATA_LED_Y: f32 = 149.4;

const STATUS_LED_X: [f32; 10] = [234.5, 312.0, 386.1, 464.7, 537.7, 609.7, 682.8, 755.9, 829.0, 899.9];
const STATUS_LED_Y: f32 = 150.3;

const WAIT_LED: (f32, f32) = (235.6, 281.4);
const HLDA_LED: (f32, f32) = (313.0, 280.5);

const POWER: (f32, f32) = (145.1, 547.2);
const RUN_STOP: (f32, f32) = (469.0, 547.2);
const SINGLE_STEP: (f32, f32) = (613.0, 547.2);
const EXAMINE: (f32, f32) = (757.0, 547.2);
const DEPOSIT: (f32, f32) = (901.0, 547.2);
const RESET: (f32, f32) = (1042.8, 547.2);
const PROTECT: (f32, f32) = (1185.7, 547.2);
const AUX1: (f32, f32) = (1324.2, 547.2);
const AUX2: (f32, f32) = (1464.9, 547.2);

#[derive(Clone, Copy)]
enum SwitchPosition {
    Up,
    Center,
    Down,
}

struct Tex {
    panel: Option<egui::TextureHandle>,

    // The only moving-switch textures used by this branch. Their original PNG
    // files are stored unchanged; all scaling/cropping/pivot work is runtime-only.
    switch_white: [Option<egui::TextureHandle>; 3],

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

    // The supplied CENTER PNG has a black canvas. Keep the file itself exactly
    // as supplied and remove that canvas only from the decoded runtime texture.
    // A luminance threshold is enough here: the ivory moving part is far above
    // it, and the fixed metal socket is provided by the panel photograph.
    fn load_center_switch_texture(
        ctx: &egui::Context,
        name: &str,
        path: &str,
    ) -> Option<egui::TextureHandle> {
        let bytes = std::fs::read(path).ok()?;
        let mut image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        for pixel in image.pixels_mut() {
            let brightness = pixel[0].max(pixel[1]).max(pixel[2]);
            if brightness <= 48 {
                pixel[3] = 0;
            } else if brightness < 80 {
                pixel[3] = (((brightness - 48) as u16 * 255) / 32) as u8;
            }
        }
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
            // AltairMachine::default() starts powered=false, running=false and
            // with address/data/wait LEDs cleared. The supplied panel asset also
            // contains only unlit lamps, so startup is visually all-dark.
            machine: AltairMachine::default(),
            tex: Tex {
                panel: Self::load_texture(
                    &cc.egui_ctx,
                    "white-pivot-panel",
                    "assets/panels/white-pivot/panel.png",
                ),
                switch_white: [
                    Self::load_texture(
                        &cc.egui_ctx,
                        "white-switch-up",
                        "assets/panels/white-pivot/switch_up.png",
                    ),
                    Self::load_center_switch_texture(
                        &cc.egui_ctx,
                        "white-switch-center",
                        "assets/panels/white-pivot/switch_center.png",
                    ),
                    Self::load_texture(
                        &cc.egui_ctx,
                        "white-switch-down",
                        "assets/panels/white-pivot/switch_down.png",
                    ),
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
            status: "Ready — fixed sockets, white pivot switches".into(),
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

include!("app3_panel.rs");
include!("app3_tty_core.rs");
include!("app3_tty_draw.rs");
include!("app3_tty_io.rs");
include!("app3_update.rs");
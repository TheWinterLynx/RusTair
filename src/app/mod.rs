mod asr33_controller;
mod asr33_state;
mod commands;
mod runtime;
mod terminal_controller;
mod terminal_serial;
mod terminal_state;
mod ui;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};

use self::asr33_state::Asr33State;
use self::terminal_state::{TerminalSpeed, TerminalState};
use self::ui::assets::Tex;
use crate::audio::AudioEngine;
use crate::config::{AppConfig, RamInit, RamSize, SerialBoard};
use crate::io::serial_router::{SerialEndpoint, SerialRouter};
use crate::machine::{AltairMachine, CLOCK_HZ};
use crate::peripherals::asr33::{
    self as teletype, KeyKind, Mode as TtyMode, PrintEvent, Teletype,
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
    config: AppConfig,
    machine: AltairMachine,
    serial_router: SerialRouter,
    tex: Tex,
    tty: Teletype,
    asr33: Asr33State,
    terminal: TerminalState,
    audio: AudioEngine,
    last_tick: Instant,
    reset_flash_until: Option<Instant>,
    status: String,
}

impl RusTairApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Tex::install_teletype_font(&cc.egui_ctx);
        let now = Instant::now();
        Self {
            config: AppConfig::default(),
            machine: AltairMachine::default(),
            serial_router: SerialRouter::default(),
            tex: Tex::load(&cc.egui_ctx),
            tty: Teletype::default(),
            asr33: Asr33State::new(now),
            terminal: TerminalState::default(),
            audio: AudioEngine::new(),
            last_tick: now,
            reset_flash_until: None,
            status: "Ready — 8 KiB RAM — MITS 88-SIO".into(),
        }
    }

    fn apply_memory_configuration(&mut self, ram_size: RamSize, ram_init: RamInit) {
        if self.config.machine.ram_size == ram_size && self.config.machine.ram_init == ram_init {
            return;
        }

        self.config.machine.ram_size = ram_size;
        self.config.machine.ram_init = ram_init;
        self.machine.configure_memory(ram_size, ram_init);
        self.asr33.tx_started = None;
        self.terminal.tx_started = None;
        self.reset_flash_until = None;
        self.status = format!(
            "Memory configured: {} — {}; machine reset",
            ram_size.label(),
            ram_init.label()
        );
    }

    fn apply_serial_board_configuration(&mut self, serial_board: SerialBoard) {
        if self.config.machine.serial_board == serial_board {
            return;
        }

        self.config.machine.serial_board = serial_board;
        self.machine.configure_serial_board(serial_board);
        self.asr33.tx_started = None;
        self.terminal.tx_started = None;
        self.reset_flash_until = None;

        // SerialRouter remains the cable selector for the single-port 88-SIO.
        // A fully populated 88-2SIO has two simultaneous physical ports, so the
        // ASR-33 owns Port 0 and the Text Terminal owns Port 1 independently.
        if serial_board == SerialBoard::TwoSio88 {
            self.serial_router.select(SerialEndpoint::InternalAsr33);
        }

        self.status = match serial_board {
            SerialBoard::Sio88 => {
                "Serial board configured: MITS 88-SIO — 00h/01h; one serial connection; machine reset"
                    .into()
            }
            SerialBoard::TwoSio88 => {
                "Serial board configured: MITS 88-2SIO — Port 0 10h/11h → ASR-33; Port 1 12h/13h → Text Terminal; machine reset"
                    .into()
            }
        };
    }

    fn terminal_uses_2sio_port1(&self) -> bool {
        self.config.machine.serial_board == SerialBoard::TwoSio88
    }

    fn terminal_serial_rx_empty(&self) -> bool {
        if self.terminal_uses_2sio_port1() {
            self.machine.bus.serial_port1_rx_empty()
        } else {
            self.machine.bus.serial_rx_empty()
        }
    }

    fn terminal_serial_receive(&mut self, byte: u8) {
        if self.terminal_uses_2sio_port1() {
            self.machine.bus.serial_port1_receive(byte);
        } else {
            self.machine.bus.serial_receive(byte);
        }
    }

    fn terminal_serial_tx_busy(&self) -> bool {
        if self.terminal_uses_2sio_port1() {
            self.machine.bus.serial_port1_tx_busy()
        } else {
            self.machine.bus.tx_busy()
        }
    }

    fn terminal_serial_tx_front(&self) -> Option<u8> {
        if self.terminal_uses_2sio_port1() {
            self.machine.bus.serial_port1_tx_front()
        } else {
            self.machine.bus.serial_tx_front()
        }
    }

    fn terminal_serial_tx_complete(&mut self) -> Option<u8> {
        if self.terminal_uses_2sio_port1() {
            self.machine.bus.serial_port1_tx_complete()
        } else {
            self.machine.bus.serial_tx_complete()
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

mod asr33_controller;
mod asr33_state;
mod commands;
mod cpu_diagnostics;
mod external_com;
mod external_serial;
mod runtime;
mod terminal_controller;
mod terminal_serial;
mod terminal_state;
mod ui;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};

use self::asr33_state::Asr33State;
use self::cpu_diagnostics::DiagnosticFileDialog;
use self::external_com::ExternalComState;
use self::external_serial::ExternalSerialState;
use self::terminal_state::TerminalState;
use self::ui::assets::Tex;
use crate::audio::AudioEngine;
use crate::config::{
    AppConfig, Asr33Speed, EmulationSpeed, RamInit, RamSize, SerialBoard, TerminalSpeed,
};
use crate::io::serial_router::{SerialConnection, SerialDevice, SerialRouter};
use crate::machine::{AltairMachine, CLOCK_HZ};
use crate::peripherals::asr33::{
    self as teletype, KeyKind, Mode as TtyMode, PrintEvent, Teletype,
};

const PANEL_W: f32 = 1935.0;
const PANEL_H: f32 = 813.0;
const TTY_W: f32 = teletype::IMAGE_W;
const TTY_H: f32 = teletype::IMAGE_H;

const PANEL_FRAME: Duration = Duration::from_millis(16);
const KEY_TAP_TIME: Duration = Duration::from_millis(50);
const PRINT_HEAD_STRIKE_TIME: Duration = Duration::from_millis(84);
const PRINT_HEAD_IMPACT_DELAY: Duration = Duration::from_millis(20);
const PRINT_HEAD_CARRIAGE_RETURN_TIME: Duration = Duration::from_millis(160);
const PAPER_FEED_TIME: Duration = Duration::from_millis(74);

const ADDR_LED_X: [f32; 16] = [
    1666.2, 1596.5, 1527.9, 1427.7, 1359.1, 1289.1, 1189.6, 1121.0, 1052.7, 953.6, 884.8,
    817.5, 718.5, 649.7, 579.9, 480.0,
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
        renderer: eframe::Renderer::Wgpu,
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
    external_serial: ExternalSerialState,
    external_com: ExternalComState,
    diagnostic_file_dialog: Option<DiagnosticFileDialog>,
    tex: Tex,
    tty: Teletype,
    asr33: Asr33State,
    terminal: TerminalState,
    audio: AudioEngine,
    last_tick: Instant,
    status: String,
}

impl RusTairApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Tex::install_teletype_font(&cc.egui_ctx);
        let now = Instant::now();
        let config = AppConfig::default();
        let mut terminal = TerminalState::default();
        terminal.speed = config.peripherals.terminal_speed;
        Self {
            config,
            machine: AltairMachine::default(),
            serial_router: SerialRouter::default(),
            external_serial: ExternalSerialState::default(),
            external_com: ExternalComState::default(),
            diagnostic_file_dialog: None,
            tex: Tex::load(&cc.egui_ctx),
            tty: Teletype::default(),
            asr33: Asr33State::new(now),
            terminal,
            audio: AudioEngine::new(),
            last_tick: now,
            status: "Ready — Intel 8080 @ 2 MHz — 8 KiB RAM — MITS 88-SIO — ASR-33 connected"
                .into(),
        }
    }

    fn asr_char_time(&self) -> Duration {
        self.config.peripherals.asr33_speed.char_time()
    }

    fn set_asr_speed(&mut self, speed: Asr33Speed) {
        if self.config.peripherals.asr33_speed == speed { return; }
        self.config.peripherals.asr33_speed = speed;
        self.asr33.tx_started = None;
        self.asr33.answerback.clear();
        self.status = format!("ASR-33 speed: {}", speed.label());
    }

    fn set_terminal_speed(&mut self, speed: TerminalSpeed) {
        if self.config.peripherals.terminal_speed == speed { return; }
        self.config.peripherals.terminal_speed = speed;
        self.terminal.speed = speed;
        self.terminal.tx_started = None;
        self.terminal.restart_input_pacing(Instant::now());
        self.status = format!("Text Terminal speed: {}", speed.label());
    }

    fn set_emulation_speed(&mut self, speed: EmulationSpeed) {
        if self.config.preferences.emulation_speed == speed { return; }
        self.config.preferences.emulation_speed = speed;
        self.last_tick = Instant::now();
        self.status = format!("CPU emulation speed: {}", speed.label());
    }

    fn apply_memory_configuration(&mut self, ram_size: RamSize, ram_init: RamInit) {
        if self.config.machine.ram_size == ram_size && self.config.machine.ram_init == ram_init { return; }
        self.config.machine.ram_size = ram_size;
        self.config.machine.ram_init = ram_init;
        self.machine.configure_memory(ram_size, ram_init);
        self.asr33.tx_started = None;
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
        self.status = format!("Memory configured: {} — {}; machine reset", ram_size.label(), ram_init.label());
    }

    fn apply_serial_board_configuration(&mut self, serial_board: SerialBoard) {
        if self.config.machine.serial_board == serial_board { return; }
        self.config.machine.serial_board = serial_board;
        self.machine.configure_serial_board(serial_board);
        self.serial_router.reset_for_board(serial_board);
        self.asr33.tx_started = None;
        self.asr33.answerback.clear();
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
        self.status = match serial_board {
            SerialBoard::Sio88 => "Serial board configured: MITS 88-SIO — ASR-33 connected to 00h/01h; machine reset".into(),
            SerialBoard::TwoSio88 => "Serial board configured: MITS 88-2SIO — ASR-33 → Port 0 (10h/11h), Text Terminal → Port 1 (12h/13h); machine reset".into(),
        };
    }

    fn serial_device_name(device: SerialDevice) -> &'static str {
        match device {
            SerialDevice::InternalAsr33 => "ASR-33",
            SerialDevice::TextTerminal => "Text Terminal",
            SerialDevice::ExternalTcp => "External TCP",
            SerialDevice::ExternalCom => "External COM",
        }
    }

    fn serial_connection_label(board: SerialBoard, connection: SerialConnection) -> &'static str {
        match (board, connection) {
            (_, SerialConnection::Disconnected) => "Disconnected",
            (SerialBoard::Sio88, SerialConnection::Port0) => "88-SIO [00h/01h]",
            (SerialBoard::Sio88, SerialConnection::Port1) => "Unavailable",
            (SerialBoard::TwoSio88, SerialConnection::Port0) => "88-2SIO Port 0 [10h/11h]",
            (SerialBoard::TwoSio88, SerialConnection::Port1) => "88-2SIO Port 1 [12h/13h]",
        }
    }

    fn serial_connection(&self, device: SerialDevice) -> SerialConnection {
        self.serial_router.connection(device)
    }

    fn set_serial_connection(&mut self, device: SerialDevice, connection: SerialConnection) {
        if self.config.machine.serial_board == SerialBoard::Sio88 && connection == SerialConnection::Port1 { return; }
        if self.serial_router.connection(device) == connection { return; }
        let displaced = self.serial_router.connect(device, connection);
        self.asr33.tx_started = None;
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
        if displaced == Some(SerialDevice::InternalAsr33)
            || (device == SerialDevice::InternalAsr33 && connection == SerialConnection::Disconnected)
        {
            self.asr33.answerback.clear();
        }
        let device_name = Self::serial_device_name(device);
        let connection_name = Self::serial_connection_label(self.config.machine.serial_board, connection);
        self.status = if let Some(displaced) = displaced {
            format!("{device_name} connected to {connection_name}; {} disconnected from that port", Self::serial_device_name(displaced))
        } else {
            format!("{device_name}: {connection_name}")
        };
    }

    fn serial_rx_empty_at(&self, connection: SerialConnection) -> bool {
        match connection {
            SerialConnection::Disconnected => true,
            SerialConnection::Port0 => self.machine.bus.serial_rx_empty(),
            SerialConnection::Port1 => self.machine.bus.serial_port1_rx_empty(),
        }
    }

    fn serial_rx_len_at(&self, connection: SerialConnection) -> usize {
        match connection {
            SerialConnection::Disconnected => 0,
            SerialConnection::Port0 => self.machine.bus.serial_rx_len(),
            SerialConnection::Port1 => self.machine.bus.serial_port1_rx_len(),
        }
    }

    fn serial_receive_at(&mut self, connection: SerialConnection, byte: u8) {
        match connection {
            SerialConnection::Disconnected => {}
            SerialConnection::Port0 => self.machine.bus.serial_receive(byte),
            SerialConnection::Port1 => self.machine.bus.serial_port1_receive(byte),
        }
    }

    fn serial_tx_busy_at(&self, connection: SerialConnection) -> bool {
        match connection {
            SerialConnection::Disconnected => false,
            SerialConnection::Port0 => self.machine.bus.tx_busy(),
            SerialConnection::Port1 => self.machine.bus.serial_port1_tx_busy(),
        }
    }

    fn serial_tx_front_at(&self, connection: SerialConnection) -> Option<u8> {
        match connection {
            SerialConnection::Disconnected => None,
            SerialConnection::Port0 => self.machine.bus.serial_tx_front(),
            SerialConnection::Port1 => self.machine.bus.serial_port1_tx_front(),
        }
    }

    fn serial_tx_complete_at(&mut self, connection: SerialConnection) -> Option<u8> {
        match connection {
            SerialConnection::Disconnected => None,
            SerialConnection::Port0 => self.machine.bus.serial_tx_complete(),
            SerialConnection::Port1 => self.machine.bus.serial_port1_tx_complete(),
        }
    }

    fn asr_connection(&self) -> SerialConnection {
        self.serial_connection(SerialDevice::InternalAsr33)
    }

    fn terminal_connection(&self) -> SerialConnection {
        self.serial_connection(SerialDevice::TextTerminal)
    }

    fn external_tcp_connection(&self) -> SerialConnection {
        self.serial_connection(SerialDevice::ExternalTcp)
    }

    fn external_com_connection(&self) -> SerialConnection {
        self.serial_connection(SerialDevice::ExternalCom)
    }
}

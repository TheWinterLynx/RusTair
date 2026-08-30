mod asr33_controller;
mod asr33_state;
mod authentic_loader;
mod commands;
mod cpu_diagnostics;
mod embedded_cpu_diagnostics;
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
use self::authentic_loader::AuthenticLoaderState;
use self::cpu_diagnostics::DiagnosticFileDialog;
use self::embedded_cpu_diagnostics::EmbeddedDiagnosticsState;
use self::external_com::ExternalComState;
use self::external_serial::ExternalSerialState;
use self::terminal_state::TerminalState;
use self::ui::assets::Tex;
use crate::audio::AudioEngine;
use crate::backend::{BackendHost, BackendSerialPort, EmulationEngine};
use crate::config::{
    AppConfig, Asr33Speed, EmulationSpeed, RamBoardProfile, RamInit, RamSize, SerialBoard,
    TerminalSpeed,
};
use crate::io::serial_router::{SerialConnection, SerialDevice, SerialRouter};
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
    machine: BackendHost,
    serial_router: SerialRouter,
    external_serial: ExternalSerialState,
    external_com: ExternalComState,
    diagnostic_file_dialog: Option<DiagnosticFileDialog>,
    embedded_diagnostics: EmbeddedDiagnosticsState,
    authentic_loader: AuthenticLoaderState,
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
        let cpu_board = config.machine.cpu_board();
        let cpu = cpu_board.cpu_model();
        let status = format!(
            "Ready — RusTair Fast 8080 — {} / {} @ {:.1} MHz — {} RAM — {} — ASR-33 connected",
            cpu_board.label(),
            cpu.label(),
            cpu_board.clock_hz() as f32 / 1_000_000.0,
            config.machine.ram_size.label(),
            config.machine.serial_board.label(),
        );
        let mut terminal = TerminalState::default();
        terminal.speed = config.peripherals.terminal_speed;
        Self {
            config,
            machine: BackendHost::rust_fast(),
            serial_router: SerialRouter::default(),
            external_serial: ExternalSerialState::default(),
            external_com: ExternalComState::default(),
            diagnostic_file_dialog: None,
            embedded_diagnostics: EmbeddedDiagnosticsState::default(),
            authentic_loader: AuthenticLoaderState::default(),
            tex: Tex::load(&cc.egui_ctx),
            tty: Teletype::default(),
            asr33: Asr33State::new(now),
            terminal,
            audio: AudioEngine::new(),
            last_tick: now,
            status,
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

    fn select_emulation_engine(&mut self, engine: EmulationEngine) {
        if self.machine.engine() == engine { return; }
        if self.machine.powered() {
            self.status = "Power OFF the Altair before changing emulation engine".into();
            return;
        }

        match self.machine.replace_engine(engine) {
            Ok(()) => {
                self.machine.configure_memory(
                    self.config.machine.ram_size,
                    self.config.machine.ram_init,
                );
                self.machine
                    .configure_memory_board_profile(self.config.machine.ram_board_profile);
                self.machine.configure_serial_board(self.config.machine.serial_board);
                self.asr33.tx_started = None;
                self.asr33.answerback.clear();
                self.terminal.tx_started = None;
                self.external_serial.reset_line_timing();
                self.external_com.reset_line_timing();
                self.last_tick = Instant::now();
                self.status = format!("Emulation engine selected: {} — machine remains POWER OFF", engine.label());
            }
            Err(error) => {
                self.status = format!("Could not select {}: {error}", engine.label());
            }
        }
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

    fn apply_memory_board_profile(&mut self, profile: RamBoardProfile) {
        if self.config.machine.ram_board_profile == profile { return; }
        if self.machine.powered() {
            self.status = "Power OFF the Altair before changing the installed RAM card timing".into();
            return;
        }
        self.config.machine.ram_board_profile = profile;
        self.machine.configure_memory_board_profile(profile);
        self.last_tick = Instant::now();
        self.status = format!("Memory card timing: {}", profile.label());
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

    fn backend_serial_port(connection: SerialConnection) -> Option<BackendSerialPort> {
        match connection {
            SerialConnection::Disconnected => None,
            SerialConnection::Port0 => Some(BackendSerialPort::Port0),
            SerialConnection::Port1 => Some(BackendSerialPort::Port1),
        }
    }

    fn serial_rx_empty_at(&mut self, connection: SerialConnection) -> bool {
        Self::backend_serial_port(connection)
            .map(|port| self.machine.serial_rx_empty(port))
            .unwrap_or(true)
    }

    fn serial_rx_len_at(&mut self, connection: SerialConnection) -> usize {
        Self::backend_serial_port(connection)
            .map(|port| self.machine.serial_rx_len(port))
            .unwrap_or(0)
    }

    fn serial_receive_at(&mut self, connection: SerialConnection, byte: u8) {
        if let Some(port) = Self::backend_serial_port(connection) {
            self.machine.serial_receive(port, byte);
        }
    }

    fn serial_tx_busy_at(&mut self, connection: SerialConnection) -> bool {
        Self::backend_serial_port(connection)
            .map(|port| self.machine.serial_tx_busy(port))
            .unwrap_or(false)
    }

    fn serial_tx_front_at(&mut self, connection: SerialConnection) -> Option<u8> {
        Self::backend_serial_port(connection)
            .and_then(|port| self.machine.serial_tx_front(port))
    }

    fn serial_tx_complete_at(&mut self, connection: SerialConnection) -> Option<u8> {
        Self::backend_serial_port(connection)
            .and_then(|port| self.machine.serial_tx_complete(port))
    }

    fn asr_connection(&self) -> SerialConnection {
        self.serial_connection(SerialDevice::InternalAsr33)
    }

    fn asr_serial_rx_empty(&mut self) -> bool { let c = self.asr_connection(); self.serial_rx_empty_at(c) }
    fn asr_serial_rx_len(&mut self) -> usize { let c = self.asr_connection(); self.serial_rx_len_at(c) }
    fn asr_serial_receive(&mut self, byte: u8) { let c = self.asr_connection(); self.serial_receive_at(c, byte); }
    fn asr_serial_tx_busy(&mut self) -> bool { let c = self.asr_connection(); self.serial_tx_busy_at(c) }
    fn asr_serial_tx_front(&mut self) -> Option<u8> { let c = self.asr_connection(); self.serial_tx_front_at(c) }
    fn asr_serial_tx_complete(&mut self) -> Option<u8> { let c = self.asr_connection(); self.serial_tx_complete_at(c) }

    fn terminal_connection(&self) -> SerialConnection {
        self.serial_connection(SerialDevice::TextTerminal)
    }

    fn terminal_serial_rx_empty(&mut self) -> bool { let c = self.terminal_connection(); self.serial_rx_empty_at(c) }
    fn terminal_serial_rx_len(&mut self) -> usize { let c = self.terminal_connection(); self.serial_rx_len_at(c) }
    fn terminal_serial_receive(&mut self, byte: u8) { let c = self.terminal_connection(); self.serial_receive_at(c, byte); }
    fn terminal_serial_tx_busy(&mut self) -> bool { let c = self.terminal_connection(); self.serial_tx_busy_at(c) }
    fn terminal_serial_tx_front(&mut self) -> Option<u8> { let c = self.terminal_connection(); self.serial_tx_front_at(c) }
    fn terminal_serial_tx_complete(&mut self) -> Option<u8> { let c = self.terminal_connection(); self.serial_tx_complete_at(c) }

    fn service_disconnected_serial_ports(&mut self) {
        if self.serial_router.device_on(SerialConnection::Port0).is_none()
            && self.machine.serial_tx_busy(BackendSerialPort::Port0)
        {
            self.machine.serial_tx_complete(BackendSerialPort::Port0);
        }
        if self.config.machine.serial_board == SerialBoard::TwoSio88
            && self.serial_router.device_on(SerialConnection::Port1).is_none()
            && self.machine.serial_tx_busy(BackendSerialPort::Port1)
        {
            self.machine.serial_tx_complete(BackendSerialPort::Port1);
        }
    }

    fn image(ui: &mut egui::Ui, texture: &egui::TextureHandle, rect: Rect) {
        ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }

    fn centered_rect(origin: Pos2, scale: f32, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_center_size(origin + Vec2::new(x * scale, y * scale), Vec2::new(w * scale, h * scale))
    }
}

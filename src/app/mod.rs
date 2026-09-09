mod asr33_controller;
mod asr33_state;
mod authentic_loader;
mod commands;
mod cpu_diagnostics;
mod embedded_cpu_diagnostics;
mod execution_clock;
mod execution_frame;
mod external_com;
mod external_serial;
mod runtime;
mod serial_hardware;
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
use self::execution_clock::ExecutionClock;
use self::external_com::ExternalComState;
use self::external_serial::ExternalSerialState;
use self::terminal_state::TerminalState;
use self::ui::assets::Tex;
use crate::audio::AudioEngine;
use crate::backend::{BackendHost, BackendSerialPort};
use crate::config::{
    AppConfig, Asr33Speed, CpuBoard, EmulationSpeed, RamInit, S100HardwareConfig,
    S100InstalledCardConfig, SerialBoard, TerminalSpeed, TwoSioStraps,
};
#[cfg(test)]
use crate::config::RamSize;
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

/// Speeds exposed for new user selections. X2 remains readable from historical
/// config files, but it is intentionally no longer offered in the UI.
const SELECTABLE_EMULATION_SPEEDS: [EmulationSpeed; 4] = [
    EmulationSpeed::Authentic,
    EmulationSpeed::X5,
    EmulationSpeed::X10,
    EmulationSpeed::Unlimited,
];

fn emulation_speed_label(speed: EmulationSpeed, board: CpuBoard) -> String {
    match speed {
        EmulationSpeed::Authentic => format!(
            "Authentic hardware clock — {:.1} MHz",
            board.clock_hz() as f32 / 1_000_000.0
        ),
        EmulationSpeed::X2 => "2× (legacy configuration)".into(),
        EmulationSpeed::X5 => "5×".into(),
        EmulationSpeed::X10 => "10×".into(),
        EmulationSpeed::Unlimited => "Unlimited".into(),
    }
}

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
    cpu_diagnostic_run_speed_label: Option<String>,
    embedded_diagnostics: EmbeddedDiagnosticsState,
    authentic_loader: AuthenticLoaderState,
    tex: Tex,
    tty: Teletype,
    asr33: Asr33State,
    terminal: TerminalState,
    audio: AudioEngine,
    last_tick: Instant,
    execution_clock: ExecutionClock,
    status: String,
}

impl RusTairApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Tex::install_teletype_font(&cc.egui_ctx);
        let now = Instant::now();
        let config = AppConfig::default();
        let cpu_board = config
            .machine
            .s100_hardware
            .active_cpu_board()
            .expect("default S-100 configuration has one CPU board");
        let cpu = cpu_board.cpu_model();
        let serial = config
            .machine
            .serial_board()
            .map_or("no serial card", SerialBoard::label);
        let status = format!(
            "Ready — RusTair Adaptive Cycle 8080 — {} / {} @ {:.1} MHz — {} KiB S-100 RAM — {}",
            cpu_board.label(),
            cpu.label(),
            cpu_board.clock_hz() as f32 / 1_000_000.0,
            config.machine.s100_hardware.installed_ram_bytes() / 1024,
            serial,
        );
        let mut terminal = TerminalState::default();
        terminal.speed = config.peripherals.terminal_speed;
        Self {
            config,
            machine: BackendHost::default(),
            serial_router: SerialRouter::default(),
            external_serial: ExternalSerialState::default(),
            external_com: ExternalComState::default(),
            diagnostic_file_dialog: None,
            cpu_diagnostic_run_speed_label: None,
            embedded_diagnostics: EmbeddedDiagnosticsState::default(),
            authentic_loader: AuthenticLoaderState::default(),
            tex: Tex::load(&cc.egui_ctx),
            tty: Teletype::default(),
            asr33: Asr33State::new(now),
            terminal,
            audio: AudioEngine::new(),
            last_tick: now,
            execution_clock: ExecutionClock::new(now),
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
        let now = Instant::now();
        self.last_tick = now;
        self.execution_clock.reset_at(now);
        let board = self
            .config
            .machine
            .s100_hardware
            .active_cpu_board()
            .expect("validated S-100 configuration has one CPU board");
        self.status = format!(
            "CPU emulation speed: {}",
            emulation_speed_label(speed, board)
        );
    }

    /// Apply one validated physical slot inventory to the active backend.
    ///
    /// This is the sole app-side hardware authority. CPU, RAM and serial identity
    /// and all card straps are materialized from the S-100 slots in one remount.
    fn apply_s100_hardware_configuration(&mut self, hardware: S100HardwareConfig, action: &str) {
        if self.machine.powered() {
            self.status = "POWER OFF required to move or reconfigure S-100 cards".into();
            return;
        }

        let previous = self.config.machine.s100_hardware;
        let old_asr_connection = self.asr_connection();
        if old_asr_connection.is_connected() {
            let _ = self.serial_set_receive_break_at(old_asr_connection, false);
        }

        self.machine
            .configure_s100_hardware(hardware, self.config.machine.ram_init);
        self.config.machine.s100_hardware = hardware;
        self.reconcile_serial_router_after_hardware_change(previous, hardware);

        let now = Instant::now();
        self.last_tick = now;
        self.execution_clock.reset_at(now);
        let serial = hardware
            .active_serial_board()
            .map_or("no serial card".to_string(), |board| board.label().to_string());
        self.status = format!(
            "{action} — live S-100 chassis remounted ({} KiB RAM, {serial}) — POWER remains OFF",
            hardware.installed_ram_bytes() / 1024
        );
    }

    /// Select how newly mounted/powered RAM is initialized without changing its
    /// physical card topology. This setting is an emulator convenience, not an
    /// aggregate RAM-size control.
    fn apply_ram_initialization(&mut self, ram_init: RamInit) {
        if self.config.machine.ram_init == ram_init { return; }
        if self.machine.powered() {
            self.status = "POWER OFF required before changing RAM power-on contents".into();
            return;
        }
        self.config.machine.ram_init = ram_init;
        self.machine
            .configure_s100_hardware(self.config.machine.s100_hardware, ram_init);
        let now = Instant::now();
        self.last_tick = now;
        self.execution_clock.reset_at(now);
        self.status = format!(
            "RAM power-on contents: {} — same S-100 card topology remounted",
            ram_init.label()
        );
    }

    fn reconcile_serial_router_after_hardware_change(
        &mut self,
        previous: S100HardwareConfig,
        next: S100HardwareConfig,
    ) {
        if previous.active_serial_board() != next.active_serial_board() {
            match next.active_serial_board() {
                Some(board) => self.serial_router.reset_for_board(board),
                None => {
                    for device in [
                        SerialDevice::InternalAsr33,
                        SerialDevice::TextTerminal,
                        SerialDevice::ExternalTcp,
                        SerialDevice::ExternalCom,
                    ] {
                        self.serial_router
                            .connect(device, SerialConnection::Disconnected);
                    }
                }
            }
        }

        for device in [
            SerialDevice::InternalAsr33,
            SerialDevice::TextTerminal,
            SerialDevice::ExternalTcp,
            SerialDevice::ExternalCom,
        ] {
            let connection = self.serial_router.connection(device);
            if !Self::serial_connection_supported(next, device, connection) {
                self.serial_router
                    .connect(device, SerialConnection::Disconnected);
            }
        }

        self.asr33.tx_started = None;
        self.asr33.answerback.clear();
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
    }

    fn serial_device_name(device: SerialDevice) -> &'static str {
        match device {
            SerialDevice::InternalAsr33 => "ASR-33",
            SerialDevice::TextTerminal => "Text Terminal",
            SerialDevice::ExternalTcp => "External TCP",
            SerialDevice::ExternalCom => "External COM",
        }
    }

    fn serial_connection_supported(
        hardware: S100HardwareConfig,
        device: SerialDevice,
        connection: SerialConnection,
    ) -> bool {
        if connection == SerialConnection::Disconnected {
            return true;
        }
        let Some((_, card)) = hardware.active_serial_card_slot() else {
            return false;
        };
        match (card, connection) {
            (S100InstalledCardConfig::Mits88Sio(config), SerialConnection::Port0) => {
                device.supports_sio_interface(config.interface)
            }
            (S100InstalledCardConfig::Mits88Sio(_), SerialConnection::Port1) => false,
            (
                S100InstalledCardConfig::Mits88TwoSio { straps, .. },
                SerialConnection::Port0,
            ) => device.supports_two_sio_interface(straps.port0_interface),
            (
                S100InstalledCardConfig::Mits88TwoSio { straps, .. },
                SerialConnection::Port1,
            ) => device.supports_two_sio_interface(straps.port1_interface),
            _ => false,
        }
    }

    fn serial_connection_label(
        hardware: S100HardwareConfig,
        connection: SerialConnection,
    ) -> String {
        if connection == SerialConnection::Disconnected {
            return "Disconnected".into();
        }
        let Some((slot, card)) = hardware.active_serial_card_slot() else {
            return "Unavailable — no serial card installed".into();
        };
        match (card, connection) {
            (S100InstalledCardConfig::Mits88Sio(config), SerialConnection::Port0) => format!(
                "Slot {slot} · 88-SIO {} [{:02X}h/{:02X}h]",
                config.interface.label(),
                config.address.status(),
                config.address.data(),
            ),
            (S100InstalledCardConfig::Mits88Sio(_), SerialConnection::Port1) => {
                "Unavailable — 88-SIO has one serial channel".into()
            }
            (
                S100InstalledCardConfig::Mits88TwoSio { straps, .. },
                SerialConnection::Port0,
            ) => format!(
                "Slot {slot} · 88-2SIO Port 0 [{:02X}h/{:02X}h · {}]",
                straps.address.port0_status(),
                straps.address.port0_data(),
                straps.port0_interface.label(),
            ),
            (
                S100InstalledCardConfig::Mits88TwoSio { straps, .. },
                SerialConnection::Port1,
            ) => format!(
                "Slot {slot} · 88-2SIO Port 1 [{:02X}h/{:02X}h · {}]",
                straps.address.port1_status(),
                straps.address.port1_data(),
                straps.port1_interface.label(),
            ),
            _ => "Unavailable".into(),
        }
    }

    fn serial_connection(&self, device: SerialDevice) -> SerialConnection {
        self.serial_router.connection(device)
    }

    fn set_serial_connection(&mut self, device: SerialDevice, connection: SerialConnection) {
        let hardware = self.config.machine.s100_hardware;
        if !Self::serial_connection_supported(hardware, device, connection) {
            let reason = match hardware.active_serial_card_slot().map(|(_, card)| card) {
                Some(S100InstalledCardConfig::Mits88Sio(_)) => device.sio_requirement_label(),
                Some(S100InstalledCardConfig::Mits88TwoSio { .. }) => {
                    device.two_sio_requirement_label()
                }
                _ => "install a serial card in Configuration → S-100 Chassis / Cards first",
            };
            self.status = format!(
                "{} not connected: {}; no hidden level converter or phantom UART is inserted",
                Self::serial_device_name(device),
                reason,
            );
            return;
        }
        if self.serial_router.connection(device) == connection { return; }

        let old_asr_connection = self.asr_connection();
        let moving_asr = device == SerialDevice::InternalAsr33;
        let displacing_asr = connection.is_connected()
            && self.serial_router.device_on(connection) == Some(SerialDevice::InternalAsr33);
        if moving_asr || displacing_asr {
            let _ = self.serial_set_receive_break_at(old_asr_connection, false);
        }

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
        let connection_name = Self::serial_connection_label(hardware, connection);
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
        Self::backend_serial_port(connection).map(|port| self.machine.serial_rx_empty(port)).unwrap_or(true)
    }
    fn serial_rx_len_at(&mut self, connection: SerialConnection) -> usize {
        Self::backend_serial_port(connection).map(|port| self.machine.serial_rx_len(port)).unwrap_or(0)
    }
    fn serial_receive_at(&mut self, connection: SerialConnection, byte: u8) {
        if let Some(port) = Self::backend_serial_port(connection) { self.machine.serial_receive(port, byte); }
    }
    fn serial_tx_busy_at(&mut self, connection: SerialConnection) -> bool {
        Self::backend_serial_port(connection).map(|port| self.machine.serial_tx_busy(port)).unwrap_or(false)
    }
    fn serial_tx_front_at(&mut self, connection: SerialConnection) -> Option<u8> {
        Self::backend_serial_port(connection).and_then(|port| self.machine.serial_tx_front(port))
    }
    fn serial_tx_complete_at(&mut self, connection: SerialConnection) -> Option<u8> {
        Self::backend_serial_port(connection).and_then(|port| self.machine.serial_tx_complete(port))
    }

    fn asr_connection(&self) -> SerialConnection { self.serial_connection(SerialDevice::InternalAsr33) }
    fn asr_serial_rx_empty(&mut self) -> bool { let c = self.asr_connection(); self.serial_rx_empty_at(c) }
    fn asr_serial_rx_len(&mut self) -> usize { let c = self.asr_connection(); self.serial_rx_len_at(c) }
    fn asr_serial_receive(&mut self, byte: u8) { let c = self.asr_connection(); self.serial_receive_at(c, byte); }
    fn asr_serial_tx_busy(&mut self) -> bool { let c = self.asr_connection(); self.serial_tx_busy_at(c) }
    fn asr_serial_tx_front(&mut self) -> Option<u8> { let c = self.asr_connection(); self.serial_tx_front_at(c) }
    fn asr_serial_tx_complete(&mut self) -> Option<u8> { let c = self.asr_connection(); self.serial_tx_complete_at(c) }

    fn terminal_connection(&self) -> SerialConnection { self.serial_connection(SerialDevice::TextTerminal) }
    fn terminal_serial_rx_len(&mut self) -> usize { let c = self.terminal_connection(); self.serial_rx_len_at(c) }
    fn terminal_serial_receive(&mut self, byte: u8) { let c = self.terminal_connection(); self.serial_receive_at(c, byte); }
    fn terminal_serial_tx_busy(&mut self) -> bool { let c = self.terminal_connection(); self.serial_tx_busy_at(c) }

    fn service_disconnected_serial_ports(&mut self) {
        if self.config.machine.serial_board().is_none() {
            return;
        }
        if self.serial_router.device_on(SerialConnection::Port0).is_none()
            && self.machine.serial_tx_busy(BackendSerialPort::Port0)
        {
            self.machine.serial_tx_complete(BackendSerialPort::Port0);
        }
        if self.config.machine.serial_board() == Some(SerialBoard::TwoSio88)
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

use super::super::*;
use crate::app::asr33_state::{TapeBitOrder, TapeTransportSpeed};
use crate::config::{
    ComDataBits, ComFlowControl, ComParity, ComStopBits, CpuModel, ExternalComConfig,
    ExternalSerialCharacterMode, ExternalSerialConfig, ExternalSerialSpeed, SioHardwareConfig,
    TcpListenScope, TerminalDuplex, TwoSioAddressBlock, TwoSioBaudTap, TwoSioInterruptTarget,
    TwoSioSignalInterface,
};
use crate::peripherals::asr33::Mode as TtyMode;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const CONFIG_VERSION: u32 = 4;
const DEFAULT_LED_BRIGHTNESS: f32 = 1.0;
const DEFAULT_LED_AURA: f32 = 1.0;
const SAVE_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SavedSettings {
    pub(super) config: AppConfig,
    pub(super) engine: EmulationEngine,
    pub(super) asr_connection: SerialConnection,
    pub(super) terminal_connection: SerialConnection,
    pub(super) external_tcp_connection: SerialConnection,
    pub(super) external_com_connection: SerialConnection,
    pub(super) external_serial: ExternalSerialConfig,
    pub(super) external_com: ExternalComConfig,
    pub(super) asr_duplex: TerminalDuplex,
    pub(super) terminal_duplex: TerminalDuplex,
    pub(super) terminal_uppercase: bool,
    pub(super) tty_mode: TtyMode,
    pub(super) reader_speed: TapeTransportSpeed,
    pub(super) punch_speed: TapeTransportSpeed,
    pub(super) tape_bit_order: TapeBitOrder,
    pub(super) led_brightness: f32,
    pub(super) led_aura: f32,
    pub(super) muted: bool,
}

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            config: AppConfig::default(),
            engine: EmulationEngine::RustFast8080,
            asr_connection: SerialConnection::Port0,
            terminal_connection: SerialConnection::Disconnected,
            external_tcp_connection: SerialConnection::Disconnected,
            external_com_connection: SerialConnection::Disconnected,
            external_serial: ExternalSerialConfig::default(),
            external_com: ExternalComConfig::default(),
            asr_duplex: TerminalDuplex::default(),
            terminal_duplex: TerminalDuplex::default(),
            terminal_uppercase: true,
            tty_mode: TtyMode::Off,
            reader_speed: TapeTransportSpeed::Historical1x,
            punch_speed: TapeTransportSpeed::Historical1x,
            tape_bit_order: TapeBitOrder::Historical8To1,
            led_brightness: DEFAULT_LED_BRIGHTNESS,
            led_aura: DEFAULT_LED_AURA,
            muted: false,
        }
    }
}

#[derive(Clone, Debug)]
struct PersistenceRuntime {
    loaded: bool,
    last_saved: SavedSettings,
    last_save_failure: Option<Instant>,
    led_brightness: f32,
    led_aura: f32,
    led_controls_open: bool,
}

impl Default for PersistenceRuntime {
    fn default() -> Self {
        let saved = SavedSettings::default();
        Self {
            loaded: false,
            last_saved: saved.clone(),
            last_save_failure: None,
            led_brightness: saved.led_brightness,
            led_aura: saved.led_aura,
            led_controls_open: false,
        }
    }
}

static PERSISTENCE_RUNTIME: OnceLock<Mutex<PersistenceRuntime>> = OnceLock::new();

fn runtime() -> &'static Mutex<PersistenceRuntime> {
    PERSISTENCE_RUNTIME.get_or_init(|| Mutex::new(PersistenceRuntime::default()))
}

pub(super) fn led_visual_settings() -> (f32, f32) {
    let state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    (state.led_brightness, state.led_aura)
}

pub(super) fn led_visual_controls_state() -> (bool, f32, f32) {
    let state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    (
        state.led_controls_open,
        state.led_brightness,
        state.led_aura,
    )
}

pub(super) fn set_led_visual_controls_state(open: bool, brightness: f32, aura: f32) {
    let mut state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.led_controls_open = open;
    state.led_brightness = brightness.clamp(0.25, 3.0);
    state.led_aura = aura.clamp(0.0, 3.0);
}

impl SavedSettings {
    fn load_or_default() -> Self {
        match fs::read_to_string(config_path()) {
            Ok(text) => Self::from_text(&text),
            Err(_) => Self::default(),
        }
    }

    fn from_text(text: &str) -> Self {
        let mut saved = Self::default();
        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim();
            let value = value.trim();
            match key {
                "machine.cpu_model" => {
                    if value == "intel8080" { saved.config.machine.cpu_model = CpuModel::Intel8080; }
                }
                "machine.ram_size" => if let Some(v) = parse_ram_size(value) { saved.config.machine.ram_size = v; },
                "machine.ram_init" => if let Some(v) = parse_ram_init(value) { saved.config.machine.ram_init = v; },
                "machine.ram_board_profile" => if let Some(v) = parse_ram_board_profile(value) { saved.config.machine.ram_board_profile = v; },
                "machine.serial_board" => if let Some(v) = parse_serial_board(value) { saved.config.machine.serial_board = v; },
                "machine.sio_hardware" => if let Some(v) = SioHardwareConfig::from_persistence_key(value) { saved.config.machine.sio_hardware = v; },
                "machine.two_sio_base" => if let Some(v) = parse_two_sio_address(value) { saved.config.machine.two_sio_straps.address = v; },
                "machine.two_sio_port0_baud" => if let Some(v) = parse_two_sio_baud(value) { saved.config.machine.two_sio_straps.port0_baud = v; },
                "machine.two_sio_port1_baud" => if let Some(v) = parse_two_sio_baud(value) { saved.config.machine.two_sio_straps.port1_baud = v; },
                "machine.two_sio_port0_interface" => if let Some(v) = TwoSioSignalInterface::from_persistence_key(value) { saved.config.machine.two_sio_straps.port0_interface = v; },
                "machine.two_sio_port1_interface" => if let Some(v) = TwoSioSignalInterface::from_persistence_key(value) { saved.config.machine.two_sio_straps.port1_interface = v; },
                "machine.two_sio_port0_irq" => if let Some(v) = TwoSioInterruptTarget::from_persistence_key(value) { saved.config.machine.two_sio_interrupt_wiring.port0 = v; },
                "machine.two_sio_port1_irq" => if let Some(v) = TwoSioInterruptTarget::from_persistence_key(value) { saved.config.machine.two_sio_interrupt_wiring.port1 = v; },
                "peripherals.asr33_speed" => if let Some(v) = parse_asr_speed(value) { saved.config.peripherals.asr33_speed = v; },
                "peripherals.terminal_speed" => if let Some(v) = parse_terminal_speed(value) { saved.config.peripherals.terminal_speed = v; },
                "compatibility.basic32_64k_probe_workaround" => if let Ok(v) = value.parse() { saved.config.compatibility.basic32_64k_probe_workaround = v; },
                "compatibility.historical_undefined_run_latch_power_on" => if let Ok(v) = value.parse() { saved.config.compatibility.historical_undefined_run_latch_power_on = v; },
                "preferences.auto_open_basic_console" => if let Ok(v) = value.parse() { saved.config.preferences.auto_open_basic_console = v; },
                "preferences.emulation_speed" => if let Some(v) = parse_emulation_speed(value) { saved.config.preferences.emulation_speed = v; },
                "engine" => if let Some(v) = parse_engine(value) { saved.engine = v; },
                "wiring.asr33" => if let Some(v) = parse_connection(value) { saved.asr_connection = v; },
                "wiring.terminal" => if let Some(v) = parse_connection(value) { saved.terminal_connection = v; },
                "wiring.external_tcp" => if let Some(v) = parse_connection(value) { saved.external_tcp_connection = v; },
                "wiring.external_com" => if let Some(v) = parse_connection(value) { saved.external_com_connection = v; },
                "external_tcp.enabled" => if let Ok(v) = value.parse() { saved.external_serial.enabled = v; },
                "external_tcp.listen_scope" => if let Some(v) = parse_listen_scope(value) { saved.external_serial.listen_scope = v; },
                "external_tcp.port" => if let Ok(v) = value.parse() { saved.external_serial.tcp_port = v; },
                "external_tcp.speed" => if let Some(v) = parse_external_speed(value) { saved.external_serial.speed = v; },
                "external_tcp.character_mode" => if let Some(v) = parse_character_mode(value) { saved.external_serial.character_mode = v; },
                "external_tcp.duplex" => if let Some(v) = parse_duplex(value) { saved.external_serial.duplex = v; },
                "external_tcp.allow_multiple_clients" => if let Ok(v) = value.parse() { saved.external_serial.allow_multiple_clients = v; },
                "external_com.enabled" => if let Ok(v) = value.parse() { saved.external_com.enabled = v; },
                "external_com.port_name_hex" => if let Some(v) = decode_hex_string(value) { saved.external_com.port_name = v; },
                "external_com.baud_rate" => if let Ok(v) = value.parse::<u32>() { if v > 0 { saved.external_com.baud_rate = v; } },
                "external_com.data_bits" => if let Some(v) = parse_data_bits(value) { saved.external_com.data_bits = v; },
                "external_com.parity" => if let Some(v) = parse_parity(value) { saved.external_com.parity = v; },
                "external_com.stop_bits" => if let Some(v) = parse_stop_bits(value) { saved.external_com.stop_bits = v; },
                "external_com.flow_control" => if let Some(v) = parse_flow_control(value) { saved.external_com.flow_control = v; },
                "external_com.character_mode" => if let Some(v) = parse_character_mode(value) { saved.external_com.character_mode = v; },
                "external_com.duplex" => if let Some(v) = parse_duplex(value) { saved.external_com.duplex = v; },
                "asr33.duplex" => if let Some(v) = parse_duplex(value) { saved.asr_duplex = v; },
                "asr33.mode" => if let Some(v) = parse_tty_mode(value) { saved.tty_mode = v; },
                "asr33.reader_speed" => if let Some(v) = parse_tape_transport_speed(value) { saved.reader_speed = v; },
                "asr33.punch_speed" => if let Some(v) = parse_tape_transport_speed(value) { saved.punch_speed = v; },
                "asr33.tape_visual_order" => if let Some(v) = parse_tape_bit_order(value) { saved.tape_bit_order = v; },
                "terminal.duplex" => if let Some(v) = parse_duplex(value) { saved.terminal_duplex = v; },
                "terminal.uppercase" => if let Ok(v) = value.parse() { saved.terminal_uppercase = v; },
                "led.brightness" => if let Ok(v) = value.parse::<f32>() { if v.is_finite() { saved.led_brightness = v.clamp(0.25, 3.0); } },
                "led.aura" => if let Ok(v) = value.parse::<f32>() { if v.is_finite() { saved.led_aura = v.clamp(0.0, 3.0); } },
                "audio.muted" => if let Ok(v) = value.parse() { saved.muted = v; },
                _ => {}
            }
        }
        saved
    }

    fn to_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# RusTair persistent configuration");
        let _ = writeln!(out, "version={CONFIG_VERSION}");
        let _ = writeln!(out, "machine.cpu_model=intel8080");
        let _ = writeln!(out, "machine.ram_size={}", ram_size_key(self.config.machine.ram_size));
        let _ = writeln!(out, "machine.ram_init={}", ram_init_key(self.config.machine.ram_init));
        let _ = writeln!(out, "machine.ram_board_profile={}", ram_board_profile_key(self.config.machine.ram_board_profile));
        let _ = writeln!(out, "machine.serial_board={}", serial_board_key(self.config.machine.serial_board));
        let _ = writeln!(out, "machine.sio_hardware={}", self.config.machine.sio_hardware.persistence_key());
        let _ = writeln!(out, "machine.two_sio_base={:02X}", self.config.machine.two_sio_straps.address.base());
        let _ = writeln!(out, "machine.two_sio_port0_baud={}", two_sio_baud_key(self.config.machine.two_sio_straps.port0_baud));
        let _ = writeln!(out, "machine.two_sio_port1_baud={}", two_sio_baud_key(self.config.machine.two_sio_straps.port1_baud));
        let _ = writeln!(out, "machine.two_sio_port0_interface={}", self.config.machine.two_sio_straps.port0_interface.persistence_key());
        let _ = writeln!(out, "machine.two_sio_port1_interface={}", self.config.machine.two_sio_straps.port1_interface.persistence_key());
        let _ = writeln!(out, "machine.two_sio_port0_irq={}", self.config.machine.two_sio_interrupt_wiring.port0.persistence_key());
        let _ = writeln!(out, "machine.two_sio_port1_irq={}", self.config.machine.two_sio_interrupt_wiring.port1.persistence_key());
        let _ = writeln!(out, "peripherals.asr33_speed={}", asr_speed_key(self.config.peripherals.asr33_speed));
        let _ = writeln!(out, "peripherals.terminal_speed={}", terminal_speed_key(self.config.peripherals.terminal_speed));
        let _ = writeln!(out, "compatibility.basic32_64k_probe_workaround={}", self.config.compatibility.basic32_64k_probe_workaround);
        let _ = writeln!(out, "compatibility.historical_undefined_run_latch_power_on={}", self.config.compatibility.historical_undefined_run_latch_power_on);
        let _ = writeln!(out, "preferences.auto_open_basic_console={}", self.config.preferences.auto_open_basic_console);
        let _ = writeln!(out, "preferences.emulation_speed={}", emulation_speed_key(self.config.preferences.emulation_speed));
        let _ = writeln!(out, "engine={}", engine_key(self.engine));
        let _ = writeln!(out, "wiring.asr33={}", connection_key(self.asr_connection));
        let _ = writeln!(out, "wiring.terminal={}", connection_key(self.terminal_connection));
        let _ = writeln!(out, "wiring.external_tcp={}", connection_key(self.external_tcp_connection));
        let _ = writeln!(out, "wiring.external_com={}", connection_key(self.external_com_connection));
        let _ = writeln!(out, "external_tcp.enabled={}", self.external_serial.enabled);
        let _ = writeln!(out, "external_tcp.listen_scope={}", listen_scope_key(self.external_serial.listen_scope));
        let _ = writeln!(out, "external_tcp.port={}", self.external_serial.tcp_port);
        let _ = writeln!(out, "external_tcp.speed={}", external_speed_key(self.external_serial.speed));
        let _ = writeln!(out, "external_tcp.character_mode={}", character_mode_key(self.external_serial.character_mode));
        let _ = writeln!(out, "external_tcp.duplex={}", duplex_key(self.external_serial.duplex));
        let _ = writeln!(out, "external_tcp.allow_multiple_clients={}", self.external_serial.allow_multiple_clients);
        let _ = writeln!(out, "external_com.enabled={}", self.external_com.enabled);
        let _ = writeln!(out, "external_com.port_name_hex={}", encode_hex_string(&self.external_com.port_name));
        let _ = writeln!(out, "external_com.baud_rate={}", self.external_com.baud_rate);
        let _ = writeln!(out, "external_com.data_bits={}", data_bits_key(self.external_com.data_bits));
        let _ = writeln!(out, "external_com.parity={}", parity_key(self.external_com.parity));
        let _ = writeln!(out, "external_com.stop_bits={}", stop_bits_key(self.external_com.stop_bits));
        let _ = writeln!(out, "external_com.flow_control={}", flow_control_key(self.external_com.flow_control));
        let _ = writeln!(out, "external_com.character_mode={}", character_mode_key(self.external_com.character_mode));
        let _ = writeln!(out, "external_com.duplex={}", duplex_key(self.external_com.duplex));
        let _ = writeln!(out, "asr33.duplex={}", duplex_key(self.asr_duplex));
        let _ = writeln!(out, "asr33.mode={}", tty_mode_key(self.tty_mode));
        let _ = writeln!(out, "asr33.reader_speed={}", tape_transport_speed_key(self.reader_speed));
        let _ = writeln!(out, "asr33.punch_speed={}", tape_transport_speed_key(self.punch_speed));
        let _ = writeln!(out, "asr33.tape_visual_order={}", tape_bit_order_key(self.tape_bit_order));
        let _ = writeln!(out, "terminal.duplex={}", duplex_key(self.terminal_duplex));
        let _ = writeln!(out, "terminal.uppercase={}", self.terminal_uppercase);
        let _ = writeln!(out, "led.brightness={:.3}", self.led_brightness);
        let _ = writeln!(out, "led.aura={:.3}", self.led_aura);
        let _ = writeln!(out, "audio.muted={}", self.muted);
        out
    }

    fn save(&self) -> Result<(), String> {
        self.save_to_path(&config_path())
    }

    fn save_to_path(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or_else(|| "configuration path has no parent".to_owned())?;
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;

        let file_name = path
            .file_name()
            .ok_or_else(|| "configuration path has no file name".to_owned())?
            .to_string_lossy();
        let temporary = parent.join(format!(".{file_name}.tmp"));
        let text = self.to_text();

        let result = (|| -> Result<(), String> {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&temporary)
                .map_err(|e| format!("could not open temporary configuration {}: {e}", temporary.display()))?;
            file.write_all(text.as_bytes())
                .map_err(|e| format!("could not write temporary configuration {}: {e}", temporary.display()))?;
            file.sync_all()
                .map_err(|e| format!("could not flush temporary configuration {}: {e}", temporary.display()))?;
            drop(file);
            fs::rename(&temporary, path).map_err(|e| {
                format!(
                    "could not atomically replace configuration {} from {}: {e}",
                    path.display(),
                    temporary.display()
                )
            })?;
            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

impl RusTairApp {
    pub(super) fn ensure_persistent_configuration_loaded(&mut self) {
        {
            let state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.loaded {
                return;
            }
        }

        let saved = SavedSettings::load_or_default();
        self.apply_persisted_settings(&saved);
        let normalized = self.capture_persisted_settings_with_leds(
            saved.led_brightness,
            saved.led_aura,
        );

        let mut state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.loaded = true;
        state.last_saved = normalized;
        state.last_save_failure = None;
        state.led_brightness = saved.led_brightness;
        state.led_aura = saved.led_aura;
        state.led_controls_open = false;
    }

    fn apply_persisted_settings(&mut self, saved: &SavedSettings) {
        self.config = saved.config;
        let engine = if saved.engine.is_available() { saved.engine } else { EmulationEngine::RustFast8080 };
        if self.machine.engine() != engine {
            let _ = self.machine.replace_engine(engine);
        }
        self.machine.configure_memory(self.config.machine.ram_size, self.config.machine.ram_init);
        self.machine.configure_memory_board_profile(self.config.machine.ram_board_profile);
        self.machine.configure_sio_hardware(self.config.machine.sio_hardware);
        self.machine.configure_serial_board(self.config.machine.serial_board);
        self.machine.configure_two_sio_straps(self.config.machine.two_sio_straps);
        self.machine.configure_two_sio_interrupt_wiring(self.config.machine.two_sio_interrupt_wiring);

        self.serial_router.reset_for_board(self.config.machine.serial_board);
        let machine = self.config.machine;
        for (device, connection) in [
            (SerialDevice::InternalAsr33, saved.asr_connection),
            (SerialDevice::TextTerminal, saved.terminal_connection),
            (SerialDevice::ExternalTcp, saved.external_tcp_connection),
            (SerialDevice::ExternalCom, saved.external_com_connection),
        ] {
            self.serial_router.connect(device, valid_connection(machine, device, connection));
        }

        self.external_serial.config = saved.external_serial;
        self.external_serial.server.restart_on_next_poll();
        self.external_serial.reset_line_timing();
        self.external_com.config = saved.external_com.clone();
        self.external_com.port.restart_on_next_poll();
        self.external_com.reset_line_timing();

        self.asr33.duplex = saved.asr_duplex;
        self.asr33.reader_speed = saved.reader_speed;
        self.asr33.punch_speed = saved.punch_speed;
        self.asr33.tape_bit_order = saved.tape_bit_order;
        self.terminal.duplex = saved.terminal_duplex;
        self.terminal.uppercase = saved.terminal_uppercase;
        self.terminal.speed = self.config.peripherals.terminal_speed;
        self.tty.set_mode(saved.tty_mode);
        self.audio.set_muted(saved.muted);
        self.last_tick = Instant::now();
        self.status = format!(
            "Ready — {} — {} RAM — {} — saved configuration loaded",
            self.machine.engine().label(),
            self.config.machine.ram_size.label(),
            self.config.machine.serial_board.label(),
        );
    }

    fn capture_persisted_settings_with_leds(
        &self,
        led_brightness: f32,
        led_aura: f32,
    ) -> SavedSettings {
        SavedSettings {
            config: self.config,
            engine: self.machine.engine(),
            asr_connection: self.serial_router.connection(SerialDevice::InternalAsr33),
            terminal_connection: self.serial_router.connection(SerialDevice::TextTerminal),
            external_tcp_connection: self.serial_router.connection(SerialDevice::ExternalTcp),
            external_com_connection: self.serial_router.connection(SerialDevice::ExternalCom),
            external_serial: self.external_serial.config,
            external_com: self.external_com.config.clone(),
            asr_duplex: self.asr33.duplex,
            terminal_duplex: self.terminal.duplex,
            terminal_uppercase: self.terminal.uppercase,
            tty_mode: self.tty.mode,
            reader_speed: self.asr33.reader_speed,
            punch_speed: self.asr33.punch_speed,
            tape_bit_order: self.asr33.tape_bit_order,
            led_brightness: led_brightness.clamp(0.25, 3.0),
            led_aura: led_aura.clamp(0.0, 3.0),
            muted: self.audio.muted(),
        }
    }

    pub(super) fn open_led_visual_controls(&mut self) {
        let mut state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.led_controls_open = true;
    }

    pub(super) fn persist_configuration_if_changed(&mut self) {
        let (brightness, aura, last_saved, last_save_failure) = {
            let state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                state.led_brightness,
                state.led_aura,
                state.last_saved.clone(),
                state.last_save_failure,
            )
        };
        let current = self.capture_persisted_settings_with_leds(brightness, aura);
        if current == last_saved {
            return;
        }
        if last_save_failure.is_some_and(|at| at.elapsed() < SAVE_RETRY_DELAY) {
            return;
        }

        match current.save() {
            Ok(()) => {
                let mut state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.last_saved = current;
                state.last_save_failure = None;
            }
            Err(error) => {
                eprintln!("RusTair configuration save failed: {error}");
                self.status = format!("Configuration save failed: {error} — will retry");
                let mut state = runtime().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.last_save_failure = Some(Instant::now());
            }
        }
    }
}

fn config_path() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("RusTair").join("config.ini");
    }
    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("RusTair")
                .join("config.ini");
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("rustair").join("config.ini");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("rustair").join("config.ini");
    }
    PathBuf::from("rustair-config.ini")
}

fn valid_connection(
    machine: crate::config::MachineConfig,
    device: SerialDevice,
    connection: SerialConnection,
) -> SerialConnection {
    let compatible = match (machine.serial_board, connection) {
        (_, SerialConnection::Disconnected) => true,
        (SerialBoard::Sio88, SerialConnection::Port1) => false,
        (SerialBoard::Sio88, SerialConnection::Port0) => {
            device.supports_sio_interface(machine.sio_hardware.interface)
        }
        (SerialBoard::TwoSio88, SerialConnection::Port0) => {
            device.supports_two_sio_interface(machine.two_sio_straps.port0_interface)
        }
        (SerialBoard::TwoSio88, SerialConnection::Port1) => {
            device.supports_two_sio_interface(machine.two_sio_straps.port1_interface)
        }
    };
    if compatible { connection } else { SerialConnection::Disconnected }
}

fn encode_hex_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        let _ = write!(encoded, "{byte:02X}");
    }
    encoded
}

fn decode_hex_string(value: &str) -> Option<String> {
    if value.len() % 2 != 0 { return None; }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
    }
    String::from_utf8(bytes).ok()
}

fn ram_size_key(v: RamSize) -> &'static str { match v { RamSize::Bytes256 => "256b", RamSize::K1 => "1k", RamSize::K4 => "4k", RamSize::K8 => "8k", RamSize::K16 => "16k", RamSize::K32 => "32k", RamSize::K48 => "48k", RamSize::K64 => "64k" } }
fn parse_ram_size(v: &str) -> Option<RamSize> { Some(match v { "256b" => RamSize::Bytes256, "1k" => RamSize::K1, "4k" => RamSize::K4, "8k" => RamSize::K8, "16k" => RamSize::K16, "32k" => RamSize::K32, "48k" => RamSize::K48, "64k" => RamSize::K64, _ => return None }) }
fn ram_init_key(v: RamInit) -> &'static str { match v { RamInit::Random => "random", RamInit::Zeroed => "zeroed" } }
fn parse_ram_init(v: &str) -> Option<RamInit> { Some(match v { "random" => RamInit::Random, "zeroed" => RamInit::Zeroed, _ => return None }) }
fn ram_board_profile_key(v: RamBoardProfile) -> &'static str { match v { RamBoardProfile::FastNoWait => "fast-no-wait", RamBoardProfile::Mits1KStatic1975 => "mits-1k-static-1975" } }
fn parse_ram_board_profile(v: &str) -> Option<RamBoardProfile> { Some(match v { "fast-no-wait" => RamBoardProfile::FastNoWait, "mits-1k-static-1975" => RamBoardProfile::Mits1KStatic1975, _ => return None }) }
fn serial_board_key(v: SerialBoard) -> &'static str { match v { SerialBoard::Sio88 => "88-sio", SerialBoard::TwoSio88 => "88-2sio" } }
fn parse_serial_board(v: &str) -> Option<SerialBoard> { Some(match v { "88-sio" => SerialBoard::Sio88, "88-2sio" => SerialBoard::TwoSio88, _ => return None }) }
fn parse_two_sio_address(v: &str) -> Option<TwoSioAddressBlock> { TwoSioAddressBlock::try_new(u8::from_str_radix(v, 16).ok()?) }
fn two_sio_baud_key(v: TwoSioBaudTap) -> &'static str { v.label() }
fn parse_two_sio_baud(v: &str) -> Option<TwoSioBaudTap> { Some(match v { "110" => TwoSioBaudTap::Baud110, "150" => TwoSioBaudTap::Baud150, "300" => TwoSioBaudTap::Baud300, "1200" => TwoSioBaudTap::Baud1200, "1800" => TwoSioBaudTap::Baud1800, "2400" => TwoSioBaudTap::Baud2400, "4800" => TwoSioBaudTap::Baud4800, "9600" => TwoSioBaudTap::Baud9600, _ => return None }) }
fn asr_speed_key(v: Asr33Speed) -> &'static str { match v { Asr33Speed::Authentic110 => "110", Asr33Speed::Accelerated2x => "2x", Asr33Speed::Accelerated4x => "4x", Asr33Speed::Instant => "instant" } }
fn parse_asr_speed(v: &str) -> Option<Asr33Speed> { Some(match v { "110" => Asr33Speed::Authentic110, "2x" => Asr33Speed::Accelerated2x, "4x" => Asr33Speed::Accelerated4x, "instant" => Asr33Speed::Instant, _ => return None }) }
fn terminal_speed_key(v: TerminalSpeed) -> &'static str { match v { TerminalSpeed::Instant => "instant", TerminalSpeed::Baud300 => "300", TerminalSpeed::Baud1200 => "1200", TerminalSpeed::Baud2400 => "2400", TerminalSpeed::Baud9600 => "9600" } }
fn parse_terminal_speed(v: &str) -> Option<TerminalSpeed> { Some(match v { "instant" => TerminalSpeed::Instant, "300" => TerminalSpeed::Baud300, "1200" => TerminalSpeed::Baud1200, "2400" => TerminalSpeed::Baud2400, "9600" => TerminalSpeed::Baud9600, _ => return None }) }
fn emulation_speed_key(v: EmulationSpeed) -> &'static str { match v { EmulationSpeed::Authentic => "authentic", EmulationSpeed::X2 => "2x", EmulationSpeed::X5 => "5x", EmulationSpeed::X10 => "10x", EmulationSpeed::Unlimited => "unlimited" } }
fn parse_emulation_speed(v: &str) -> Option<EmulationSpeed> { Some(match v { "authentic" => EmulationSpeed::Authentic, "2x" => EmulationSpeed::X2, "5x" => EmulationSpeed::X5, "10x" => EmulationSpeed::X10, "unlimited" => EmulationSpeed::Unlimited, _ => return None }) }
fn engine_key(v: EmulationEngine) -> &'static str { match v { EmulationEngine::RustFast8080 => "rust-fast-8080", EmulationEngine::RustCycleAccurate8080 => "rust-cycle-8080" } }
fn parse_engine(v: &str) -> Option<EmulationEngine> { Some(match v { "rust-fast-8080" => EmulationEngine::RustFast8080, "rust-cycle-8080" => EmulationEngine::RustCycleAccurate8080, _ => return None }) }
fn connection_key(v: SerialConnection) -> &'static str { match v { SerialConnection::Disconnected => "disconnected", SerialConnection::Port0 => "port0", SerialConnection::Port1 => "port1" } }
fn parse_connection(v: &str) -> Option<SerialConnection> { Some(match v { "disconnected" => SerialConnection::Disconnected, "port0" => SerialConnection::Port0, "port1" => SerialConnection::Port1, _ => return None }) }
fn listen_scope_key(v: TcpListenScope) -> &'static str { match v { TcpListenScope::Loopback => "loopback", TcpListenScope::AllInterfaces => "all" } }
fn parse_listen_scope(v: &str) -> Option<TcpListenScope> { Some(match v { "loopback" => TcpListenScope::Loopback, "all" => TcpListenScope::AllInterfaces, _ => return None }) }
fn external_speed_key(v: ExternalSerialSpeed) -> &'static str { match v { ExternalSerialSpeed::Instant => "instant", ExternalSerialSpeed::Baud110 => "110", ExternalSerialSpeed::Baud300 => "300", ExternalSerialSpeed::Baud1200 => "1200", ExternalSerialSpeed::Baud2400 => "2400", ExternalSerialSpeed::Baud9600 => "9600" } }
fn parse_external_speed(v: &str) -> Option<ExternalSerialSpeed> { Some(match v { "instant" => ExternalSerialSpeed::Instant, "110" => ExternalSerialSpeed::Baud110, "300" => ExternalSerialSpeed::Baud300, "1200" => ExternalSerialSpeed::Baud1200, "2400" => ExternalSerialSpeed::Baud2400, "9600" => ExternalSerialSpeed::Baud9600, _ => return None }) }
fn character_mode_key(v: ExternalSerialCharacterMode) -> &'static str { match v { ExternalSerialCharacterMode::Asr33Uppercase => "asr33", ExternalSerialCharacterMode::SevenBitAscii => "7bit", ExternalSerialCharacterMode::Raw8Bit => "raw8" } }
fn parse_character_mode(v: &str) -> Option<ExternalSerialCharacterMode> { Some(match v { "asr33" => ExternalSerialCharacterMode::Asr33Uppercase, "7bit" => ExternalSerialCharacterMode::SevenBitAscii, "raw8" => ExternalSerialCharacterMode::Raw8Bit, _ => return None }) }
fn duplex_key(v: TerminalDuplex) -> &'static str { match v { TerminalDuplex::FullDuplexRemoteEcho => "full", TerminalDuplex::HalfDuplexLocalEcho => "half" } }
fn parse_duplex(v: &str) -> Option<TerminalDuplex> { Some(match v { "full" => TerminalDuplex::FullDuplexRemoteEcho, "half" => TerminalDuplex::HalfDuplexLocalEcho, _ => return None }) }
fn data_bits_key(v: ComDataBits) -> &'static str { match v { ComDataBits::Five => "5", ComDataBits::Six => "6", ComDataBits::Seven => "7", ComDataBits::Eight => "8" } }
fn parse_data_bits(v: &str) -> Option<ComDataBits> { Some(match v { "5" => ComDataBits::Five, "6" => ComDataBits::Six, "7" => ComDataBits::Seven, "8" => ComDataBits::Eight, _ => return None }) }
fn parity_key(v: ComParity) -> &'static str { match v { ComParity::None => "none", ComParity::Odd => "odd", ComParity::Even => "even" } }
fn parse_parity(v: &str) -> Option<ComParity> { Some(match v { "none" => ComParity::None, "odd" => ComParity::Odd, "even" => ComParity::Even, _ => return None }) }
fn stop_bits_key(v: ComStopBits) -> &'static str { match v { ComStopBits::One => "1", ComStopBits::Two => "2" } }
fn parse_stop_bits(v: &str) -> Option<ComStopBits> { Some(match v { "1" => ComStopBits::One, "2" => ComStopBits::Two, _ => return None }) }
fn flow_control_key(v: ComFlowControl) -> &'static str { match v { ComFlowControl::None => "none", ComFlowControl::Software => "software", ComFlowControl::Hardware => "hardware" } }
fn parse_flow_control(v: &str) -> Option<ComFlowControl> { Some(match v { "none" => ComFlowControl::None, "software" => ComFlowControl::Software, "hardware" => ComFlowControl::Hardware, _ => return None }) }
fn tty_mode_key(v: TtyMode) -> &'static str { match v { TtyMode::Off => "off", TtyMode::Line => "line", TtyMode::Local => "local" } }
fn parse_tty_mode(v: &str) -> Option<TtyMode> { Some(match v { "off" => TtyMode::Off, "line" => TtyMode::Line, "local" => TtyMode::Local, _ => return None }) }
fn tape_transport_speed_key(v: TapeTransportSpeed) -> &'static str { match v { TapeTransportSpeed::Historical1x => "1x", TapeTransportSpeed::X5 => "5x", TapeTransportSpeed::X10 => "10x", TapeTransportSpeed::Unlimited => "unlimited" } }
fn parse_tape_transport_speed(v: &str) -> Option<TapeTransportSpeed> { Some(match v { "1x" => TapeTransportSpeed::Historical1x, "5x" => TapeTransportSpeed::X5, "10x" => TapeTransportSpeed::X10, "unlimited" => TapeTransportSpeed::Unlimited, _ => return None }) }
fn tape_bit_order_key(v: TapeBitOrder) -> &'static str { match v { TapeBitOrder::Historical8To1 => "8to1", TapeBitOrder::Reversed1To8 => "1to8" } }
fn parse_tape_bit_order(v: &str) -> Option<TapeBitOrder> { Some(match v { "8to1" => TapeBitOrder::Historical8To1, "1to8" => TapeBitOrder::Reversed1To8, _ => return None }) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_text_round_trip_preserves_all_tunable_groups() {
        let mut saved = SavedSettings::default();
        saved.engine = EmulationEngine::RustCycleAccurate8080;
        saved.config.machine.ram_size = RamSize::K48;
        saved.config.machine.ram_board_profile = RamBoardProfile::Mits1KStatic1975;
        saved.config.machine.sio_hardware = SioHardwareConfig::from_persistence_key("rev0,a-rs232,06,9600,7,even,1").unwrap();
        saved.config.machine.serial_board = SerialBoard::TwoSio88;
        saved.config.machine.two_sio_straps.address = TwoSioAddressBlock::try_new(0x44).unwrap();
        saved.config.machine.two_sio_straps.port0_baud = TwoSioBaudTap::Baud300;
        saved.config.machine.two_sio_straps.port1_baud = TwoSioBaudTap::Baud4800;
        saved.config.machine.two_sio_straps.port0_interface = TwoSioSignalInterface::Ttl;
        saved.config.machine.two_sio_straps.port1_interface = TwoSioSignalInterface::Rs232;
        saved.config.machine.two_sio_interrupt_wiring.port0 = TwoSioInterruptTarget::Vi3;
        saved.config.machine.two_sio_interrupt_wiring.port1 = TwoSioInterruptTarget::Disconnected;
        saved.config.preferences.emulation_speed = EmulationSpeed::X5;
        saved.external_tcp_connection = SerialConnection::Port0;
        saved.external_com_connection = SerialConnection::Port1;
        saved.asr_connection = SerialConnection::Disconnected;
        saved.terminal_connection = SerialConnection::Disconnected;
        saved.external_serial.enabled = true;
        saved.external_serial.tcp_port = 9876;
        saved.external_com.port_name = "COM=17\\virtual".into();
        saved.external_com.baud_rate = 115_200;
        saved.terminal_uppercase = false;
        saved.tty_mode = TtyMode::Line;
        saved.reader_speed = TapeTransportSpeed::X10;
        saved.punch_speed = TapeTransportSpeed::X5;
        saved.tape_bit_order = TapeBitOrder::Reversed1To8;
        saved.led_brightness = 1.37;
        saved.led_aura = 2.15;
        saved.muted = true;

        let decoded = SavedSettings::from_text(&saved.to_text());
        assert_eq!(decoded, saved);
    }

    #[test]
    fn old_or_invalid_sio_hardware_keeps_safe_default_as_one_atomic_card() {
        let old = SavedSettings::from_text("machine.serial_board=88-sio\n");
        assert_eq!(old.config.machine.sio_hardware, SioHardwareConfig::default());

        let invalid = SavedSettings::from_text("machine.sio_hardware=rev0,a-rs232,07,9600,7,even,1\n");
        assert_eq!(invalid.config.machine.sio_hardware, SioHardwareConfig::default());
    }

    #[test]
    fn persisted_invalid_two_sio_block_cannot_override_safe_default() {
        let decoded = SavedSettings::from_text("machine.two_sio_base=FC\nmachine.two_sio_port0_baud=300\n");
        assert_eq!(decoded.config.machine.two_sio_straps.address, TwoSioAddressBlock::DEFAULT);
        assert_eq!(decoded.config.machine.two_sio_straps.port0_baud, TwoSioBaudTap::Baud300);
    }

    #[test]
    fn old_or_invalid_two_sio_signal_wiring_keeps_safe_physical_defaults() {
        let old = SavedSettings::from_text("machine.serial_board=88-2sio\n");
        assert_eq!(old.config.machine.two_sio_straps.port0_interface, TwoSioSignalInterface::Tty20mA);
        assert_eq!(old.config.machine.two_sio_straps.port1_interface, TwoSioSignalInterface::Rs232);

        let invalid = SavedSettings::from_text("machine.two_sio_port0_interface=usb\nmachine.two_sio_port1_interface=ttl\n");
        assert_eq!(invalid.config.machine.two_sio_straps.port0_interface, TwoSioSignalInterface::Tty20mA);
        assert_eq!(invalid.config.machine.two_sio_straps.port1_interface, TwoSioSignalInterface::Ttl);
    }

    #[test]
    fn persisted_cables_cannot_bypass_selected_electrical_family() {
        let mut machine = crate::config::MachineConfig::default();
        machine.serial_board = SerialBoard::TwoSio88;
        machine.two_sio_straps.port0_interface = TwoSioSignalInterface::Rs232;
        machine.two_sio_straps.port1_interface = TwoSioSignalInterface::Tty20mA;
        assert_eq!(valid_connection(machine, SerialDevice::InternalAsr33, SerialConnection::Port0), SerialConnection::Disconnected);
        assert_eq!(valid_connection(machine, SerialDevice::InternalAsr33, SerialConnection::Port1), SerialConnection::Port1);
        assert_eq!(valid_connection(machine, SerialDevice::ExternalCom, SerialConnection::Port0), SerialConnection::Port0);
        assert_eq!(valid_connection(machine, SerialDevice::ExternalCom, SerialConnection::Port1), SerialConnection::Disconnected);
        assert_eq!(valid_connection(machine, SerialDevice::TextTerminal, SerialConnection::Port1), SerialConnection::Port1);
    }

    #[test]
    fn old_or_invalid_interrupt_wiring_keeps_safe_migration_default() {
        let old = SavedSettings::from_text("machine.serial_board=88-2sio\n");
        assert_eq!(old.config.machine.two_sio_interrupt_wiring.port0, TwoSioInterruptTarget::Pint);
        assert_eq!(old.config.machine.two_sio_interrupt_wiring.port1, TwoSioInterruptTarget::Pint);

        let invalid = SavedSettings::from_text("machine.two_sio_port0_irq=rst7\nmachine.two_sio_port1_irq=vi6\n");
        assert_eq!(invalid.config.machine.two_sio_interrupt_wiring.port0, TwoSioInterruptTarget::Pint);
        assert_eq!(invalid.config.machine.two_sio_interrupt_wiring.port1, TwoSioInterruptTarget::Vi6);
    }

    #[test]
    fn persistent_defaults_match_current_led_and_tape_calibration() {
        let saved = SavedSettings::default();
        assert_eq!(saved.led_brightness, DEFAULT_LED_BRIGHTNESS);
        assert_eq!(saved.led_aura, DEFAULT_LED_AURA);
        assert_eq!(saved.reader_speed, TapeTransportSpeed::Historical1x);
        assert_eq!(saved.punch_speed, TapeTransportSpeed::Historical1x);
        assert_eq!(saved.tape_bit_order, TapeBitOrder::Historical8To1);
    }

    #[test]
    fn atomic_save_replaces_existing_configuration_without_leaving_temp_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rustair-persistence-{}-{nonce}", std::process::id()));
        let path = dir.join("config.ini");

        let mut saved = SavedSettings::default();
        saved.save_to_path(&path).unwrap();
        saved.config.machine.ram_size = RamSize::K48;
        saved.save_to_path(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("machine.ram_size=48k"));
        assert!(text.contains("machine.ram_board_profile=fast-no-wait"));
        assert!(text.contains("machine.sio_hardware=rev1,c-tty,00,110,8,none,2"));
        assert!(text.contains("machine.two_sio_base=10"));
        assert!(text.contains("machine.two_sio_port0_baud=110"));
        assert!(text.contains("machine.two_sio_port1_baud=9600"));
        assert!(text.contains("machine.two_sio_port0_interface=tty20ma"));
        assert!(text.contains("machine.two_sio_port1_interface=rs232"));
        assert!(text.contains("machine.two_sio_port0_irq=pint"));
        assert!(text.contains("machine.two_sio_port1_irq=pint"));
        assert!(text.contains("asr33.reader_speed=1x"));
        assert!(text.contains("asr33.punch_speed=1x"));
        assert!(text.contains("asr33.tape_visual_order=8to1"));
        assert!(!dir.join(".config.ini.tmp").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sio_rejects_persisted_port_one_connections() {
        let machine = crate::config::MachineConfig::default();
        assert_eq!(
            valid_connection(machine, SerialDevice::TextTerminal, SerialConnection::Port1),
            SerialConnection::Disconnected
        );
    }
}

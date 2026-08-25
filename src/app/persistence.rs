use super::*;
use crate::config::{
    ComDataBits, ComFlowControl, ComParity, ComStopBits, ExternalComConfig,
    ExternalSerialCharacterMode, ExternalSerialConfig, ExternalSerialSpeed, TcpListenScope,
    TerminalDuplex,
};
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

const CONFIG_VERSION: u32 = 1;

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
            led_brightness: 1.0,
            led_aura: 1.0,
            muted: false,
        }
    }
}

impl SavedSettings {
    pub(super) fn load_or_default() -> Self {
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
                "machine.serial_board" => if let Some(v) = parse_serial_board(value) { saved.config.machine.serial_board = v; },
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
        let _ = writeln!(out, "machine.serial_board={}", serial_board_key(self.config.machine.serial_board));
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
        let _ = writeln!(out, "terminal.duplex={}", duplex_key(self.terminal_duplex));
        let _ = writeln!(out, "terminal.uppercase={}", self.terminal_uppercase);
        let _ = writeln!(out, "led.brightness={:.3}", self.led_brightness);
        let _ = writeln!(out, "led.aura={:.3}", self.led_aura);
        let _ = writeln!(out, "audio.muted={}", self.muted);
        out
    }

    fn save(&self) -> Result<(), String> {
        let path = config_path();
        let parent = path.parent().ok_or_else(|| "configuration path has no parent".to_owned())?;
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        fs::write(&path, self.to_text()).map_err(|e| format!("could not write {}: {e}", path.display()))
    }
}

impl RusTairApp {
    pub(super) fn apply_persisted_settings(&mut self, saved: &SavedSettings) {
        self.config = saved.config;
        let engine = if saved.engine.is_available() { saved.engine } else { EmulationEngine::RustFast8080 };
        if self.machine.engine() != engine {
            let _ = self.machine.replace_engine(engine);
        }
        self.machine.configure_memory(self.config.machine.ram_size, self.config.machine.ram_init);
        self.machine.configure_serial_board(self.config.machine.serial_board);

        self.serial_router.reset_for_board(self.config.machine.serial_board);
        let board = self.config.machine.serial_board;
        for (device, connection) in [
            (SerialDevice::InternalAsr33, valid_connection(board, saved.asr_connection)),
            (SerialDevice::TextTerminal, valid_connection(board, saved.terminal_connection)),
            (SerialDevice::ExternalTcp, valid_connection(board, saved.external_tcp_connection)),
            (SerialDevice::ExternalCom, valid_connection(board, saved.external_com_connection)),
        ] {
            self.serial_router.connect(device, connection);
        }

        self.external_serial.config = saved.external_serial;
        self.external_serial.server.restart_on_next_poll();
        self.external_serial.reset_line_timing();
        self.external_com.config = saved.external_com.clone();
        self.external_com.port.restart_on_next_poll();
        self.external_com.reset_line_timing();

        self.asr33.duplex = saved.asr_duplex;
        self.terminal.duplex = saved.terminal_duplex;
        self.terminal.uppercase = saved.terminal_uppercase;
        self.terminal.speed = self.config.peripherals.terminal_speed;
        self.tty.set_mode(saved.tty_mode);
        self.led_brightness = saved.led_brightness.clamp(0.25, 3.0);
        self.led_aura = saved.led_aura.clamp(0.0, 3.0);
        self.audio.set_muted(saved.muted);
    }

    pub(super) fn capture_persisted_settings(&self) -> SavedSettings {
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
            led_brightness: self.led_brightness,
            led_aura: self.led_aura,
            muted: self.audio.muted(),
        }
    }

    pub(super) fn persist_configuration_if_changed(&mut self) {
        let current = self.capture_persisted_settings();
        if current == self.last_saved_settings {
            return;
        }
        if let Err(error) = current.save() {
            eprintln!("RusTair configuration save failed: {error}");
        }
        // Remember the attempted snapshot as well. This avoids hammering the
        // filesystem every frame if a path is temporarily unwritable; the next
        // user configuration change will trigger another save attempt.
        self.last_saved_settings = current;
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

fn valid_connection(board: SerialBoard, connection: SerialConnection) -> SerialConnection {
    if board == SerialBoard::Sio88 && connection == SerialConnection::Port1 {
        SerialConnection::Disconnected
    } else {
        connection
    }
}

fn encode_hex_string(value: &str) -> String {
    value.as_bytes().iter().map(|byte| format!("{byte:02X}")).collect()
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
fn serial_board_key(v: SerialBoard) -> &'static str { match v { SerialBoard::Sio88 => "88-sio", SerialBoard::TwoSio88 => "88-2sio" } }
fn parse_serial_board(v: &str) -> Option<SerialBoard> { Some(match v { "88-sio" => SerialBoard::Sio88, "88-2sio" => SerialBoard::TwoSio88, _ => return None }) }
fn asr_speed_key(v: Asr33Speed) -> &'static str { match v { Asr33Speed::Authentic110 => "110", Asr33Speed::Accelerated2x => "2x", Asr33Speed::Accelerated4x => "4x", Asr33Speed::Instant => "instant" } }
fn parse_asr_speed(v: &str) -> Option<Asr33Speed> { Some(match v { "110" => Asr33Speed::Authentic110, "2x" => Asr33Speed::Accelerated2x, "4x" => Asr33Speed::Accelerated4x, "instant" => Asr33Speed::Instant, _ => return None }) }
fn terminal_speed_key(v: TerminalSpeed) -> &'static str { match v { TerminalSpeed::Instant => "instant", TerminalSpeed::Baud300 => "300", TerminalSpeed::Baud1200 => "1200", TerminalSpeed::Baud2400 => "2400", TerminalSpeed::Baud9600 => "9600" } }
fn parse_terminal_speed(v: &str) -> Option<TerminalSpeed> { Some(match v { "instant" => TerminalSpeed::Instant, "300" => TerminalSpeed::Baud300, "1200" => TerminalSpeed::Baud1200, "2400" => TerminalSpeed::Baud2400, "9600" => TerminalSpeed::Baud9600, _ => return None }) }
fn emulation_speed_key(v: EmulationSpeed) -> &'static str { match v { EmulationSpeed::Authentic => "authentic", EmulationSpeed::X2 => "2x", EmulationSpeed::X5 => "5x", EmulationSpeed::X10 => "10x", EmulationSpeed::Unlimited => "unlimited" } }
fn parse_emulation_speed(v: &str) -> Option<EmulationSpeed> { Some(match v { "authentic" => EmulationSpeed::Authentic, "2x" => EmulationSpeed::X2, "5x" => EmulationSpeed::X5, "10x" => EmulationSpeed::X10, "unlimited" => EmulationSpeed::Unlimited, _ => return None }) }
fn engine_key(v: EmulationEngine) -> &'static str { match v { EmulationEngine::RustFast8080 => "rust-fast-8080", EmulationEngine::RustCycleAccurate8080 => "rust-cycle-8080", EmulationEngine::SimhAltair => "simh-altair", EmulationEngine::SimhAltairZ80 => "simh-altairz80" } }
fn parse_engine(v: &str) -> Option<EmulationEngine> { Some(match v { "rust-fast-8080" => EmulationEngine::RustFast8080, "rust-cycle-8080" => EmulationEngine::RustCycleAccurate8080, "simh-altair" => EmulationEngine::SimhAltair, "simh-altairz80" => EmulationEngine::SimhAltairZ80, _ => return None }) }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_text_round_trip_preserves_all_tunable_groups() {
        let mut saved = SavedSettings::default();
        saved.engine = EmulationEngine::RustCycleAccurate8080;
        saved.config.machine.ram_size = RamSize::K48;
        saved.config.machine.serial_board = SerialBoard::TwoSio88;
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
        saved.led_brightness = 1.37;
        saved.led_aura = 2.15;
        saved.muted = true;

        let decoded = SavedSettings::from_text(&saved.to_text());
        assert_eq!(decoded, saved);
    }

    #[test]
    fn persistent_defaults_match_current_led_calibration() {
        let saved = SavedSettings::default();
        assert_eq!(saved.led_brightness, 1.0);
        assert_eq!(saved.led_aura, 1.0);
    }

    #[test]
    fn sio_rejects_persisted_port_one_connections() {
        assert_eq!(
            valid_connection(SerialBoard::Sio88, SerialConnection::Port1),
            SerialConnection::Disconnected
        );
    }
}

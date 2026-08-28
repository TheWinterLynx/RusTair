use super::*;
use crate::config::{
    ComDataBits, ComFlowControl, ComParity, ComStopBits, ExternalComConfig,
    ExternalSerialCharacterMode, TerminalDuplex,
};
use crate::io::com_serial::{ComSerialTransport, ComTransportState};

const COM_POLL_INTERVAL: Duration = Duration::from_millis(10);
const UART_BUSY_RETRY: Duration = Duration::from_millis(1);
const COMMON_BAUD_RATES: [u32; 10] = [
    110, 300, 1_200, 2_400, 4_800, 9_600, 19_200, 38_400, 57_600, 115_200,
];

pub(super) struct ExternalComState {
    pub(super) window_open: bool,
    pub(super) config: ExternalComConfig,
    pub(super) port: ComSerialTransport,
    pub(super) available_ports: Vec<String>,
    pub(super) port_scan_error: Option<String>,
    tx_started: Option<Instant>,
}

impl Default for ExternalComState {
    fn default() -> Self {
        Self {
            window_open: false,
            config: ExternalComConfig::default(),
            port: ComSerialTransport::default(),
            available_ports: Vec::new(),
            port_scan_error: None,
            tx_started: None,
        }
    }
}

impl ExternalComState {
    pub(super) fn reset_line_timing(&mut self) { self.tx_started = None; }
}

impl RusTairApp {
    pub(in crate::app) fn external_com_connection(&self) -> SerialConnection {
        self.serial_connection(SerialDevice::ExternalCom)
    }

    pub(in crate::app) fn process_external_com(&mut self, ctx: &egui::Context) {
        let config = self.external_com.config.clone();
        self.external_com.port.poll(&config);
        if config.enabled { ctx.request_repaint_after(COM_POLL_INTERVAL); }

        let connection = self.external_com_connection();
        if !self.machine.powered() || !connection.is_connected() {
            self.external_com.port.clear_rx();
            self.external_com.reset_line_timing();
            return;
        }

        if self.external_com.port.rx_pending() != 0 {
            if self.serial_rx_empty_at(connection) {
                if let Some(raw_byte) = self.external_com.port.pop_rx() {
                    let byte = config.character_mode.rx_transform(raw_byte);
                    self.serial_receive_at(connection, byte);
                    if self.external_com.port.rx_pending() != 0 { ctx.request_repaint(); }
                }
            } else { ctx.request_repaint_after(UART_BUSY_RETRY); }
        }

        let now = Instant::now();
        let char_time = config.char_time();
        if !self.serial_tx_busy_at(connection) {
            self.external_com.tx_started = None;
            return;
        }
        if let Some(started) = self.external_com.tx_started {
            let elapsed = now.duration_since(started);
            if char_time.is_zero() || elapsed >= char_time {
                self.serial_tx_complete_at(connection);
                self.external_com.tx_started = None;
            } else { ctx.request_repaint_after(char_time - elapsed); }
        }
        if self.external_com.tx_started.is_none()
            && self.serial_tx_busy_at(connection)
            && let Some(byte) = self.serial_tx_front_at(connection)
        {
            let host_byte = config.character_mode.tx_transform(byte);
            self.external_com.port.queue_tx(host_byte);
            self.external_com.tx_started = Some(now);
            if char_time.is_zero() {
                self.serial_tx_complete_at(connection);
                self.external_com.tx_started = None;
                ctx.request_repaint();
            } else { ctx.request_repaint_after(char_time); }
        }
    }

    pub(in crate::app) fn refresh_external_com_ports(&mut self) {
        match ComSerialTransport::available_port_names() {
            Ok(ports) => { self.external_com.available_ports = ports; self.external_com.port_scan_error = None; }
            Err(error) => { self.external_com.available_ports.clear(); self.external_com.port_scan_error = Some(error); }
        }
    }

    fn apply_external_com_config(&mut self, next: ExternalComConfig) {
        let previous = self.external_com.config.clone();
        if previous == next { return; }
        let framing_changed = previous.baud_rate != next.baud_rate
            || previous.data_bits != next.data_bits || previous.parity != next.parity
            || previous.stop_bits != next.stop_bits;
        let hardware_changed = previous.enabled != next.enabled
            || previous.port_name != next.port_name || framing_changed
            || previous.flow_control != next.flow_control;
        let character_mode_changed = previous.character_mode != next.character_mode;
        self.external_com.config = next.clone();
        if hardware_changed { self.external_com.port.restart_on_next_poll(); }
        if framing_changed || character_mode_changed || previous.duplex != next.duplex { self.external_com.reset_line_timing(); }

        self.status = if next.enabled {
            if next.port_name.trim().is_empty() {
                "External COM enabled — select a serial port".into()
            } else {
                format!("External COM enabled: {} — {} — {} — {}", next.port_name, next.framing_label(), next.character_mode.label(), next.duplex.label())
            }
        } else { "External COM disabled".into() };
    }

    fn draw_external_com_config_controls(&mut self, ui: &mut egui::Ui, explanatory: bool) {
        let mut config = self.external_com.config.clone();
        ui.checkbox(&mut config.enabled, "Enable physical/virtual serial port");
        ui.horizontal_wrapped(|ui| {
            ui.label("Serial port:");
            let selected = if config.port_name.trim().is_empty() { "Select port…".to_owned() } else { config.port_name.clone() };
            egui::ComboBox::from_id_salt("external-com-port").selected_text(selected).show_ui(ui, |ui| {
                for port in &self.external_com.available_ports { ui.selectable_value(&mut config.port_name, port.clone(), port); }
            });
            if ui.button("Refresh ports").clicked() { self.refresh_external_com_ports(); }
        });
        ui.horizontal(|ui| {
            ui.label("Port name:");
            ui.add(egui::TextEdit::singleline(&mut config.port_name).desired_width(150.0).hint_text("COM3, /dev/ttyUSB0, …"));
        });
        ui.horizontal(|ui| {
            ui.label("Baud:");
            egui::ComboBox::from_id_salt("external-com-baud").selected_text(config.baud_rate.to_string()).show_ui(ui, |ui| {
                for baud in COMMON_BAUD_RATES { ui.selectable_value(&mut config.baud_rate, baud, baud.to_string()); }
            });
            ui.add(egui::DragValue::new(&mut config.baud_rate).range(1..=4_000_000).speed(100));
        });
        ui.horizontal(|ui| {
            ui.label("Data bits:");
            egui::ComboBox::from_id_salt("external-com-data-bits").selected_text(config.data_bits.label()).show_ui(ui, |ui| {
                for value in ComDataBits::ALL { ui.selectable_value(&mut config.data_bits, value, value.label()); }
            });
            ui.label("Parity:");
            egui::ComboBox::from_id_salt("external-com-parity").selected_text(config.parity.label()).show_ui(ui, |ui| {
                for value in ComParity::ALL { ui.selectable_value(&mut config.parity, value, value.label()); }
            });
            ui.label("Stop:");
            egui::ComboBox::from_id_salt("external-com-stop-bits").selected_text(config.stop_bits.label()).show_ui(ui, |ui| {
                for value in ComStopBits::ALL { ui.selectable_value(&mut config.stop_bits, value, value.label()); }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Flow control:");
            egui::ComboBox::from_id_salt("external-com-flow-control").selected_text(config.flow_control.label()).show_ui(ui, |ui| {
                for value in ComFlowControl::ALL { ui.selectable_value(&mut config.flow_control, value, value.label()); }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Character mode:");
            egui::ComboBox::from_id_salt("external-com-character-mode").selected_text(config.character_mode.label()).show_ui(ui, |ui| {
                for mode in ExternalSerialCharacterMode::ALL { ui.selectable_value(&mut config.character_mode, mode, mode.label()); }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Terminal duplex:");
            egui::ComboBox::from_id_salt("external-com-duplex").selected_text(config.duplex.label()).show_ui(ui, |ui| {
                for duplex in TerminalDuplex::ALL { ui.selectable_value(&mut config.duplex, duplex, duplex.label()); }
            });
        });
        if let Some(error) = &self.external_com.port_scan_error { ui.small(error); }
        if explanatory {
            ui.small("Framing configures the real host serial port. Character mode is a separate byte-level terminal model applied at the Altair boundary.");
            ui.small("COM RX is already physically paced by the host UART/driver, so RusTair does not add a second receive delay. The emulated UART still accepts only one unread character at a time.");
            ui.small("Altair TX-ready timing follows the selected real framing: start bit + data bits + optional parity + stop bits.");
            ui.small("Duplex describes the attached terminal. RusTair does not fabricate local-echo bytes; a physical terminal or host terminal program performs its own local echo when configured for half duplex.");
        }
        self.apply_external_com_config(config);
    }

    pub(in crate::app) fn draw_external_com_config_menu(&mut self, ui: &mut egui::Ui) {
        if self.external_com.available_ports.is_empty() { self.refresh_external_com_ports(); }
        self.draw_external_com_config_controls(ui, true);
        ui.separator();
        ui.small(self.external_com_status_text());
    }

    fn draw_external_com_connection_selector(&mut self, ui: &mut egui::Ui) {
        let board = self.config.machine.serial_board;
        let current = self.external_com_connection();
        let mut selected = current;
        ui.horizontal(|ui| {
            ui.label("Virtual cable:");
            egui::ComboBox::from_id_salt("external-com-serial-connection")
                .selected_text(Self::serial_connection_label(board, current)).show_ui(ui, |ui| {
                    ui.selectable_value(&mut selected, SerialConnection::Disconnected, "Disconnected");
                    ui.selectable_value(&mut selected, SerialConnection::Port0, Self::serial_connection_label(board, SerialConnection::Port0));
                    if board == SerialBoard::TwoSio88 {
                        ui.selectable_value(&mut selected, SerialConnection::Port1, Self::serial_connection_label(board, SerialConnection::Port1));
                    }
                });
        });
        if selected != current { self.set_serial_connection(SerialDevice::ExternalCom, selected); }
    }

    fn external_com_status_text(&self) -> String {
        let config = &self.external_com.config;
        if !config.enabled { return "COM endpoint: disabled".into(); }
        if let Some(error) = self.external_com.port.last_error() { return format!("COM endpoint error: {error}"); }
        let port = if self.external_com.port.active_port_name().is_empty() { config.port_name.as_str() } else { self.external_com.port.active_port_name() };
        match self.external_com.port.state() {
            ComTransportState::Disabled => "COM endpoint: disabled".into(),
            ComTransportState::Closed => format!("COM endpoint: closed — {port}"),
            ComTransportState::Opening => format!("COM endpoint: opening {port}…"),
            ComTransportState::Open => format!("COM endpoint: open {} — {} — {}", port, config.framing_label(), config.character_mode.label()),
            ComTransportState::Error => format!("COM endpoint: error — {port}"),
        }
    }

    fn draw_external_com_window(&mut self, ctx: &egui::Context) {
        if self.external_com.available_ports.is_empty() { self.refresh_external_com_ports(); }
        egui::TopBottomPanel::top("external-com-top").show(ctx, |ui| {
            self.draw_external_com_connection_selector(ui); ui.separator(); self.draw_external_com_config_controls(ui, false);
        });
        egui::TopBottomPanel::bottom("external-com-status").show(ctx, |ui| { ui.small(self.external_com_status_text()); });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("External serial — COM / host serial port");
            ui.label("Connect a real RS-232/USB serial adapter, virtual COM pair or Unix serial device to one emulated MITS serial port.");
            ui.separator();
            let config = self.external_com.config.clone();

            ui::collapsible_section(ui, "Transport state", true, |ui| {
                egui::Grid::new("external-com-counters").num_columns(2).show(ui, |ui| {
                    ui.label("Host port"); ui.monospace(if config.port_name.is_empty() { "--" } else { config.port_name.as_str() }); ui.end_row();
                    ui.label("Framing"); ui.monospace(config.framing_label()); ui.end_row();
                    ui.label("Flow control"); ui.monospace(config.flow_control.label()); ui.end_row();
                    ui.label("Character mode"); ui.monospace(config.character_mode.label()); ui.end_row();
                    ui.label("Terminal duplex"); ui.monospace(config.duplex.label()); ui.end_row();
                    ui.label("Host RX bytes"); ui.monospace(self.external_com.port.rx_bytes().to_string()); ui.end_row();
                    ui.label("Pending RX bytes"); ui.monospace(self.external_com.port.rx_pending().to_string()); ui.end_row();
                    ui.label("Host TX bytes"); ui.monospace(self.external_com.port.tx_bytes().to_string()); ui.end_row();
                    ui.label("Dropped RX bytes"); ui.monospace(self.external_com.port.dropped_rx_bytes().to_string()); ui.end_row();
                    ui.label("Dropped TX bytes"); ui.monospace(self.external_com.port.dropped_tx_bytes().to_string()); ui.end_row();
                });
            });

            ui.separator();
            ui::collapsible_section(ui, "Transport actions", true, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Clear pending RX").clicked() { self.external_com.port.clear_rx(); }
                    if ui.button("Reopen port").clicked() { self.external_com.port.restart_on_next_poll(); self.external_com.reset_line_timing(); ctx.request_repaint(); }
                    if ui.button("Refresh port list").clicked() { self.refresh_external_com_ports(); }
                });
            });

            ui.separator();
            ui::collapsible_section(ui, "How the COM bridge behaves", false, |ui| {
                ui.label("• The host COM device is a transport only; guest software still sees the selected 88-SIO/88-2SIO and its normal I/O addresses.");
                ui.label("• The OS serial driver applies baud rate, data bits, parity, stop bits and flow control to the actual host port.");
                ui.label("• Received host bytes enter the emulated UART only when its receive register is free; no second baud delay is added on top of the physical link.");
                ui.label("• Guest TX-ready timing is based on the complete selected asynchronous frame, even if the host driver buffers the byte immediately.");
                ui.label("• ASR-33 style strips bit 7 and uppercases host a-z on input. 7-bit ASCII preserves case. Raw 8-bit is byte-transparent.");
                ui.label("• Duplex never creates extra serial traffic. Any half-duplex local echo belongs to the attached terminal or terminal application.");
                ui.label("• Unplugging a USB serial adapter reports an endpoint error without blocking the emulator; use Reopen port after reconnecting it.");
            });
        });
    }

    pub(in crate::app) fn show_external_com_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.external_com.window_open { return; }
        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-external-com"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — External Serial / COM")
                .with_inner_size([800.0, 720.0])
                .with_min_inner_size([640.0, 500.0])
                .with_resizable(true),
            |external_ctx, _class| {
                self.draw_external_com_window(external_ctx);
                if external_ctx.input(|input| input.viewport().close_requested()) { self.external_com.window_open = false; }
            },
        );
    }
}

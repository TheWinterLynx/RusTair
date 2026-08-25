use super::*;
use crate::config::{
    ExternalSerialCharacterMode, ExternalSerialConfig, ExternalSerialSpeed, TcpListenScope,
    TerminalDuplex,
};
use crate::io::tcp_serial::TcpSerialServer;

const NETWORK_POLL_INTERVAL: Duration = Duration::from_millis(10);
const UART_BUSY_RETRY: Duration = Duration::from_millis(1);

pub(super) struct ExternalSerialState {
    pub(super) window_open: bool,
    pub(super) config: ExternalSerialConfig,
    pub(super) server: TcpSerialServer,
    tx_started: Option<Instant>,
    rx_next_at: Option<Instant>,
}

impl Default for ExternalSerialState {
    fn default() -> Self {
        Self {
            window_open: false,
            config: ExternalSerialConfig::default(),
            server: TcpSerialServer::default(),
            tx_started: None,
            rx_next_at: None,
        }
    }
}

impl ExternalSerialState {
    pub(super) fn reset_line_timing(&mut self) {
        self.tx_started = None;
        self.rx_next_at = None;
    }
}

impl RusTairApp {
    pub(in crate::app) fn external_tcp_connection(&self) -> SerialConnection {
        self.serial_connection(SerialDevice::ExternalTcp)
    }

    pub(in crate::app) fn process_external_serial(&mut self, ctx: &egui::Context) {
        let config = self.external_serial.config;
        self.external_serial.server.poll(config);

        if config.enabled { ctx.request_repaint_after(NETWORK_POLL_INTERVAL); }

        let connection = self.external_tcp_connection();
        if !self.machine.powered() || !connection.is_connected() {
            self.external_serial.server.clear_rx();
            self.external_serial.reset_line_timing();
            return;
        }

        let now = Instant::now();
        let char_time = config.speed.char_time();

        if self.external_serial.server.rx_pending() == 0 {
            self.external_serial.rx_next_at = None;
        } else {
            if self.external_serial.rx_next_at.is_none() { self.external_serial.rx_next_at = Some(now); }
            if self.serial_rx_empty_at(connection) {
                let due_in = self.external_serial.rx_next_at
                    .and_then(|due| due.checked_duration_since(now)).unwrap_or(Duration::ZERO);
                if due_in.is_zero() {
                    if let Some((raw_byte, _peer)) = self.external_serial.server.pop_rx() {
                        let byte = config.character_mode.rx_transform(raw_byte);
                        self.serial_receive_at(connection, byte);
                        if self.external_serial.server.rx_pending() == 0 {
                            self.external_serial.rx_next_at = None;
                        } else if char_time.is_zero() {
                            self.external_serial.rx_next_at = Some(now);
                            ctx.request_repaint();
                        } else {
                            self.external_serial.rx_next_at = Some(now + char_time);
                            ctx.request_repaint_after(char_time);
                        }
                    }
                } else { ctx.request_repaint_after(due_in); }
            } else { ctx.request_repaint_after(UART_BUSY_RETRY); }
        }

        if !self.serial_tx_busy_at(connection) {
            self.external_serial.tx_started = None;
        } else {
            if let Some(started) = self.external_serial.tx_started {
                let elapsed = now.duration_since(started);
                if char_time.is_zero() || elapsed >= char_time {
                    self.serial_tx_complete_at(connection);
                    self.external_serial.tx_started = None;
                } else { ctx.request_repaint_after(char_time - elapsed); }
            }

            if self.external_serial.tx_started.is_none()
                && self.serial_tx_busy_at(connection)
                && let Some(byte) = self.serial_tx_front_at(connection)
            {
                let host_byte = config.character_mode.tx_transform(byte);
                self.external_serial.server.broadcast_byte(host_byte);
                self.external_serial.server.flush_clients();
                self.external_serial.tx_started = Some(now);
                if char_time.is_zero() {
                    self.serial_tx_complete_at(connection);
                    self.external_serial.tx_started = None;
                    ctx.request_repaint();
                } else { ctx.request_repaint_after(char_time); }
            }
        }
    }

    fn apply_external_serial_config(&mut self, next: ExternalSerialConfig) {
        let previous = self.external_serial.config;
        if previous == next { return; }
        let listener_changed = previous.enabled != next.enabled
            || previous.listen_scope != next.listen_scope || previous.tcp_port != next.tcp_port;
        let speed_changed = previous.speed != next.speed;
        let character_mode_changed = previous.character_mode != next.character_mode;
        let duplex_changed = previous.duplex != next.duplex;
        self.external_serial.config = next;
        if listener_changed { self.external_serial.server.restart_on_next_poll(); }
        if speed_changed || character_mode_changed || duplex_changed { self.external_serial.reset_line_timing(); }

        self.status = if next.enabled {
            format!(
                "External TCP enabled: {}:{} — {} — {} — {} — {} client mode",
                next.listen_scope.bind_ipv4(), next.tcp_port, next.speed.label(),
                next.character_mode.label(), next.duplex.label(),
                if next.allow_multiple_clients { "multiple" } else { "single" }
            )
        } else { "External TCP disabled".into() };
    }

    fn draw_external_serial_config_controls(&mut self, ui: &mut egui::Ui, explanatory: bool) {
        let mut config = self.external_serial.config;
        ui.checkbox(&mut config.enabled, "Enable raw TCP server");
        ui.horizontal(|ui| {
            ui.label("Listen:");
            egui::ComboBox::from_id_salt("external-tcp-listen-scope")
                .selected_text(config.listen_scope.label()).show_ui(ui, |ui| {
                    for scope in TcpListenScope::ALL { ui.selectable_value(&mut config.listen_scope, scope, scope.label()); }
                });
        });
        ui.horizontal(|ui| {
            ui.label("TCP port:");
            ui.add(egui::DragValue::new(&mut config.tcp_port).range(1..=u16::MAX).speed(1));
        });
        ui.horizontal(|ui| {
            ui.label("Line speed:");
            egui::ComboBox::from_id_salt("external-tcp-line-speed")
                .selected_text(config.speed.label()).show_ui(ui, |ui| {
                    for speed in ExternalSerialSpeed::ALL { ui.selectable_value(&mut config.speed, speed, speed.label()); }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Character mode:");
            egui::ComboBox::from_id_salt("external-tcp-character-mode")
                .selected_text(config.character_mode.label()).show_ui(ui, |ui| {
                    for mode in ExternalSerialCharacterMode::ALL { ui.selectable_value(&mut config.character_mode, mode, mode.label()); }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Terminal duplex:");
            egui::ComboBox::from_id_salt("external-tcp-duplex")
                .selected_text(config.duplex.label()).show_ui(ui, |ui| {
                    for duplex in TerminalDuplex::ALL { ui.selectable_value(&mut config.duplex, duplex, duplex.label()); }
                });
        });
        ui.checkbox(&mut config.allow_multiple_clients, "Allow multiple TCP clients on this serial endpoint");

        if explanatory {
            ui.small("Raw TCP carries bytes only; no Telnet negotiation or echo-control protocol is inserted.");
            ui.small("For full duplex, configure the terminal client with local echo off. For half duplex, local echo belongs to the terminal client itself.");
            ui.small("ASR-33 style strips bit 7 and uppercases host a-z on input; 7-bit ASCII preserves case; Raw 8-bit is byte-transparent.");
            ui.small("Multiple-client mode fans out one serial endpoint behind the same virtual cable; it does not create additional Altair serial ports.");
            if config.listen_scope == TcpListenScope::AllInterfaces {
                ui.small("LAN mode exposes the listener beyond this PC. Windows Firewall/network policy may control who can reach it.");
            }
        }
        self.apply_external_serial_config(config);
    }

    pub(in crate::app) fn draw_external_serial_config_menu(&mut self, ui: &mut egui::Ui) {
        self.draw_external_serial_config_controls(ui, true);
        ui.separator();
        ui.small(self.external_tcp_status_text());
    }

    fn draw_external_connection_selector(&mut self, ui: &mut egui::Ui) {
        let board = self.config.machine.serial_board;
        let current = self.external_tcp_connection();
        let mut selected = current;
        ui.horizontal(|ui| {
            ui.label("Virtual cable:");
            egui::ComboBox::from_id_salt("external-tcp-serial-connection")
                .selected_text(Self::serial_connection_label(board, current)).show_ui(ui, |ui| {
                    ui.selectable_value(&mut selected, SerialConnection::Disconnected, "Disconnected");
                    ui.selectable_value(&mut selected, SerialConnection::Port0, Self::serial_connection_label(board, SerialConnection::Port0));
                    if board == SerialBoard::TwoSio88 {
                        ui.selectable_value(&mut selected, SerialConnection::Port1, Self::serial_connection_label(board, SerialConnection::Port1));
                    }
                });
        });
        if selected != current { self.set_serial_connection(SerialDevice::ExternalTcp, selected); }
    }

    fn external_tcp_status_text(&self) -> String {
        let config = self.external_serial.config;
        if !config.enabled { return "TCP server: disabled".into(); }
        if let Some(error) = self.external_serial.server.last_error() { return format!("TCP server error: {error}"); }
        if self.external_serial.server.listening() {
            let clients = self.external_serial.server.client_count();
            let bind = self.external_serial.server.active_bind().map(|address| address.to_string())
                .unwrap_or_else(|| format!("{}:{}", config.listen_scope.bind_ipv4(), config.tcp_port));
            return format!("TCP server: listening on {bind} — {clients} client{} — {} — {}",
                if clients == 1 { "" } else { "s" }, config.character_mode.label(), config.duplex.label());
        }
        "TCP server: starting…".into()
    }

    fn draw_external_serial_window(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("external-tcp-top").show(ctx, |ui| {
            self.draw_external_connection_selector(ui);
            ui.separator();
            self.draw_external_serial_config_controls(ui, false);
        });
        egui::TopBottomPanel::bottom("external-tcp-status").show(ctx, |ui| { ui.small(self.external_tcp_status_text()); });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("External serial — raw TCP");
            ui.label("Connect a terminal application to an emulated MITS serial port through a raw TCP byte stream.");
            ui.separator();
            let config = self.external_serial.config;
            if config.listen_scope == TcpListenScope::Loopback {
                ui.monospace(format!("PuTTY: Connection type = Raw   Host = 127.0.0.1   Port = {}", config.tcp_port));
            } else {
                ui.monospace(format!("PuTTY: Connection type = Raw   Host = <this PC's LAN IP>   Port = {}", config.tcp_port));
            }
            match config.duplex {
                TerminalDuplex::FullDuplexRemoteEcho => ui.monospace("PuTTY: Terminal -> Local echo = Force off   Local line editing = Force off"),
                TerminalDuplex::HalfDuplexLocalEcho => ui.monospace("PuTTY: Terminal -> Local echo = Force on    Local line editing = Force off"),
            };
            ui.small("Use Raw rather than Telnet so negotiation bytes never enter the guest serial stream.");

            ui.separator(); ui.strong("Transport state");
            egui::Grid::new("external-tcp-counters").num_columns(2).show(ui, |ui| {
                ui.label("Character mode"); ui.monospace(config.character_mode.label()); ui.end_row();
                ui.label("Terminal duplex"); ui.monospace(config.duplex.label()); ui.end_row();
                ui.label("Clients"); ui.monospace(self.external_serial.server.client_count().to_string()); ui.end_row();
                ui.label("Network RX bytes"); ui.monospace(self.external_serial.server.rx_bytes().to_string()); ui.end_row();
                ui.label("Pending RX bytes"); ui.monospace(self.external_serial.server.rx_pending().to_string()); ui.end_row();
                ui.label("Altair TX bytes"); ui.monospace(self.external_serial.server.tx_bytes().to_string()); ui.end_row();
                ui.label("Rejected extra clients"); ui.monospace(self.external_serial.server.rejected_clients().to_string()); ui.end_row();
                ui.label("Dropped slow-client TX copies"); ui.monospace(self.external_serial.server.dropped_tx_bytes().to_string()); ui.end_row();
            });

            let peers = self.external_serial.server.peer_addresses();
            if !peers.is_empty() {
                ui.separator(); ui.strong("Connected clients");
                for peer in peers { ui.monospace(peer.to_string()); }
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Disconnect client(s)").clicked() { self.external_serial.server.disconnect_all(); self.external_serial.reset_line_timing(); }
                if ui.button("Clear pending RX").clicked() { self.external_serial.server.clear_rx(); self.external_serial.rx_next_at = None; }
                if ui.button("Restart listener").clicked() {
                    self.external_serial.server.restart_on_next_poll(); self.external_serial.reset_line_timing(); ctx.request_repaint();
                }
            });

            ui.separator();
            ui.collapsing("How the serial bridge behaves", |ui| {
                ui.label("• TCP is only the host transport; the guest still sees the selected 88-SIO/88-2SIO UART and normal I/O addresses.");
                ui.label("• Duplex controls how the attached terminal should display typed input. RusTair does not create local-echo serial bytes.");
                ui.label("• ASR-33 style masks bit 7 in both directions and uppercases host a-z on input; 7-bit ASCII preserves input case; Raw 8-bit performs no transformation.");
                ui.label("• TCP may receive pasted text instantly, but bytes enter the UART at the configured line speed and only when its receive register is free.");
                ui.label("• Guest transmit-ready timing is paced even when no TCP client is connected.");
                ui.label("• Multiple-client mode broadcasts guest output to all clients and merges their input behind the one External TCP endpoint.");
            });
        });
    }

    pub(in crate::app) fn show_external_serial_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.external_serial.window_open { return; }
        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-external-tcp"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — External Serial / TCP")
                .with_inner_size([760.0, 680.0])
                .with_min_inner_size([620.0, 480.0])
                .with_resizable(true),
            |external_ctx, _class| {
                self.draw_external_serial_window(external_ctx);
                if external_ctx.input(|input| input.viewport().close_requested()) { self.external_serial.window_open = false; }
            },
        );
    }
}
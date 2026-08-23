use super::*;
use crate::config::{
    ExternalSerialCharacterMode, ExternalSerialConfig, ExternalSerialSpeed, TcpListenScope,
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

    /// Poll host TCP sockets and bridge the stream to whichever emulated serial
    /// port the External TCP cable is attached to. Socket I/O is always
    /// non-blocking; serial pacing remains wall-clock based and independent of
    /// CPU emulation speed. Character normalization is an endpoint option and
    /// is directional: an ASR-33-style host keyboard uppercases input, while
    /// terminal output only needs the historical 7-bit mask.
    pub(in crate::app) fn process_external_serial(&mut self, ctx: &egui::Context) {
        let config = self.external_serial.config;
        self.external_serial.server.poll(config);

        if config.enabled {
            // Keep accepting/reading sockets even while the Altair is stopped.
            ctx.request_repaint_after(NETWORK_POLL_INTERVAL);
        }

        let connection = self.external_tcp_connection();
        if !self.machine.powered || !connection.is_connected() {
            // A physical terminal cannot leave characters latched in a UART it
            // was not connected to. Discard host input accumulated while the
            // machine is off or the virtual cable is unplugged.
            self.external_serial.server.clear_rx();
            self.external_serial.reset_line_timing();
            return;
        }

        let now = Instant::now();
        let char_time = config.speed.char_time();

        // Host -> Altair. TCP may deliver a whole paste immediately, but the
        // emulated UART sees at most one character register at the configured
        // line rate.
        if self.external_serial.server.rx_pending() == 0 {
            self.external_serial.rx_next_at = None;
        } else {
            if self.external_serial.rx_next_at.is_none() {
                self.external_serial.rx_next_at = Some(now);
            }

            if self.serial_rx_empty_at(connection) {
                let due_in = self
                    .external_serial
                    .rx_next_at
                    .and_then(|due| due.checked_duration_since(now))
                    .unwrap_or(Duration::ZERO);

                if due_in.is_zero() {
                    if let Some(byte) = self.external_serial.server.pop_rx() {
                        let byte = config.character_mode.rx_transform(byte);
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
                } else {
                    ctx.request_repaint_after(due_in);
                }
            } else {
                // Wait until guest software consumes the one-character receive
                // register; line pacing cannot overrun the emulated UART.
                ctx.request_repaint_after(UART_BUSY_RETRY);
            }
        }

        // Altair -> host. The byte is made visible to TCP clients when its
        // serial frame starts; the emulated transmit register becomes ready
        // only after the configured character time has elapsed.
        if !self.serial_tx_busy_at(connection) {
            self.external_serial.tx_started = None;
        } else {
            if let Some(started) = self.external_serial.tx_started {
                let elapsed = now.duration_since(started);
                if char_time.is_zero() || elapsed >= char_time {
                    self.serial_tx_complete_at(connection);
                    self.external_serial.tx_started = None;
                } else {
                    ctx.request_repaint_after(char_time - elapsed);
                }
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
                } else {
                    ctx.request_repaint_after(char_time);
                }
            }
        }
    }

    fn apply_external_serial_config(&mut self, next: ExternalSerialConfig) {
        let previous = self.external_serial.config;
        if previous == next {
            return;
        }

        let listener_changed = previous.enabled != next.enabled
            || previous.listen_scope != next.listen_scope
            || previous.tcp_port != next.tcp_port;
        let speed_changed = previous.speed != next.speed;
        let character_mode_changed = previous.character_mode != next.character_mode;

        self.external_serial.config = next;
        if listener_changed {
            self.external_serial.server.restart_on_next_poll();
        }
        if speed_changed || character_mode_changed {
            self.external_serial.reset_line_timing();
        }

        self.status = if next.enabled {
            format!(
                "External TCP enabled: {}:{} — {} — {} — {} client mode",
                next.listen_scope.bind_ipv4(),
                next.tcp_port,
                next.speed.label(),
                next.character_mode.label(),
                if next.allow_multiple_clients {
                    "multiple"
                } else {
                    "single"
                }
            )
        } else {
            "External TCP disabled".into()
        };
    }

    fn draw_external_serial_config_controls(&mut self, ui: &mut egui::Ui, explanatory: bool) {
        let mut config = self.external_serial.config;

        ui.checkbox(&mut config.enabled, "Enable raw TCP server");

        ui.horizontal(|ui| {
            ui.label("Listen:");
            egui::ComboBox::from_id_salt("external-tcp-listen-scope")
                .selected_text(config.listen_scope.label())
                .show_ui(ui, |ui| {
                    for scope in TcpListenScope::ALL {
                        ui.selectable_value(&mut config.listen_scope, scope, scope.label());
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label("TCP port:");
            ui.add(
                egui::DragValue::new(&mut config.tcp_port)
                    .range(1..=u16::MAX)
                    .speed(1),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Line speed:");
            egui::ComboBox::from_id_salt("external-tcp-line-speed")
                .selected_text(config.speed.label())
                .show_ui(ui, |ui| {
                    for speed in ExternalSerialSpeed::ALL {
                        ui.selectable_value(&mut config.speed, speed, speed.label());
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label("Character mode:");
            egui::ComboBox::from_id_salt("external-tcp-character-mode")
                .selected_text(config.character_mode.label())
                .show_ui(ui, |ui| {
                    for mode in ExternalSerialCharacterMode::ALL {
                        ui.selectable_value(&mut config.character_mode, mode, mode.label());
                    }
                });
        });

        ui.checkbox(
            &mut config.allow_multiple_clients,
            "Allow multiple TCP clients on this serial endpoint",
        );

        if explanatory {
            ui.small("Single-client mode is the default. Extra connection attempts are rejected while one client is attached.");
            ui.small("Multiple-client mode broadcasts Altair TX to every client and merges every client's incoming bytes into the same UART RX stream.");
            ui.small("ASR-33 style is the default for early Altair software: it strips bit 7 and converts host keyboard a-z to A-Z before the byte reaches the UART.");
            ui.small("7-bit ASCII still strips bit 7 but preserves case. Raw 8-bit preserves every byte unchanged.");
            ui.small("Raw TCP means no Telnet negotiation is inserted or interpreted; character mode is a separate serial-terminal transformation.");
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
                .selected_text(Self::serial_connection_label(board, current))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut selected,
                        SerialConnection::Disconnected,
                        "Disconnected",
                    );
                    ui.selectable_value(
                        &mut selected,
                        SerialConnection::Port0,
                        Self::serial_connection_label(board, SerialConnection::Port0),
                    );
                    if board == SerialBoard::TwoSio88 {
                        ui.selectable_value(
                            &mut selected,
                            SerialConnection::Port1,
                            Self::serial_connection_label(board, SerialConnection::Port1),
                        );
                    }
                });
        });

        if selected != current {
            self.set_serial_connection(SerialDevice::ExternalTcp, selected);
        }
    }

    fn external_tcp_status_text(&self) -> String {
        let config = self.external_serial.config;
        if !config.enabled {
            return "TCP server: disabled".into();
        }
        if let Some(error) = self.external_serial.server.last_error() {
            return format!("TCP server error: {error}");
        }
        if self.external_serial.server.listening() {
            let clients = self.external_serial.server.client_count();
            let bind = self
                .external_serial
                .server
                .active_bind()
                .map(|address| address.to_string())
                .unwrap_or_else(|| {
                    format!("{}:{}", config.listen_scope.bind_ipv4(), config.tcp_port)
                });
            return format!(
                "TCP server: listening on {bind} — {clients} client{} — {}",
                if clients == 1 { "" } else { "s" },
                config.character_mode.label(),
            );
        }
        "TCP server: starting…".into()
    }

    fn draw_external_serial_window(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("external-tcp-top").show(ctx, |ui| {
            self.draw_external_connection_selector(ui);
            ui.separator();
            self.draw_external_serial_config_controls(ui, false);
        });

        egui::TopBottomPanel::bottom("external-tcp-status").show(ctx, |ui| {
            ui.small(self.external_tcp_status_text());
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("External serial — raw TCP");
            ui.label("Connect PuTTY or another application to the emulated MITS serial interface through a TCP transport. Character mode models what kind of terminal is connected behind that transport.");
            ui.separator();

            let config = self.external_serial.config;
            if config.listen_scope == TcpListenScope::Loopback {
                ui.monospace(format!(
                    "PuTTY: Connection type = Raw   Host = 127.0.0.1   Port = {}",
                    config.tcp_port
                ));
            } else {
                ui.monospace(format!(
                    "PuTTY: Connection type = Raw   Host = <this PC's LAN IP>   Port = {}",
                    config.tcp_port
                ));
            }
            ui.small("Do not select Telnet: Telnet negotiation bytes would become guest serial data.");

            ui.separator();
            ui.strong("Transport state");
            egui::Grid::new("external-tcp-counters")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Character mode");
                    ui.monospace(config.character_mode.label());
                    ui.end_row();
                    ui.label("Clients");
                    ui.monospace(self.external_serial.server.client_count().to_string());
                    ui.end_row();
                    ui.label("Network RX bytes");
                    ui.monospace(self.external_serial.server.rx_bytes().to_string());
                    ui.end_row();
                    ui.label("Pending RX bytes");
                    ui.monospace(self.external_serial.server.rx_pending().to_string());
                    ui.end_row();
                    ui.label("Altair TX bytes");
                    ui.monospace(self.external_serial.server.tx_bytes().to_string());
                    ui.end_row();
                    ui.label("Rejected extra clients");
                    ui.monospace(self.external_serial.server.rejected_clients().to_string());
                    ui.end_row();
                    ui.label("Dropped slow-client TX copies");
                    ui.monospace(self.external_serial.server.dropped_tx_bytes().to_string());
                    ui.end_row();
                });

            let peers = self.external_serial.server.peer_addresses();
            if !peers.is_empty() {
                ui.separator();
                ui.strong("Connected clients");
                for peer in peers {
                    ui.monospace(peer.to_string());
                }
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Disconnect client(s)").clicked() {
                    self.external_serial.server.disconnect_all();
                    self.external_serial.reset_line_timing();
                }
                if ui.button("Clear pending RX").clicked() {
                    self.external_serial.server.clear_rx();
                    self.external_serial.rx_next_at = None;
                }
                if ui.button("Restart listener").clicked() {
                    self.external_serial.server.restart_on_next_poll();
                    self.external_serial.reset_line_timing();
                    ctx.request_repaint();
                }
            });

            ui.separator();
            ui.collapsing("How the serial bridge behaves", |ui| {
                ui.label("• The TCP server is only the host transport; the guest still sees the selected 88-SIO/88-2SIO UART and its normal I/O addresses.");
                ui.label("• ASR-33 style masks bit 7 in both directions and uppercases host keyboard a-z on input, matching the uppercase-only keyboard expected by early software such as BASIC 3.2.");
                ui.label("• 7-bit ASCII masks bit 7 but preserves input case for later/case-aware terminal software.");
                ui.label("• Raw 8-bit performs no byte transformation and is intended for binary/protocol experiments.");
                ui.label("• TCP may receive pasted text instantly, but bytes enter the UART at the configured line speed and only when its receive register is free.");
                ui.label("• Guest transmit-ready timing is also paced, even when no TCP client is connected.");
                ui.label("• With multiple clients enabled, guest output is broadcast to all clients; all client input shares one merged RX stream.");
                ui.label("• Each emulated serial port still has one virtual cable/endpoint. Multi-client fan-out happens behind the External TCP endpoint, not on the Altair bus.");
            });
        });
    }

    pub(in crate::app) fn show_external_serial_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.external_serial.window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-external-tcp"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — External Serial / TCP")
                .with_inner_size([760.0, 680.0])
                .with_min_inner_size([620.0, 480.0])
                .with_resizable(true),
            |external_ctx, _class| {
                self.draw_external_serial_window(external_ctx);
                if external_ctx.input(|input| input.viewport().close_requested()) {
                    self.external_serial.window_open = false;
                }
            },
        );
    }
}

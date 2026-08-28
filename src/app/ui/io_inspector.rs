use super::super::*;

const IO_INSPECTOR_OPEN: &str = "rustair-io-inspector-open";
const IO_CAPTURE_ENABLED: &str = "rustair-io-inspector-capture-enabled";
const TRACE_FOLLOW_NEWEST: &str = "rustair-io-inspector-follow-newest";
const TRACE_VIEW_GENERATION: &str = "rustair-io-inspector-trace-view-generation";
const TRACE_ACTIVE_VIEW: &str = "rustair-io-inspector-active-trace";
const SELECTED_PORT: &str = "rustair-io-inspector-selected-port";
const WRITE_VALUE: &str = "rustair-io-inspector-write-value";
const INJECT_BYTES: &str = "rustair-io-inspector-inject-bytes";
const TRACE_SELECTED_ONLY: &str = "rustair-io-inspector-trace-selected-only";
const TRACE_SHOW_STATUS_POLLS: &str = "rustair-io-inspector-show-status-polls";

impl RusTairApp {
    fn set_io_inspector_open(ctx: &egui::Context, open: bool) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(IO_INSPECTOR_OPEN), open));
    }

    fn io_inspector_is_open(ctx: &egui::Context) -> bool {
        ctx.data_mut(|data| *data.get_temp_mut_or(egui::Id::new(IO_INSPECTOR_OPEN), false))
    }

    fn set_io_capture_requested(ctx: &egui::Context, enabled: bool) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(IO_CAPTURE_ENABLED), enabled));
    }

    fn io_capture_requested(ctx: &egui::Context) -> bool {
        ctx.data_mut(|data| *data.get_temp_mut_or(egui::Id::new(IO_CAPTURE_ENABLED), true))
    }

    fn trace_follow_newest(ctx: &egui::Context) -> bool {
        ctx.data_mut(|data| *data.get_temp_mut_or(egui::Id::new(TRACE_FOLLOW_NEWEST), true))
    }

    fn set_trace_follow_newest(ctx: &egui::Context, follow: bool) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(TRACE_FOLLOW_NEWEST), follow));
    }

    fn trace_view_generation(ctx: &egui::Context) -> u64 {
        ctx.data_mut(|data| *data.get_temp_mut_or(egui::Id::new(TRACE_VIEW_GENERATION), 0_u64))
    }

    fn bump_trace_view_generation(ctx: &egui::Context) {
        ctx.data_mut(|data| {
            let generation = data.get_temp_mut_or(egui::Id::new(TRACE_VIEW_GENERATION), 0_u64);
            *generation = generation.wrapping_add(1);
        });
    }

    fn active_trace_view(ctx: &egui::Context) -> u8 {
        ctx.data_mut(|data| *data.get_temp_mut_or(egui::Id::new(TRACE_ACTIVE_VIEW), 0_u8))
    }

    fn set_active_trace_view(ctx: &egui::Context, view: u8) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(TRACE_ACTIVE_VIEW), view));
    }

    pub(in crate::app) fn open_io_inspector(&mut self, ctx: &egui::Context) {
        Self::set_io_inspector_open(ctx, true);
        Self::set_io_capture_requested(ctx, true);
        Self::set_trace_follow_newest(ctx, true);
        Self::set_active_trace_view(ctx, 0);
        self.machine.clear_io_trace();
        self.external_serial.server.clear_network_trace();
        self.external_com.port.clear_trace();
        Self::bump_trace_view_generation(ctx);
    }

    fn port_name(&self, port: u8) -> &'static str {
        match (self.config.machine.serial_board, port) {
            (SerialBoard::Sio88, 0x00) => "MITS 88-SIO status",
            (SerialBoard::Sio88, 0x01) => "MITS 88-SIO data",
            (SerialBoard::Sio88, 0x10) => "88-2SIO absent status",
            (SerialBoard::Sio88, 0x12) => "88-2SIO absent status",
            (SerialBoard::TwoSio88, 0x00) => "88-SIO absent status",
            (SerialBoard::TwoSio88, 0x10) => "88-2SIO Port 0 status/control",
            (SerialBoard::TwoSio88, 0x11) => "88-2SIO Port 0 data",
            (SerialBoard::TwoSio88, 0x12) => "88-2SIO Port 1 status/control",
            (SerialBoard::TwoSio88, 0x13) => "88-2SIO Port 1 data",
            (_, 0xff) => "Altair front-panel sense/data port",
            _ => "Unmapped",
        }
    }

    fn is_serial_data_port(&self, port: u8) -> bool {
        match self.config.machine.serial_board {
            SerialBoard::Sio88 => port == 0x01,
            SerialBoard::TwoSio88 => matches!(port, 0x11 | 0x13),
        }
    }

    fn is_serial_status_port(&self, port: u8) -> bool {
        match self.config.machine.serial_board {
            SerialBoard::Sio88 => port == 0x00,
            SerialBoard::TwoSio88 => matches!(port, 0x10 | 0x12),
        }
    }

    fn byte_text(byte: u8) -> String {
        match byte {
            0x00 => "NUL".into(), 0x07 => "BEL".into(), 0x08 => "BS".into(),
            0x09 => "TAB".into(), 0x0a => "LF".into(), 0x0d => "CR".into(),
            0x1b => "ESC".into(), 0x20..=0x7e => format!("'{}'", byte as char),
            _ => ".".into(),
        }
    }

    fn trace_kind(kind: u8) -> &'static str {
        match kind {
            0 => "CPU IN", 1 => "CPU OUT", 2 => "UART RX <= endpoint",
            3 => "UART TX complete", _ => "?",
        }
    }

    fn parse_hex_byte(text: &str) -> Result<u8, String> {
        let trimmed = text.trim();
        let without_prefix = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")).unwrap_or(trimmed);
        let without_suffix = without_prefix.strip_suffix('h').or_else(|| without_prefix.strip_suffix('H')).unwrap_or(without_prefix);
        u8::from_str_radix(without_suffix, 16).map_err(|_| format!("'{text}' is not an 8-bit hexadecimal value"))
    }

    fn parse_hex_sequence(text: &str) -> Result<Vec<u8>, String> {
        let normalized = text.replace(',', " ").replace(';', " ");
        let mut bytes = Vec::new();
        for token in normalized.split_whitespace() { bytes.push(Self::parse_hex_byte(token)?); }
        if bytes.is_empty() { return Err("Enter at least one hexadecimal byte".into()); }
        Ok(bytes)
    }

    fn draw_capture_toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut capture_requested = Self::io_capture_requested(&ctx);
        let mut follow_newest = Self::trace_follow_newest(&ctx);

        ui.horizontal_wrapped(|ui| {
            if capture_requested {
                ui.strong("CAPTURE ACTIVE");
                if ui.small_button("Pause").clicked() { capture_requested = false; }
            } else {
                ui.label("CAPTURE PAUSED");
                if ui.small_button("Resume").clicked() { capture_requested = true; follow_newest = true; }
            }

            if ui.small_button("Clear traces & pause")
                .on_hover_text("Clear UART, TCP and COM trace views, reset their scroll state, and pause capture")
                .clicked()
            {
                capture_requested = false;
                self.machine.clear_io_trace();
                self.external_serial.server.clear_network_trace();
                self.external_com.port.clear_trace();
                Self::bump_trace_view_generation(&ctx);
            }
            ui.checkbox(&mut follow_newest, "Follow newest");
        });

        Self::set_io_capture_requested(&ctx, capture_requested);
        Self::set_trace_follow_newest(&ctx, follow_newest);
    }

    fn draw_io_header(&mut self, ui: &mut egui::Ui) {
        let cpu = self.machine.intel8080_state();
        let panel = self.machine.front_panel_state();
        ui.horizontal_wrapped(|ui| {
            ui.strong("INTEL 8080 I/O INSPECTOR / EDITOR"); ui.separator();
            ui.label(self.config.machine.serial_board.label()); ui.separator();
            ui.monospace(format!("PC {:04X}h", cpu.pc)); ui.separator();
            ui.label(if panel.running { "RUNNING" } else { "STOPPED" }); ui.separator();
            self.draw_capture_toolbar(ui);
        });
    }

    fn draw_port_map(&mut self, ui: &mut egui::Ui, selected: &mut u8) {
        ui.small("00h–FFh I/O address space. '*' means the CPU has accessed the port in this machine session.");
        ui.add_space(3.0);
        let columns = ((ui.available_width() / 44.0).floor() as usize).clamp(4, 8);
        egui::Grid::new("io-port-map").num_columns(columns).spacing([4.0, 3.0]).show(ui, |ui| {
            for raw in 0u16..=255 {
                let port = raw as u8;
                let (_, _, in_count, out_count) = self.machine.io_port_activity(port);
                let label = if in_count != 0 || out_count != 0 { format!("{:02X}*", port) } else { format!("{:02X}", port) };
                let response = ui.selectable_label(*selected == port, label);
                if response.clicked() { *selected = port; }
                response.on_hover_text(format!("{:02X}h — {}\nIN count: {in_count}\nOUT count: {out_count}", port, self.port_name(port)));
                if (raw as usize + 1) % columns == 0 { ui.end_row(); }
            }
        });
    }

    fn draw_status_interpretation(&self, ui: &mut egui::Ui, port: u8, value: u8) {
        match (self.config.machine.serial_board, port) {
            (SerialBoard::Sio88, 0x00) => {
                ui.strong("88-SIO status"); ui.monospace(format!("{:08b}", value));
                ui.label(format!("bit 0 = {} → RX {}", value & 0x01, if value & 0x01 != 0 { "empty / wait" } else { "data available" }));
                ui.label(format!("bits 6/7 = {:02b} → TX {}", (value >> 6) & 0x03, if value & 0xc0 != 0 { "busy" } else { "ready" }));
                ui.small("BASIC 3.2 waits on IN 00h until bit 0 becomes 0, then consumes the byte with IN 01h.");
            }
            (SerialBoard::TwoSio88, 0x10 | 0x12) => {
                ui.strong("MC6850-style status"); ui.monospace(format!("{:08b}", value));
                ui.label(format!("bit 0 RDRF = {} → RX {}", value & 0x01, if value & 0x01 != 0 { "data available" } else { "empty" }));
                ui.label(format!("bit 1 TDRE = {} → TX {}", (value >> 1) & 0x01, if value & 0x02 != 0 { "ready" } else { "busy" }));
                ui.small("Writing this port targets the control register; CR1:CR0 = 11 performs the emulated master reset.");
            }
            (_, 0xff) => {
                ui.strong("Front-panel I/O port"); ui.monospace(format!("{:08b}", value));
                ui.small("IN FFh reads A15..A8 sense switches; OUT FFh drives the eight data lamps in the current panel model.");
            }
            _ => {}
        }
    }

    fn draw_selected_port(&mut self, ui: &mut egui::Ui, selected: u8) {
        let live = self.machine.peek_io_port(selected);
        let (last_in, last_out, in_count, out_count) = self.machine.io_port_activity(selected);
        let serial_data = self.is_serial_data_port(selected);

        ui.horizontal_wrapped(|ui| { ui.heading(format!("Port {:02X}h", selected)); ui.label(self.port_name(selected)); });
        ui.small("Peek is non-invasive. Controls explicitly labelled CPU-style, Inject, Clear or Complete are invasive debugger actions.");

        egui::Grid::new("io-selected-summary").num_columns(2).show(ui, |ui| {
            ui.label("Live / peek"); ui.monospace(format!("{:02X}h  {:08b}  {}", live, live, Self::byte_text(live))); ui.end_row();
            ui.label("Last IN"); ui.monospace(last_in.map_or_else(|| "--".into(), |v| format!("{:02X}h  {}", v, Self::byte_text(v)))); ui.end_row();
            ui.label("Last OUT"); ui.monospace(last_out.map_or_else(|| "--".into(), |v| format!("{:02X}h  {}", v, Self::byte_text(v)))); ui.end_row();
            ui.label("IN / OUT count"); ui.monospace(format!("{in_count} / {out_count}")); ui.end_row();
        });

        if self.is_serial_status_port(selected) || selected == 0xff {
            ui.separator();
            super::collapsible_section(ui, "Status interpretation", true, |ui| {
                self.draw_status_interpretation(ui, selected, live);
            });
        }

        ui.separator();
        super::collapsible_section(ui, "Debugger I/O controls", true, |ui| {
            let value_id = egui::Id::new(WRITE_VALUE).with(selected);
            let mut write_text = ui.data_mut(|data| data.get_temp_mut_or(value_id, format!("{:02X}", live)).clone());
            ui.horizontal_wrapped(|ui| {
                ui.label("Hex:"); ui.add(egui::TextEdit::singleline(&mut write_text).desired_width(55.0));
                if ui.small_button("Load peek").clicked() { write_text = format!("{:02X}", live); }
                if ui.small_button("CPU-style IN").clicked() {
                    let value = self.machine.debugger_input_port(selected);
                    self.status = format!("Debugger IN {:02X}h -> {:02X}h ({})", selected, value, Self::byte_text(value));
                }
                if ui.small_button("CPU-style OUT").clicked() {
                    match Self::parse_hex_byte(&write_text) {
                        Ok(value) => {
                            self.machine.debugger_output_port(selected, value);
                            self.status = format!("Debugger OUT {:02X}h <- {:02X}h ({})", selected, value, Self::byte_text(value));
                        }
                        Err(error) => self.status = error,
                    }
                }
            });
            ui.data_mut(|data| data.insert_temp(value_id, write_text));
        });

        if serial_data {
            ui.separator();
            super::collapsible_section(ui, "Serial DATA-port tools", true, |ui| {
                let inject_id = egui::Id::new(INJECT_BYTES).with(selected);
                let mut inject_text = ui.data_mut(|data| data.get_temp_mut_or(inject_id, "59 0D".to_owned()).clone());
                ui.horizontal_wrapped(|ui| {
                    ui.label("RX hex:"); ui.add(egui::TextEdit::singleline(&mut inject_text).desired_width(150.0));
                    if ui.small_button("Inject RX").clicked() {
                        match Self::parse_hex_sequence(&inject_text) {
                            Ok(bytes) => {
                                let mut injected = 0usize;
                                for byte in bytes { if self.machine.debugger_inject_serial_rx(selected, byte) { injected += 1; } }
                                self.status = format!("Injected {injected} byte(s) directly into UART RX at {:02X}h", selected);
                            }
                            Err(error) => self.status = error,
                        }
                    }
                });
                ui.data_mut(|data| data.insert_temp(inject_id, inject_text));
                ui.small("Example 59 0D = ASCII 'Y' + carriage return, bypassing host transports.");
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("Clear RX").clicked() { self.machine.debugger_clear_serial_rx(selected); }
                    if ui.small_button("Complete one TX").clicked() {
                        let completed = self.machine.debugger_complete_serial_tx(selected);
                        self.status = match completed {
                            Some(byte) => format!("Completed UART TX {:02X}h ({})", byte, Self::byte_text(byte)),
                            None => "UART TX was already empty".into(),
                        };
                    }
                    if ui.small_button("Clear TX").clicked() { self.machine.debugger_clear_serial_tx(selected); }
                });
            });
        }
    }

    fn draw_io_trace(&mut self, ui: &mut egui::Ui, selected: u8, generation: u64) {
        ui.horizontal_wrapped(|ui| { ui.strong("EMULATED I/O / UART TRACE"); ui.separator(); ui.small("identical adjacent accesses are coalesced as xN"); });
        let filter_id = egui::Id::new(TRACE_SELECTED_ONLY);
        let status_poll_id = egui::Id::new(TRACE_SHOW_STATUS_POLLS);
        let mut selected_only = ui.data_mut(|data| *data.get_temp_mut_or(filter_id, false));
        let mut show_status_polls = ui.data_mut(|data| *data.get_temp_mut_or(status_poll_id, false));
        let events = self.machine.io_trace_snapshot();
        let hidden_status_accesses: u64 = events.iter()
            .filter(|(_, kind, port, _, _)| *kind == 0 && self.is_serial_status_port(*port))
            .map(|(_, _, _, _, repeat)| *repeat as u64).sum();

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut selected_only, "Selected port only");
            ui.checkbox(&mut show_status_polls, "Show STATUS polling");
            if !show_status_polls && hidden_status_accesses != 0 { ui.small(format!("{hidden_status_accesses} reads hidden")); }
        });
        ui.data_mut(|data| data.insert_temp(filter_id, selected_only));
        ui.data_mut(|data| data.insert_temp(status_poll_id, show_status_polls));

        let follow_newest = Self::trace_follow_newest(ui.ctx());
        let height = ui.available_height().max(120.0);
        egui::ScrollArea::both().id_salt(("io-trace-scroll", generation)).max_height(height)
            .auto_shrink([false, false]).stick_to_bottom(follow_newest).show(ui, |ui| {
                egui::Grid::new(("io-trace-grid", generation)).striped(true).num_columns(6).show(ui, |ui| {
                    ui.strong("#"); ui.strong("Stage"); ui.strong("Port"); ui.strong("Hex"); ui.strong("ASCII"); ui.strong("Repeat"); ui.end_row();
                    for (sequence, kind, port, value, repeat) in events {
                        if selected_only && port != selected { continue; }
                        if !show_status_polls && kind == 0 && self.is_serial_status_port(port) { continue; }
                        ui.monospace(sequence.to_string()); ui.label(Self::trace_kind(kind)); ui.monospace(format!("{:02X}h", port));
                        ui.monospace(format!("{:02X}", value)); ui.monospace(Self::byte_text(value));
                        ui.monospace(if repeat > 1 { format!("x{repeat}") } else { String::new() }); ui.end_row();
                    }
                });
            });
    }

    fn draw_network_trace(&mut self, ui: &mut egui::Ui, generation: u64) {
        ui.horizontal_wrapped(|ui| { ui.strong("RAW TCP TRACE"); ui.separator(); ui.label(format!("Mode: {}", self.external_serial.config.character_mode.label())); });
        ui.small("Inbound raw bytes are shown before character transformation; UART value is what the emulated serial interface receives.");
        let follow_newest = Self::trace_follow_newest(ui.ctx());
        let mode = self.external_serial.config.character_mode;
        let events = self.external_serial.server.network_trace_snapshot();
        let height = ui.available_height().max(120.0);
        egui::ScrollArea::both().id_salt(("tcp-byte-trace-scroll", generation)).max_height(height)
            .auto_shrink([false, false]).stick_to_bottom(follow_newest).show(ui, |ui| {
                egui::Grid::new(("tcp-byte-trace-grid", generation)).striped(true).num_columns(7).show(ui, |ui| {
                    ui.strong("#"); ui.strong("Direction"); ui.strong("Raw"); ui.strong("Raw ASCII");
                    ui.strong("UART value"); ui.strong("UART ASCII"); ui.strong("Peer"); ui.end_row();
                    for (sequence, inbound, byte, peer) in events {
                        let uart_value = if inbound { mode.rx_transform(byte) } else { byte };
                        ui.monospace(sequence.to_string()); ui.label(if inbound { "TCP -> RusTair" } else { "RusTair -> TCP" });
                        ui.monospace(format!("{:02X}", byte)); ui.monospace(Self::byte_text(byte));
                        ui.monospace(format!("{:02X}", uart_value)); ui.monospace(Self::byte_text(uart_value));
                        ui.monospace(peer.map_or_else(String::new, |address| address.to_string())); ui.end_row();
                    }
                });
            });
    }

    fn draw_com_trace(&mut self, ui: &mut egui::Ui, generation: u64) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("COM / HOST SERIAL TRACE"); ui.separator();
            ui.label(format!("Mode: {}", self.external_com.config.character_mode.label())); ui.separator();
            ui.label(self.external_com.config.framing_label());
        });
        ui.small("Inbound raw bytes are captured as delivered by the host driver; outbound bytes are captured after character-mode transformation.");
        let follow_newest = Self::trace_follow_newest(ui.ctx());
        let mode = self.external_com.config.character_mode;
        let events = self.external_com.port.trace_snapshot();
        let height = ui.available_height().max(120.0);
        egui::ScrollArea::both().id_salt(("com-byte-trace-scroll", generation)).max_height(height)
            .auto_shrink([false, false]).stick_to_bottom(follow_newest).show(ui, |ui| {
                egui::Grid::new(("com-byte-trace-grid", generation)).striped(true).num_columns(7).show(ui, |ui| {
                    ui.strong("#"); ui.strong("Direction"); ui.strong("Raw"); ui.strong("Raw ASCII");
                    ui.strong("UART value"); ui.strong("UART ASCII"); ui.strong("Host port"); ui.end_row();
                    for (sequence, inbound, byte, port_name) in events {
                        let uart_value = if inbound { mode.rx_transform(byte) } else { byte };
                        ui.monospace(sequence.to_string()); ui.label(if inbound { "COM -> RusTair" } else { "RusTair -> COM" });
                        ui.monospace(format!("{:02X}", byte)); ui.monospace(Self::byte_text(byte));
                        ui.monospace(format!("{:02X}", uart_value)); ui.monospace(Self::byte_text(uart_value));
                        ui.monospace(port_name); ui.end_row();
                    }
                });
            });
    }

    fn draw_io_help(&self, ui: &mut egui::Ui) {
        ui.label("1. Wait until the guest is waiting for serial input.");
        ui.label("2. Clear traces & pause.");
        ui.label("3. Resume, reproduce one short event, then pause again.");
        ui.label("4. TCP/COM traces show host transport bytes and transformed UART values.");
        ui.label("5. Emulated I/O shows UART enqueue and the DATA-port value returned to the guest.");
        ui.label("6. STATUS polling is hidden by default so busy-wait loops do not bury useful DATA events.");
        ui.label("7. Inject RX bypasses host transports for an A/B test of UART and guest software.");
    }

    fn draw_io_sidebar(&mut self, ui: &mut egui::Ui, selected: &mut u8) {
        egui::ScrollArea::vertical().id_salt("io-inspector-sidebar-scroll").auto_shrink([false, false]).show(ui, |ui| {
            super::collapsible_section(ui, "Selected I/O port", true, |ui| self.draw_selected_port(ui, *selected));
            ui.separator();
            super::collapsible_section(ui, "I/O port map 00h–FFh", false, |ui| self.draw_port_map(ui, selected));
            ui.separator();
            super::collapsible_section(ui, "How to use the serial traces", false, |ui| self.draw_io_help(ui));
        });
    }

    fn draw_trace_tabs(&self, ui: &mut egui::Ui, active: &mut u8) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(active, 0, "Emulated I/O / UART");
            ui.selectable_value(active, 1, "Raw TCP");
            ui.selectable_value(active, 2, "COM / host serial");
            ui.separator(); ui.small("Choose one trace to use the full available height.");
        });
    }

    fn draw_io_inspector(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("io-inspector-toolbar").resizable(false).show(ctx, |ui| self.draw_io_header(ui));
        let selected_id = egui::Id::new(SELECTED_PORT);
        let mut selected = ctx.data_mut(|data| *data.get_temp_mut_or(selected_id, self.config.machine.serial_board.data_port()));
        egui::SidePanel::right("io-inspector-sidebar").resizable(true).default_width(365.0).width_range(300.0..=520.0)
            .show(ctx, |ui| self.draw_io_sidebar(ui, &mut selected));
        ctx.data_mut(|data| data.insert_temp(selected_id, selected));

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut active = Self::active_trace_view(ctx);
            self.draw_trace_tabs(ui, &mut active);
            Self::set_active_trace_view(ctx, active);
            ui.separator();
            let generation = Self::trace_view_generation(ctx);
            match active {
                1 => self.draw_network_trace(ui, generation),
                2 => self.draw_com_trace(ui, generation),
                _ => self.draw_io_trace(ui, selected, generation),
            }
        });
    }

    pub(in crate::app) fn show_io_inspector_viewport(&mut self, parent_ctx: &egui::Context) {
        if !Self::io_inspector_is_open(parent_ctx) { return; }
        let parent_for_close = parent_ctx.clone();
        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-io-inspector"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — Intel 8080 I/O Inspector / Editor")
                .with_inner_size([1360.0, 780.0])
                .with_min_inner_size([960.0, 560.0])
                .with_resizable(true),
            |io_ctx, _class| {
                self.draw_io_inspector(io_ctx);
                if io_ctx.input(|input| input.viewport().close_requested()) {
                    Self::set_io_inspector_open(&parent_for_close, false);
                }
            },
        );
    }
}

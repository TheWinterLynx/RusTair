impl RusTairApp {
    // ----------------------- ASR-33 -----------------------

    fn play_print_events(&mut self, events: &[PrintEvent]) {
        for event in events {
            match event {
                PrintEvent::Printable => {
                    self.audio.play_once("assets/printcharpadded.mp3");
                    self.print_head_raise_until = Some(Instant::now() + Duration::from_millis(100));
                }
                PrintEvent::CarriageReturn => self.audio.play_once("assets/crpadded.mp3"),
                PrintEvent::Bell => self.audio.play_once("assets/bellpadded.mp3"),
            }
        }
    }

    fn set_tty_mode(&mut self, mode: TtyMode) {
        if mode == self.tty.mode { return; }
        self.tty.set_mode(mode);
        self.audio.play_once("assets/powerbtn.mp3");
        self.tty_power_flash_until = None;
        if mode == TtyMode::Off {
            self.audio.stop_loop("tty-motor");
        } else {
            self.audio.start_loop("tty-motor", "assets/up-hum4.mp3");
        }
    }

    fn flash_tty_power(&mut self, ctx: &egui::Context) {
        self.tty_power_flash_until = Some(Instant::now() + Duration::from_secs(2));
        ctx.request_repaint_after(PANEL_FRAME);
    }

    fn send_tty_byte(&mut self, byte: u8) {
        if self.tty.mode == TtyMode::Off { return; }
        self.machine.bus.serial_rx.push_back(byte & 0x7f);
        let events = self.tty.print_local(byte);
        self.play_print_events(&events);
    }

    fn print_tty_serial_byte(&mut self, byte: u8) {
        let was_off = self.tty.mode == TtyMode::Off;
        let events = self.tty.print_serial(byte);
        if was_off && self.tty.mode == TtyMode::Line {
            self.audio.play_once("assets/powerbtn.mp3");
            self.audio.start_loop("tty-motor", "assets/up-hum4.mp3");
        }
        self.play_print_events(&events);
    }

    fn active_serial_tx_char_time(&self) -> Duration {
        if self.terminal_window_open {
            self.terminal_speed.char_time()
        } else {
            TTY_CHAR_TIME
        }
    }

    // The Altair's transmit holding register is paced by the currently active
    // serial endpoint. With only the ASR-33 visible, the original 110-baud-ish
    // mechanical timing remains in control. Opening the text terminal switches
    // the line to its selected electronic terminal speed.
    //
    // Every outgoing byte is also copied into a separate ASR-33 print queue.
    // That queue is consumed at the mechanical 90 ms/character rate regardless
    // of text-terminal speed, so a 9600-baud terminal no longer makes the
    // teletype unrealistically print at 9600 baud.
    fn process_serial_tx(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let char_time = self.active_serial_tx_char_time();

        if let Some(started) = self.serial_tx_started {
            if char_time.is_zero() || now.duration_since(started) >= char_time {
                self.machine.bus.serial_tx.pop_front();
                self.serial_tx_started = None;
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(char_time.saturating_sub(now.duration_since(started)));
                return;
            }
        }

        if self.serial_tx_started.is_none() {
            if let Some(&byte) = self.machine.bus.serial_tx.front() {
                self.terminal_receive_byte(byte);
                self.tty_output_queue.push_back(byte);
                self.serial_tx_started = Some(now);

                if char_time.is_zero() {
                    self.machine.bus.serial_tx.pop_front();
                    self.serial_tx_started = None;
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(char_time);
                }
            }
        }
    }

    fn process_tty_output_queue(&mut self, ctx: &egui::Context) {
        if self.tty_output_queue.is_empty() {
            self.tty_output_started = None;
            return;
        }

        let now = Instant::now();
        if let Some(started) = self.tty_output_started {
            let elapsed = now.duration_since(started);
            if elapsed < TTY_CHAR_TIME {
                ctx.request_repaint_after(TTY_CHAR_TIME - elapsed);
                return;
            }
            self.tty_output_started = None;
        }

        if self.tty_output_started.is_none() {
            if let Some(byte) = self.tty_output_queue.pop_front() {
                self.print_tty_serial_byte(byte);
                self.tty_output_started = Some(now);
                ctx.request_repaint_after(TTY_CHAR_TIME);
            }
        }
    }

    fn process_serial_devices(&mut self, ctx: &egui::Context) {
        self.process_serial_tx(ctx);
        self.process_tty_output_queue(ctx);
    }

    fn key_index_for_byte(byte: u8) -> Option<usize> {
        teletype::KEYS.iter().position(|key| match key.kind {
            KeyKind::Character(_) => {
                teletype::key_to_byte(key.kind, false, false) == Some(byte)
                    || teletype::key_to_byte(key.kind, true, false) == Some(byte)
            }
            KeyKind::Escape => byte == 0x1b,
            KeyKind::LineFeed => byte == b'\n',
            KeyKind::CarriageReturn => byte == b'\r',
            KeyKind::Delete => byte == 0x7f,
            KeyKind::Space => byte == b' ',
            KeyKind::Control | KeyKind::Shift => false,
        })
    }

    fn animate_keyboard_byte(&mut self, byte: u8, ctx: &egui::Context) {
        if let Some(index) = Self::key_index_for_byte(byte) {
            self.animated_key = Some(index);
            self.pressed_key = Some(index);
            self.key_auto_release_at = Some(Instant::now() + KEY_TAP_TIME);
            self.key_displacement = 0.0;
            self.key_anim_tick = Instant::now();
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    fn process_tty_keyboard(&mut self, ctx: &egui::Context) {
        let mut bytes = Vec::new();
        let mut any_key = false;

        ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Text(text) => {
                        any_key = true;
                        for b in text.bytes() {
                            bytes.push(b.to_ascii_uppercase());
                        }
                    }
                    egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } => {
                        any_key = true;
                        bytes.push(b'\r');
                    }
                    egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                        any_key = true;
                        bytes.push(0x7f);
                    }
                    egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                        any_key = true;
                        bytes.push(0x1b);
                    }
                    egui::Event::Key { key, pressed: true, modifiers, .. } if modifiers.ctrl => {
                        any_key = true;
                        let letter = match key {
                            egui::Key::A=>Some(b'A'), egui::Key::B=>Some(b'B'),
                            egui::Key::C=>Some(b'C'), egui::Key::D=>Some(b'D'),
                            egui::Key::E=>Some(b'E'), egui::Key::F=>Some(b'F'),
                            egui::Key::G=>Some(b'G'), egui::Key::H=>Some(b'H'),
                            egui::Key::I=>Some(b'I'), egui::Key::J=>Some(b'J'),
                            egui::Key::K=>Some(b'K'), egui::Key::L=>Some(b'L'),
                            egui::Key::M=>Some(b'M'), egui::Key::N=>Some(b'N'),
                            egui::Key::O=>Some(b'O'), egui::Key::P=>Some(b'P'),
                            egui::Key::Q=>Some(b'Q'), egui::Key::R=>Some(b'R'),
                            egui::Key::S=>Some(b'S'), egui::Key::T=>Some(b'T'),
                            egui::Key::U=>Some(b'U'), egui::Key::V=>Some(b'V'),
                            egui::Key::W=>Some(b'W'), egui::Key::X=>Some(b'X'),
                            egui::Key::Y=>Some(b'Y'), egui::Key::Z=>Some(b'Z'),
                            _=>None,
                        };
                        if let Some(letter) = letter { bytes.push(letter - 64); }
                    }
                    _ => {}
                }
            }
        });

        if self.tty.mode == TtyMode::Off {
            if any_key { self.flash_tty_power(ctx); }
            return;
        }

        for byte in bytes {
            self.animate_keyboard_byte(byte, ctx);
            self.send_tty_byte(byte);
        }
    }

    fn update_key_animation(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if self.key_auto_release_at.is_some_and(|until| now >= until) {
            self.pressed_key = None;
            self.key_auto_release_at = None;
        }

        let dt = now.duration_since(self.key_anim_tick).as_secs_f32().min(0.05);
        self.key_anim_tick = now;
        let velocity = 8.0 / 0.030;

        if self.pressed_key.is_some() {
            self.key_displacement = (self.key_displacement + velocity * dt).min(40.0);
        } else if self.key_displacement > 0.0 {
            self.key_displacement = (self.key_displacement - velocity * dt).max(0.0);
            if self.key_displacement == 0.0 { self.animated_key = None; }
        }

        if self.key_displacement > 0.0 || self.pressed_key.is_some() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    fn press_tty_key(&mut self, index: usize, ctx: &egui::Context) {
        if self.tty.mode == TtyMode::Off {
            self.flash_tty_power(ctx);
            return;
        }
        if self.pressed_key.is_some() { return; }

        self.pressed_key = Some(index);
        self.animated_key = Some(index);
        self.key_auto_release_at = None;
        self.key_displacement = 0.0;
        self.key_anim_tick = Instant::now();

        let key = teletype::KEYS[index];
        match key.kind {
            KeyKind::Shift => self.tty.shift_down = true,
            KeyKind::Control => self.tty.control_down = true,
            kind => {
                if let Some(byte) = teletype::key_to_byte(kind, self.tty.shift_down, self.tty.control_down) {
                    self.send_tty_byte(byte);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(8));
    }

    fn release_tty_key(&mut self) {
        if let Some(index) = self.pressed_key.take() {
            match teletype::KEYS[index].kind {
                KeyKind::Shift => self.tty.shift_down = false,
                KeyKind::Control => self.tty.control_down = false,
                _ => {}
            }
        }
        self.key_auto_release_at = None;
    }
}

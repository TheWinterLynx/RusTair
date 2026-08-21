use super::*;

impl RusTairApp {
    // ----------------------- ASR-33 -----------------------

    fn play_print_events(&mut self, events: &[PrintEvent]) {
        let now = Instant::now();
        for event in events {
            match event {
                PrintEvent::Printable(byte) => {
                    self.print_head_glyph = *byte;
                    self.print_head_raise_until = Some(now + PRINT_HEAD_STRIKE_TIME);
                    self.print_head_impact_at = Some(now + PRINT_HEAD_IMPACT_DELAY);
                }
                PrintEvent::CarriageReturn => {
                    self.audio.play_once("assets/crpadded.mp3");
                    self.print_head_auto_return_at = None;
                    self.print_head_raise_until = None;
                    self.print_head_impact_at = None;
                    self.print_head_carriage_return_until =
                        Some(now + PRINT_HEAD_CARRIAGE_RETURN_TIME);
                }
                PrintEvent::LineFeed => {
                    self.paper_feed_until = Some(now + PAPER_FEED_TIME);
                }
                PrintEvent::AutomaticReturn => {
                    self.print_head_auto_return_at = Some(now + PRINT_HEAD_STRIKE_TIME);
                }
                PrintEvent::Bell => self.audio.play_once("assets/bellpadded.mp3"),
            }
        }
    }

    fn process_tty_repeat(&mut self, ctx: &egui::Context) {
        let repeat_held = self.pressed_key.is_some_and(|index| {
            matches!(teletype::KEYS[index].kind, KeyKind::Repeat)
        });
        if !repeat_held {
            return;
        }

        let now = Instant::now();
        let timer_id = egui::Id::new("asr33-repeat-next-at");
        let next_at = ctx.data(|data| data.get_temp::<Instant>(timer_id));
        let next_at = next_at.unwrap_or(now + TTY_CHAR_TIME);

        if now >= next_at {
            if let Some(byte) = self.tty.last_key_byte {
                self.send_tty_byte(byte);
            }
            ctx.data_mut(|data| data.insert_temp(timer_id, now + TTY_CHAR_TIME));
        }
        ctx.request_repaint_after(Duration::from_millis(5));
    }

    pub(in crate::app) fn update_teletype_mechanics(&mut self, ctx: &egui::Context) {
        self.process_tty_repeat(ctx);
        let now = Instant::now();

        if self.print_head_impact_at.is_some_and(|at| now >= at) {
            self.audio.play_once("assets/printcharpadded.mp3");
            self.print_head_impact_at = None;
        }

        if self
            .print_head_auto_return_at
            .is_some_and(|at| now >= at)
        {
            self.print_head_auto_return_at = None;
            if self.tty.complete_auto_wrap() {
                self.audio.play_once("assets/crpadded.mp3");
                self.print_head_raise_until = None;
                self.print_head_impact_at = None;
                self.print_head_carriage_return_until =
                    Some(now + PRINT_HEAD_CARRIAGE_RETURN_TIME);
                self.paper_feed_until = Some(now + PAPER_FEED_TIME);
            }
        }

        if self.print_head_raise_until.is_some_and(|until| now >= until) {
            self.print_head_raise_until = None;
        }
        if self
            .print_head_carriage_return_until
            .is_some_and(|until| now >= until)
        {
            self.print_head_carriage_return_until = None;
        }
        if self.paper_feed_until.is_some_and(|until| now >= until) {
            self.paper_feed_until = None;
        }

        if self.print_head_impact_at.is_some()
            || self.print_head_auto_return_at.is_some()
            || self.print_head_raise_until.is_some()
            || self.print_head_carriage_return_until.is_some()
            || self.paper_feed_until.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    pub(in crate::app) fn set_tty_mode(&mut self, mode: TtyMode) {
        if mode == self.tty.mode { return; }
        self.tty.set_mode(mode);
        self.audio.play_once("assets/powerbtn.mp3");
        self.tty_power_flash_until = None;
        if mode == TtyMode::Off {
            self.audio.stop_loop("tty-motor");
            self.print_head_raise_until = None;
            self.print_head_impact_at = None;
            self.print_head_auto_return_at = None;
            self.print_head_carriage_return_until = None;
            self.paper_feed_until = None;
            self.tty_answerback_queue.clear();
            self.tty_answerback_next_at = None;
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
        let byte = byte & 0x7f;
        self.tty.last_key_byte = Some(byte);
        self.machine.bus.serial_receive(byte);
        let events = self.tty.print_local(byte);
        self.play_print_events(&events);
    }

    fn start_tty_answerback(&mut self, ctx: &egui::Context) {
        if self.tty.mode == TtyMode::Off {
            self.flash_tty_power(ctx);
            return;
        }

        if self.tty_answerback_queue.is_empty() {
            self.tty_answerback_queue
                .extend(TTY_ANSWERBACK.iter().copied());
            self.tty_answerback_next_at = Some(Instant::now());
        }
        ctx.request_repaint_after(Duration::from_millis(5));
    }

    pub(in crate::app) fn process_tty_answerback(&mut self, ctx: &egui::Context) {
        if self.terminal.window_open || self.tty_answerback_queue.is_empty() {
            return;
        }

        let now = Instant::now();
        if self.tty_answerback_next_at.is_some_and(|at| now < at) {
            ctx.request_repaint_after(Duration::from_millis(5));
            return;
        }

        if let Some(byte) = self.tty_answerback_queue.pop_front() {
            self.machine.bus.serial_receive(byte & 0x7f);
        }

        if self.tty_answerback_queue.is_empty() {
            self.tty_answerback_next_at = None;
        } else {
            self.tty_answerback_next_at = Some(now + TTY_CHAR_TIME);
            ctx.request_repaint_after(Duration::from_millis(5));
        }
    }

    pub(in crate::app) fn process_tty_serial(&mut self, ctx: &egui::Context) {
        let now = Instant::now();

        if let Some(started) = self.tty_tx_started {
            if now.duration_since(started) >= TTY_CHAR_TIME {
                self.machine.bus.serial_tx_complete();
                self.tty_tx_started = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(5));
                return;
            }
        }

        let carriage_returning = self
            .print_head_carriage_return_until
            .is_some_and(|until| now < until);
        if self.tty.auto_wrap_pending()
            || self.print_head_auto_return_at.is_some()
            || carriage_returning
        {
            ctx.request_repaint_after(Duration::from_millis(5));
            return;
        }

        if self.tty_tx_started.is_none() {
            if let Some(byte) = self.machine.bus.serial_tx_front() {
                let was_off = self.tty.mode == TtyMode::Off;
                let events = self.tty.print_serial(byte);
                if was_off && self.tty.mode == TtyMode::Line {
                    self.audio.play_once("assets/powerbtn.mp3");
                    self.audio.start_loop("tty-motor", "assets/up-hum4.mp3");
                }
                self.play_print_events(&events);

                if byte & 0x7f == 0x05 {
                    self.start_tty_answerback(ctx);
                }

                self.tty_tx_started = Some(now);
                ctx.request_repaint_after(PANEL_FRAME);
            }
        }
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
            KeyKind::Repeat
            | KeyKind::Break
            | KeyKind::HereIs
            | KeyKind::Control
            | KeyKind::Shift => false,
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

    pub(in crate::app) fn process_tty_keyboard(&mut self, ctx: &egui::Context) {
        let mut keystrokes: Vec<(u8, Option<u8>)> = Vec::new();
        let mut any_key = false;

        ctx.input(|input| {
            for (event_index, event) in input.events.iter().enumerate() {
                match event {
                    egui::Event::Text(text) => {
                        let host_autorepeat = event_index.checked_sub(1).is_some_and(|previous| {
                            matches!(
                                input.events[previous],
                                egui::Event::Key {
                                    pressed: true,
                                    repeat: true,
                                    ..
                                }
                            )
                        });
                        if host_autorepeat {
                            continue;
                        }

                        for ch in text.chars() {
                            let ch = ch.to_ascii_uppercase();
                            if !ch.is_ascii() {
                                continue;
                            }
                            let byte = ch as u8;
                            if !(0x20..=0x5f).contains(&byte) {
                                continue;
                            }
                            any_key = true;
                            keystrokes.push((byte, Some(byte)));
                        }
                    }
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        repeat: false,
                        ..
                    } => {
                        any_key = true;
                        keystrokes.push((b'\r', Some(b'\r')));
                        keystrokes.push((b'\n', None));
                    }
                    egui::Event::Key {
                        key: egui::Key::Backspace,
                        pressed: true,
                        repeat: false,
                        ..
                    } => {
                        any_key = true;
                        keystrokes.push((b'_', Some(0x7f)));
                    }
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        repeat: false,
                        ..
                    } => {
                        any_key = true;
                        keystrokes.push((0x1b, Some(0x1b)));
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.ctrl => {
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
                        if let Some(letter) = letter {
                            keystrokes.push((letter - 64, Some(letter)));
                        }
                    }
                    _ => {}
                }
            }
        });

        if self.tty.mode == TtyMode::Off {
            if any_key { self.flash_tty_power(ctx); }
            return;
        }

        for (byte, visual_byte) in keystrokes {
            if let Some(visual_byte) = visual_byte {
                self.animate_keyboard_byte(visual_byte, ctx);
            }
            self.send_tty_byte(byte);
        }
    }

    pub(in crate::app) fn update_key_animation(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if self.key_auto_release_at.is_some_and(|until| now >= until) {
            self.pressed_key = None;
            self.key_auto_release_at = None;
        }

        let dt = now.duration_since(self.key_anim_tick).as_secs_f32().min(0.05);
        self.key_anim_tick = now;
        let velocity = 8.0 / 0.030;

        if self.pressed_key.is_some() {
            self.key_displacement = (self.key_displacement + velocity * dt).min(7.0);
        } else if self.key_displacement > 0.0 {
            self.key_displacement = (self.key_displacement - velocity * dt).max(0.0);
            if self.key_displacement == 0.0 { self.animated_key = None; }
        }

        if self.key_displacement > 0.0 || self.pressed_key.is_some() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    pub(in crate::app) fn press_tty_key(&mut self, index: usize, ctx: &egui::Context) {
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
            KeyKind::HereIs => self.start_tty_answerback(ctx),
            KeyKind::Repeat => {
                if let Some(byte) = self.tty.last_key_byte {
                    self.send_tty_byte(byte);
                    let timer_id = egui::Id::new("asr33-repeat-next-at");
                    ctx.data_mut(|data| {
                        data.insert_temp(timer_id, Instant::now() + TTY_CHAR_TIME)
                    });
                    ctx.request_repaint_after(Duration::from_millis(5));
                }
            }
            kind => {
                let modifiers = ctx.input(|input| input.modifiers);
                let shifted = self.tty.shift_down || modifiers.shift;
                let controlled = self.tty.control_down || modifiers.ctrl;
                if let Some(byte) = teletype::key_to_byte(kind, shifted, controlled) {
                    self.send_tty_byte(byte);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(8));
    }

    pub(in crate::app) fn release_tty_key(&mut self) {
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

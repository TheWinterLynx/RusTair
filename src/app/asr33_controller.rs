use super::*;

impl RusTairApp {
    // ----------------------- ASR-33 -----------------------

    fn play_print_events(&mut self, events: &[PrintEvent]) {
        let now = Instant::now();
        let mechanics = &mut self.asr33.mechanics;
        for event in events {
            match event {
                PrintEvent::Printable(byte) => {
                    mechanics.print_head_glyph = *byte;
                    mechanics.print_head_raise_until = Some(now + PRINT_HEAD_STRIKE_TIME);
                    mechanics.print_head_impact_at = Some(now + PRINT_HEAD_IMPACT_DELAY);
                }
                PrintEvent::CarriageReturn => {
                    self.audio.play_once("assets/crpadded.mp3");
                    mechanics.print_head_auto_return_at = None;
                    mechanics.print_head_raise_until = None;
                    mechanics.print_head_impact_at = None;
                    mechanics.print_head_carriage_return_until =
                        Some(now + PRINT_HEAD_CARRIAGE_RETURN_TIME);
                }
                PrintEvent::LineFeed => {
                    mechanics.paper_feed_until = Some(now + PAPER_FEED_TIME);
                }
                PrintEvent::AutomaticReturn => {
                    mechanics.print_head_auto_return_at = Some(now + PRINT_HEAD_STRIKE_TIME);
                }
                PrintEvent::Bell => self.audio.play_once("assets/bellpadded.mp3"),
            }
        }
    }

    fn process_tty_repeat(&mut self, ctx: &egui::Context) {
        let repeat_held = self.asr33.keyboard.pressed_key.is_some_and(|index| {
            matches!(teletype::KEYS[index].kind, KeyKind::Repeat)
        });
        if !repeat_held {
            return;
        }

        let now = Instant::now();
        let char_time = self.asr_char_time();
        let timer_id = egui::Id::new("asr33-repeat-next-at");
        let next_at = ctx.data(|data| data.get_temp::<Instant>(timer_id));
        let next_at = next_at.unwrap_or(now + char_time);

        if now >= next_at {
            if let Some(byte) = self.tty.last_key_byte {
                self.send_tty_byte(byte);
            }
            ctx.data_mut(|data| data.insert_temp(timer_id, now + char_time));
        }
        ctx.request_repaint_after(Duration::from_millis(5));
    }

    pub(in crate::app) fn update_teletype_mechanics(&mut self, ctx: &egui::Context) {
        self.process_tty_repeat(ctx);
        let now = Instant::now();

        if self
            .asr33
            .mechanics
            .print_head_impact_at
            .is_some_and(|at| now >= at)
        {
            self.audio.play_once("assets/printcharpadded.mp3");
            self.asr33.mechanics.print_head_impact_at = None;
        }

        if self
            .asr33
            .mechanics
            .print_head_auto_return_at
            .is_some_and(|at| now >= at)
        {
            self.asr33.mechanics.print_head_auto_return_at = None;
            if self.tty.complete_auto_wrap() {
                self.audio.play_once("assets/crpadded.mp3");
                self.asr33.mechanics.print_head_raise_until = None;
                self.asr33.mechanics.print_head_impact_at = None;
                self.asr33.mechanics.print_head_carriage_return_until =
                    Some(now + PRINT_HEAD_CARRIAGE_RETURN_TIME);
                self.asr33.mechanics.paper_feed_until = Some(now + PAPER_FEED_TIME);
            }
        }

        if self
            .asr33
            .mechanics
            .print_head_raise_until
            .is_some_and(|until| now >= until)
        {
            self.asr33.mechanics.print_head_raise_until = None;
        }
        if self
            .asr33
            .mechanics
            .print_head_carriage_return_until
            .is_some_and(|until| now >= until)
        {
            self.asr33.mechanics.print_head_carriage_return_until = None;
        }
        if self
            .asr33
            .mechanics
            .paper_feed_until
            .is_some_and(|until| now >= until)
        {
            self.asr33.mechanics.paper_feed_until = None;
        }

        if self.asr33.mechanics.printing_active() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    pub(in crate::app) fn set_tty_mode(&mut self, mode: TtyMode) {
        if mode == self.tty.mode {
            return;
        }
        self.tty.set_mode(mode);
        self.audio.play_once("assets/powerbtn.mp3");
        self.asr33.power_flash_until = None;
        if mode == TtyMode::Off {
            self.audio.stop_loop("tty-motor");
            self.asr33.mechanics.clear_motion();
            self.asr33.answerback.clear();
        } else {
            self.audio.start_loop("tty-motor", "assets/up-hum4.mp3");
        }
    }

    fn flash_tty_power(&mut self, ctx: &egui::Context) {
        self.asr33.power_flash_until = Some(Instant::now() + Duration::from_secs(2));
        ctx.request_repaint_after(PANEL_FRAME);
    }

    fn send_tty_byte(&mut self, byte: u8) {
        if self.tty.mode == TtyMode::Off {
            return;
        }

        let byte = byte & 0x7f;
        self.tty.last_key_byte = Some(byte);

        match self.tty.mode {
            TtyMode::Off => {}
            // LOCAL is a true offline keyboard/printer loop: the character is
            // printed mechanically but is not injected into the Altair UART.
            TtyMode::Local => {
                let events = self.tty.print_local(byte);
                self.play_print_events(&events);
            }
            // LINE always transmits. Half duplex additionally routes the same
            // keyboard character to the local printer; full duplex waits for
            // the computer to echo it back through serial TX before printing.
            TtyMode::Line => {
                self.asr_serial_receive(byte);
                if self.asr33.duplex.local_echo() {
                    let events = self.tty.print_serial(byte);
                    self.play_print_events(&events);
                }
            }
        }
    }

    fn start_tty_answerback(&mut self, ctx: &egui::Context) {
        if self.tty.mode == TtyMode::Off {
            self.flash_tty_power(ctx);
            return;
        }
        if !self.asr_connection().is_connected() {
            self.asr33.answerback.clear();
            return;
        }

        self.asr33.answerback.trigger(Instant::now());
        ctx.request_repaint_after(Duration::from_millis(5));
    }

    pub(in crate::app) fn process_tty_answerback(&mut self, ctx: &egui::Context) {
        if !self.asr_connection().is_connected() || !self.asr33.answerback.pending() {
            return;
        }

        let now = Instant::now();
        if self.asr33.answerback.time_until_next(now).is_some() {
            ctx.request_repaint_after(Duration::from_millis(5));
            return;
        }

        let char_time = self.asr_char_time();
        if let Some(byte) = self.asr33.answerback.take_due(now, char_time) {
            self.asr_serial_receive(byte & 0x7f);
        }

        if self.asr33.answerback.pending() {
            ctx.request_repaint_after(Duration::from_millis(5));
        }
    }

    pub(in crate::app) fn process_tty_serial(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let char_time = self.asr_char_time();

        if let Some(started) = self.asr33.tx_started {
            if char_time.is_zero() || now.duration_since(started) >= char_time {
                self.asr_serial_tx_complete();
                self.asr33.tx_started = None;
            } else {
                ctx.request_repaint_after(char_time - now.duration_since(started));
                return;
            }
        }

        let carriage_returning = self
            .asr33
            .mechanics
            .print_head_carriage_return_until
            .is_some_and(|until| now < until);
        if self.tty.auto_wrap_pending()
            || self.asr33.mechanics.print_head_auto_return_at.is_some()
            || carriage_returning
        {
            ctx.request_repaint_after(Duration::from_millis(5));
            return;
        }

        if self.asr33.tx_started.is_none() {
            if let Some(byte) = self.asr_serial_tx_front() {
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

                if char_time.is_zero() {
                    self.asr_serial_tx_complete();
                    self.asr33.tx_started = None;
                    ctx.request_repaint();
                } else {
                    self.asr33.tx_started = Some(now);
                    ctx.request_repaint_after(char_time);
                }
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
            let keyboard = &mut self.asr33.keyboard;
            keyboard.animated_key = Some(index);
            keyboard.pressed_key = Some(index);
            keyboard.auto_release_at = Some(Instant::now() + KEY_TAP_TIME);
            keyboard.displacement = 0.0;
            keyboard.anim_tick = Instant::now();
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
            if any_key {
                self.flash_tty_power(ctx);
            }
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
        let keyboard = &mut self.asr33.keyboard;
        if keyboard.auto_release_at.is_some_and(|until| now >= until) {
            keyboard.pressed_key = None;
            keyboard.auto_release_at = None;
        }

        let dt = now.duration_since(keyboard.anim_tick).as_secs_f32().min(0.05);
        keyboard.anim_tick = now;
        let velocity = 8.0 / 0.030;

        if keyboard.pressed_key.is_some() {
            keyboard.displacement = (keyboard.displacement + velocity * dt).min(7.0);
        } else if keyboard.displacement > 0.0 {
            keyboard.displacement = (keyboard.displacement - velocity * dt).max(0.0);
            if keyboard.displacement == 0.0 {
                keyboard.animated_key = None;
            }
        }

        if keyboard.displacement > 0.0 || keyboard.pressed_key.is_some() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    pub(in crate::app) fn press_tty_key(&mut self, index: usize, ctx: &egui::Context) {
        if self.tty.mode == TtyMode::Off {
            self.flash_tty_power(ctx);
            return;
        }
        if self.asr33.keyboard.pressed_key.is_some() {
            return;
        }

        {
            let keyboard = &mut self.asr33.keyboard;
            keyboard.pressed_key = Some(index);
            keyboard.animated_key = Some(index);
            keyboard.auto_release_at = None;
            keyboard.displacement = 0.0;
            keyboard.anim_tick = Instant::now();
        }

        let key = teletype::KEYS[index];
        match key.kind {
            KeyKind::Shift => self.tty.shift_down = true,
            KeyKind::Control => self.tty.control_down = true,
            KeyKind::HereIs => self.start_tty_answerback(ctx),
            KeyKind::Repeat => {
                if let Some(byte) = self.tty.last_key_byte {
                    self.send_tty_byte(byte);
                    let timer_id = egui::Id::new("asr33-repeat-next-at");
                    let char_time = self.asr_char_time();
                    ctx.data_mut(|data| {
                        data.insert_temp(timer_id, Instant::now() + char_time)
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
        if let Some(index) = self.asr33.keyboard.pressed_key.take() {
            match teletype::KEYS[index].kind {
                KeyKind::Shift => self.tty.shift_down = false,
                KeyKind::Control => self.tty.control_down = false,
                _ => {}
            }
        }
        self.asr33.keyboard.auto_release_at = None;
    }
}

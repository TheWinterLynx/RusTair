impl RusTairApp {
    fn draw_pressed_key(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        let Some(index) = self.animated_key else { return; };
        if self.key_displacement <= 0.0 { return; }

        let key = teletype::KEYS[index];

        let source = Rect::from_min_max(
            Pos2::new(key.x / TTY_W, key.y / TTY_H),
            Pos2::new((key.x + key.w) / TTY_W, (key.y + key.h + 40.0) / TTY_H),
        );
        let target = Rect::from_min_size(
            origin + Vec2::new(key.x * scale, key.y * scale),
            Vec2::new(key.w * scale, (key.h + 40.0) * scale),
        );
        if let Some(body) = &self.tex.tty_body {
            ui.painter().image(body.id(), target, source, Color32::WHITE);
        }

        let key_source = Rect::from_min_max(
            Pos2::new(key.x / TTY_W, key.y / TTY_H),
            Pos2::new((key.x + key.w) / TTY_W, (key.y + key.h) / TTY_H),
        );
        let key_target = Rect::from_min_size(
            origin + Vec2::new(key.x * scale, (key.y + self.key_displacement) * scale),
            Vec2::new(key.w * scale, key.h * scale),
        );
        if let Some(keys) = &self.tex.tty_keys {
            ui.painter().image(keys.id(), key_target, key_source, Color32::WHITE);
        }
    }

    fn draw_paper_text(&self, ui: &mut egui::Ui, paper: Rect, scale: f32) {
        let char_width = self.tty.char_width_image_px();
        let font_size = (char_width * 1.63 * scale).max(5.0);
        let line_height = (font_size * 1.03).max(6.0);
        let max_lines = ((paper.height() / line_height).floor() as usize).max(1);
        let lines: Vec<&str> = self.tty.output.split('\n').collect();
        let first = lines.len().saturating_sub(max_lines);
        let visible = &lines[first..];
        let painter = ui.painter().with_clip_rect(paper);

        for (row, line) in visible.iter().enumerate() {
            let from_bottom = visible.len() - 1 - row;
            let y = paper.bottom() - from_bottom as f32 * line_height;
            painter.text(
                Pos2::new(paper.left(), y),
                egui::Align2::LEFT_BOTTOM,
                *line,
                FontId::new(font_size, FontFamily::Name("teletype".into())),
                Color32::from_rgb(35, 35, 30),
            );
        }
    }

    fn draw_teletype(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let scale = (available.x / TTY_W).min(available.y / TTY_H).clamp(0.12, 1.5);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(TTY_W * scale, TTY_H * scale),
            Sense::click_and_drag(),
        );
        let origin = rect.min;

        if let Some(t) = &self.tex.tty_body { Self::image(ui, t, rect); }
        if let Some(t) = &self.tex.tty_keys { Self::image(ui, t, rect); }

        let paper = Rect::from_min_max(
            origin + Vec2::new(teletype::PRINT_LEFT * scale, 0.0),
            origin + Vec2::new(
                (teletype::PRINT_LEFT + teletype::PRINTABLE_WIDTH) * scale,
                teletype::PRINT_TOP * scale,
            ),
        );
        self.draw_paper_text(ui, paper, scale);

        if self.tty.mode != TtyMode::Off {
            if let Some(t) = &self.tex.tty_head {
                let char_width = self.tty.char_width_image_px();
                let x = teletype::PRINT_LEFT + self.tty.column as f32 * char_width - char_width;
                let raised = self.print_head_raise_until.is_some_and(|until| Instant::now() < until);
                let y = teletype::PRINT_HEAD_TOP - if raised { TTY_H * 0.02 } else { 0.0 };
                let head_rect = Rect::from_min_size(
                    origin + Vec2::new(x * scale, y * scale),
                    Vec2::new(
                        TTY_W * 0.06 * scale,
                        TTY_H * (if raised { 0.075 } else { 0.10 }) * scale,
                    ),
                );
                Self::image(ui, t, head_rect);
            }
        }

        let selector_size = Vec2::new(
            TTY_W * 0.18 * scale,
            288.0 * (TTY_W * 0.18 / 349.0) * scale,
        );
        let mut selector = Rect::from_min_size(
            Pos2::new(rect.right() - selector_size.x, rect.bottom() - selector_size.y),
            selector_size,
        );

        let flashing = self.tty_power_flash_until.is_some_and(|until| Instant::now() < until);
        if flashing {
            let remaining = self.tty_power_flash_until
                .and_then(|until| until.checked_duration_since(Instant::now()))
                .map(|d| d.as_secs_f32())
                .unwrap_or(0.0);
            let phase = 2.0 - remaining;
            let grow = 1.0 + 0.06 * (phase * 8.0).sin().abs();
            selector = Rect::from_center_size(selector.center(), selector.size() * grow);
            ui.ctx().request_repaint_after(PANEL_FRAME);
        }

        if let Some(t) = &self.tex.tty_line_local { Self::image(ui, t, selector); }

        if response.is_pointer_button_down_on() {
            if let Some(pointer) = response.interact_pointer_pos() {
                if selector.contains(pointer) {
                    let xp = (pointer.x - selector.left()) / selector.width();
                    let yp = (pointer.y - selector.top()) / selector.height();
                    if yp < 0.52 {
                        self.set_tty_mode(TtyMode::Off);
                    } else if xp < 0.40 && yp > 0.40 && yp < 0.80 {
                        self.set_tty_mode(TtyMode::Line);
                    } else if xp > 0.56 && yp > 0.40 && yp < 0.80 {
                        self.set_tty_mode(TtyMode::Local);
                    }
                } else if self.pressed_key.is_none() {
                    let ix = (pointer.x - rect.left()) / scale;
                    let iy = (pointer.y - rect.top()) / scale;
                    if let Some(index) = teletype::KEYS.iter().position(|k| k.contains(ix, iy)) {
                        self.press_tty_key(index, ui.ctx());
                    }
                }
            }
        }

        let pointer_down = ui.ctx().input(|i| i.pointer.any_down());
        if !pointer_down && self.key_auto_release_at.is_none() {
            self.release_tty_key();
        }
        self.draw_pressed_key(ui, origin, scale);

        if !flashing {
            if let Some(t) = &self.tex.tty_knob {
                let knob_w = TTY_W * 0.06 * scale;
                let knob_h = knob_w * 117.0 / 116.0;
                let knob_rect = Rect::from_min_size(
                    Pos2::new(
                        rect.right() - TTY_W * 0.06 * scale - knob_w,
                        rect.bottom() - TTY_H * 0.022 * scale - knob_h,
                    ),
                    Vec2::new(knob_w, knob_h),
                );
                let target_angle = match self.tty.mode {
                    TtyMode::Line => -std::f32::consts::FRAC_PI_2,
                    TtyMode::Off => 0.0,
                    TtyMode::Local => std::f32::consts::FRAC_PI_2,
                };
                let angle = ui.ctx().animate_value_with_time(
                    egui::Id::new("asr33-selector-knob-angle"),
                    target_angle,
                    0.5,
                );
                egui::Image::new(t)
                    .rotate(angle, Vec2::splat(0.5))
                    .paint_at(ui, knob_rect);
            }
        }

        if !self.tty.tape_in.is_empty() || self.tty.capture_to_tape {
            let tape = Rect::from_min_size(
                Pos2::new(rect.left() + 18.0 * scale, rect.bottom() - 250.0 * scale),
                Vec2::new(520.0 * scale, 115.0 * scale),
            );
            ui.painter().rect_filled(tape, 3.0, Color32::from_rgb(224, 210, 160));
            let n = if self.tty.capture_to_tape { self.tty.tape_out.len() } else { self.tty.tape_in.len() };
            ui.painter().text(
                tape.center(),
                egui::Align2::CENTER_CENTER,
                if self.tty.capture_to_tape {
                    format!("PUNCHING PAPER TAPE  {n} bytes")
                } else {
                    format!("READING PAPER TAPE  {n} bytes")
                },
                FontId::monospace((22.0 * scale).max(8.0)),
                Color32::from_rgb(45, 42, 34),
            );
        }
    }

}

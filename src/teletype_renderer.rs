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

    fn print_head_lift(&self, now: Instant) -> f32 {
        let Some(until) = self.print_head_raise_until else { return 0.0; };
        let Some(remaining) = until.checked_duration_since(now) else { return 0.0; };

        let total = PRINT_HEAD_STRIKE_TIME.as_secs_f32().max(0.001);
        let elapsed = (1.0 - remaining.as_secs_f32() / total).clamp(0.0, 1.0);

        // The real mechanism snaps into the ribbon/paper quickly, then spends
        // most of the character interval dropping back below the glass.
        if elapsed < 0.24 {
            let t = (elapsed / 0.24).clamp(0.0, 1.0);
            1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
        } else {
            let t = ((elapsed - 0.24) / 0.76).clamp(0.0, 1.0);
            let smooth = t * t * (3.0 - 2.0 * t);
            1.0 - smooth
        }
    }

    fn draw_print_head(&self, ui: &mut egui::Ui, rect: Rect, origin: Pos2, scale: f32) {
        if self.tty.mode == TtyMode::Off { return; }
        let Some(head) = &self.tex.tty_head else { return; };

        let now = Instant::now();
        let lift = self.print_head_lift(now);
        let returning = self
            .print_head_carriage_return_until
            .is_some_and(|until| now < until);

        let char_width = self.tty.char_width_image_px();
        let active_column = self.tty.column.saturating_sub(1);
        let target_center_x = teletype::PRINT_LEFT
            + (active_column as f32 + 0.5) * char_width;

        // A normal character advances with a small mechanical slide.  A CR
        // deliberately takes longer, so the carriage visibly sweeps home.
        let travel_time = if returning {
            PRINT_HEAD_CARRIAGE_RETURN_TIME.as_secs_f32()
        } else {
            0.040
        };
        let center_x = ui.ctx().animate_value_with_time(
            egui::Id::new("asr33-typewheel-x"),
            target_center_x,
            travel_time,
        );

        // asr33head.png is 177x186.  The old renderer stretched it to an
        // arbitrary 6% x 10% rectangle; keep the real aspect ratio instead.
        let texture_size = head.size_vec2();
        let head_aspect = texture_size.y / texture_size.x.max(1.0);
        let head_width = TTY_W * 0.060 * (0.96 + 0.07 * lift);
        let head_height = head_width * head_aspect;

        // In repose the wheel sits down inside the mechanism and only its top
        // is visible through the window.  During a print strike it snaps up.
        const REST_TOP_RATIO: f32 = 0.355;
        const STRIKE_TOP_RATIO: f32 = 0.322;
        const GLASS_SILL_RATIO: f32 = 0.380;

        let rest_top = TTY_H * REST_TOP_RATIO;
        let strike_top = TTY_H * STRIKE_TOP_RATIO;
        let top_y = rest_top + (strike_top - rest_top) * lift;
        let sill_y = TTY_H * GLASS_SILL_RATIO;

        let head_rect = Rect::from_min_size(
            origin + Vec2::new((center_x - head_width * 0.5) * scale, top_y * scale),
            Vec2::new(head_width * scale, head_height * scale),
        );

        // Clipping is the depth cue that was missing before: the typewheel is
        // genuinely hidden by the photographic glass/body instead of merely
        // being moved on top of the complete teletype image.
        let glass_clip = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.right(), origin.y + sill_y * scale),
        );
        let shade = (225.0 + 30.0 * lift).round() as u8;
        ui.painter().with_clip_rect(glass_clip).image(
            head.id(),
            head_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_rgb(shade, shade, shade),
        );

        // Repaint a narrow strip of the original photograph over the wheel.
        // This uses the real glass/case pixels as a foreground layer and
        // removes the hard "sprite pasted on top" edge at the sill.
        if let Some(body) = &self.tex.tty_body {
            let lip_top = sill_y - TTY_H * 0.004;
            let lip_bottom = sill_y + TTY_H * 0.012;
            let lip_left = (center_x - head_width * 0.72).max(0.0);
            let lip_right = (center_x + head_width * 0.72).min(TTY_W);
            let lip_target = Rect::from_min_max(
                origin + Vec2::new(lip_left * scale, lip_top * scale),
                origin + Vec2::new(lip_right * scale, lip_bottom * scale),
            );
            let lip_source = Rect::from_min_max(
                Pos2::new(lip_left / TTY_W, lip_top / TTY_H),
                Pos2::new(lip_right / TTY_W, lip_bottom / TTY_H),
            );
            ui.painter().image(body.id(), lip_target, lip_source, Color32::WHITE);
        }

        if lift > 0.0 || returning {
            ui.ctx().request_repaint_after(Duration::from_millis(8));
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
        self.draw_print_head(ui, rect, origin, scale);

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

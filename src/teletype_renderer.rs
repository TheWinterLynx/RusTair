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

    fn paper_feed_offset(&self, now: Instant, line_height: f32) -> f32 {
        let Some(until) = self.paper_feed_until else { return 0.0; };
        let Some(remaining) = until.checked_duration_since(now) else { return 0.0; };
        let total = PAPER_FEED_TIME.as_secs_f32().max(0.001);
        line_height * (remaining.as_secs_f32() / total).clamp(0.0, 1.0)
    }

    fn ink_character_style(line: usize, column: usize, byte: u8) -> (f32, f32, u8) {
        // Deterministic micro-variation: the same piece of paper does not
        // shimmer between frames, but every impact is a little different.
        let mut hash = (line as u32).wrapping_mul(0x9e37_79b9)
            ^ (column as u32).wrapping_mul(0x85eb_ca6b)
            ^ (byte as u32).wrapping_mul(0xc2b2_ae35);
        hash ^= hash >> 16;
        hash = hash.wrapping_mul(0x7feb_352d);
        hash ^= hash >> 15;

        let x = ((hash & 0xff) as f32 / 255.0 - 0.5) * 0.65;
        let y = (((hash >> 8) & 0xff) as f32 / 255.0 - 0.5) * 0.90;
        let ink = 27 + ((hash >> 16) & 0x0f) as u8;
        (x, y, ink)
    }

    fn paper_line_height(&self, scale: f32) -> f32 {
        let char_width = self.tty.char_width_image_px();
        let font_size = (char_width * 1.63 * scale).max(5.0);
        (font_size * 1.03).max(6.0)
    }

    fn draw_virtual_paper(&self, ui: &mut egui::Ui, machine: Rect, origin: Pos2, scale: f32) {
        if self.tty.output.is_empty() { return; }

        // The paper is not a floating card. Its free end moves up with LF, but
        // the lower end continues *inside* the mechanism and is occluded later
        // by a photographic foreground strip from the platen/glass assembly.
        let line_height = self.paper_line_height(scale);
        let baseline_inset = TTY_H * 0.024 * scale;
        let print_baseline = origin.y + teletype::PRINT_TOP * scale - baseline_inset;
        let feed_offset = self.paper_feed_offset(Instant::now(), line_height);
        let line_count = self.tty.output.split('\n').count().max(1) as f32;

        // A shorter leader keeps the first few lines from looking like a large
        // rectangular card suddenly glued to the machine.
        const LEADER_LINES: f32 = 4.5;
        let sheet_top = (print_baseline
            - (line_count + LEADER_LINES) * line_height
            + feed_offset)
            .max(machine.top());

        // Deliberately extend the paper below the visible print station. The
        // lower edge is never meant to be seen; draw_paper_foreground() covers
        // it with the real mechanism pixels after ink has been rendered.
        let sheet_bottom = origin.y + (teletype::PRINT_TOP + TTY_H * 0.032) * scale;
        if sheet_top >= sheet_bottom { return; }

        let char_width = self.tty.char_width_image_px();
        let side_margin = char_width * 1.8 * scale;
        let bottom_left = origin.x + teletype::PRINT_LEFT * scale - side_margin;
        let bottom_right = origin.x
            + (teletype::PRINT_LEFT + teletype::PRINTABLE_WIDTH) * scale
            + side_margin;

        // Only the sheet geometry gets a tiny perspective cue; text remains
        // perfectly horizontal. The upper free edge is slightly narrower and
        // a fraction out of square, like lightly tensioned continuous paper.
        let taper = (char_width * 0.38 * scale).max(0.8 * scale);
        let top_left = bottom_left + taper;
        let top_right = bottom_right - taper;
        let top_left_y = sheet_top + 0.9 * scale;
        let top_right_y = sheet_top;

        let paper_points = vec![
            Pos2::new(top_left, top_left_y),
            Pos2::new(top_right, top_right_y),
            Pos2::new(bottom_right, sheet_bottom),
            Pos2::new(bottom_left, sheet_bottom),
        ];

        let painter = ui.painter().with_clip_rect(machine);

        // Near-opaque paper removes the horizontal machinery bands that were
        // bleeding through the old translucent rectangle. The warm-grey tone
        // is intentionally close to aged teletype stock rather than UI white.
        painter.add(egui::Shape::convex_polygon(
            paper_points,
            Color32::from_rgb(220, 218, 211),
            egui::Stroke::new(0.0, Color32::TRANSPARENT),
        ));

        // Very restrained illumination: one broad soft lift in the upper half
        // and a contact shadow only where the sheet approaches the platen. This
        // avoids the previous drop-shadow-around-a-card appearance.
        let inner_left = top_left.max(bottom_left) + 3.0 * scale;
        let inner_right = top_right.min(bottom_right) - 3.0 * scale;
        let highlight_top = sheet_top + line_height * 0.55;
        let highlight_bottom = (highlight_top + line_height * 1.4).min(sheet_bottom);
        if highlight_top < highlight_bottom && inner_left < inner_right {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(inner_left, highlight_top),
                    Pos2::new(inner_right, highlight_bottom),
                ),
                0.0,
                Color32::from_rgba_unmultiplied(255, 254, 247, 10),
            );
        }

        let contact_y = origin.y + (teletype::PRINT_TOP - TTY_H * 0.010) * scale;
        let contact_h = (TTY_H * 0.008 * scale).max(1.0);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(bottom_left + 2.0 * scale, contact_y - contact_h),
                Pos2::new(bottom_right - 2.0 * scale, contact_y),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(38, 34, 30, 24),
        );

        // The free edge gets only a hairline highlight/shadow. No shadow is
        // drawn around the whole sheet, so it reads as flexible paper instead
        // of a panel floating above the photograph.
        if sheet_top > machine.top() + 1.0 {
            painter.line_segment(
                [Pos2::new(top_left, top_left_y), Pos2::new(top_right, top_right_y)],
                egui::Stroke::new(
                    (0.65 * scale).max(0.45),
                    Color32::from_rgba_unmultiplied(255, 255, 248, 72),
                ),
            );
            painter.line_segment(
                [
                    Pos2::new(top_left, top_left_y + 1.2 * scale),
                    Pos2::new(top_right, top_right_y + 1.2 * scale),
                ],
                egui::Stroke::new(
                    (0.55 * scale).max(0.4),
                    Color32::from_rgba_unmultiplied(68, 62, 56, 28),
                ),
            );
        }

        // A tiny lower-side contact shadow suggests that the sheet disappears
        // into the machine without outlining the entire paper rectangle.
        let side_shadow_top = (contact_y - line_height * 1.1).max(sheet_top);
        painter.line_segment(
            [
                Pos2::new(bottom_left + 0.8 * scale, side_shadow_top),
                Pos2::new(bottom_left, contact_y),
            ],
            egui::Stroke::new(
                (0.8 * scale).max(0.5),
                Color32::from_rgba_unmultiplied(35, 31, 28, 24),
            ),
        );
        painter.line_segment(
            [
                Pos2::new(bottom_right - 0.8 * scale, side_shadow_top),
                Pos2::new(bottom_right, contact_y),
            ],
            egui::Stroke::new(
                (0.8 * scale).max(0.5),
                Color32::from_rgba_unmultiplied(35, 31, 28, 20),
            ),
        );
    }

    fn draw_paper_foreground(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        if self.tty.output.is_empty() { return; }
        let Some(body) = &self.tex.tty_body else { return; };

        // Put the real platen/glass pixels back in front of paper and ink. This
        // is the decisive depth cue: the sheet now physically disappears into
        // the ASR-33 instead of ending in a visible rectangular lower edge.
        let char_width = self.tty.char_width_image_px();
        let side_margin = char_width * 2.4;
        let left = (teletype::PRINT_LEFT - side_margin).max(0.0);
        let right = (teletype::PRINT_LEFT + teletype::PRINTABLE_WIDTH + side_margin)
            .min(TTY_W);
        let top = (teletype::PRINT_TOP - TTY_H * 0.008).max(0.0);
        let bottom = (teletype::PRINT_TOP + TTY_H * 0.046).min(TTY_H);

        let target = Rect::from_min_max(
            origin + Vec2::new(left * scale, top * scale),
            origin + Vec2::new(right * scale, bottom * scale),
        );
        let source = Rect::from_min_max(
            Pos2::new(left / TTY_W, top / TTY_H),
            Pos2::new(right / TTY_W, bottom / TTY_H),
        );
        ui.painter().image(body.id(), target, source, Color32::WHITE);
    }

    fn draw_paper_text(&self, ui: &mut egui::Ui, paper: Rect, scale: f32) {
        let char_width = self.tty.char_width_image_px();
        let char_cell = char_width * scale;
        let font_size = (char_width * 1.63 * scale).max(5.0);
        let line_height = (font_size * 1.03).max(6.0);

        // Keep the print baseline deliberately flat. The photographed window
        // seam has a little perspective, but bending the text to follow it is
        // much more noticeable than the seam itself. A small constant inset
        // keeps every character safely above the dark lower edge instead.
        let baseline_inset = TTY_H * 0.024 * scale;
        let print_baseline = paper.bottom() - baseline_inset;
        let printable_height = (paper.height() - baseline_inset).max(line_height);
        let max_lines = ((printable_height / line_height).floor() as usize).max(1);

        let now = Instant::now();
        let feed_offset = self.paper_feed_offset(now, line_height);
        let extra_line = usize::from(feed_offset > 0.01);

        let lines: Vec<&str> = self.tty.output.split('\n').collect();
        let first = lines.len().saturating_sub(max_lines + extra_line);
        let visible = &lines[first..];
        let painter = ui.painter().with_clip_rect(paper);
        let font = FontId::new(font_size, FontFamily::Name("teletype".into()));

        for (row, line) in visible.iter().enumerate() {
            let from_bottom = visible.len() - 1 - row;
            let baseline = print_baseline - from_bottom as f32 * line_height + feed_offset;
            let absolute_line = first + row;

            for (column, byte) in line.bytes().take(self.tty.paper_width).enumerate() {
                if byte == b' ' {
                    continue;
                }
                let (jitter_x, jitter_y, ink) =
                    Self::ink_character_style(absolute_line, column, byte);
                let blue = ink.saturating_sub(3);

                painter.text(
                    Pos2::new(
                        paper.left() + column as f32 * char_cell + jitter_x * scale,
                        baseline + jitter_y * scale,
                    ),
                    egui::Align2::LEFT_BOTTOM,
                    (byte as char).to_string(),
                    font.clone(),
                    Color32::from_rgb(ink, ink, blue),
                );
            }
        }
    }

    fn print_head_lift(&self, now: Instant) -> f32 {
        let Some(until) = self.print_head_raise_until else { return 0.0; };
        let Some(remaining) = until.checked_duration_since(now) else { return 0.0; };

        let total = PRINT_HEAD_STRIKE_TIME.as_secs_f32().max(0.001);
        let elapsed = (1.0 - remaining.as_secs_f32() / total).clamp(0.0, 1.0);

        // The selector settles during the first ~20 ms, the typewheel snaps
        // into the ribbon at the impact point, then drops back under the glass.
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
        let last_column = self.tty.paper_width.saturating_sub(1);
        let active_column = self.tty.column.saturating_sub(1).min(last_column);
        let target_center_x = teletype::PRINT_LEFT
            + (active_column as f32 + 0.5) * char_width;

        // A normal character advances with a small mechanical slide. A CR
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

        // Model 33 character selection is mechanical: 16 rotational positions
        // crossed with four vertical levels. Do not rotate the whole PNG in the
        // screen plane (that would look like the assembly is tilting). Instead
        // keep the lower mount fixed and make only the cylindrical upper wheel
        // visibly turn around its vertical axis: its face slides, narrows a
        // little and the highlight moves from one side to the other.
        let (slot, level) = teletype::typewheel_position(self.print_head_glyph);
        let slot_target = (slot as f32 - 7.5) / 7.5;
        let level_target = (level as f32 - 1.5) / 1.5;
        let slot_pose = ui.ctx().animate_value_with_time(
            egui::Id::new("asr33-typewheel-slot"),
            slot_target,
            0.034,
        );
        let level_pose = ui.ctx().animate_value_with_time(
            egui::Id::new("asr33-typewheel-level"),
            level_target,
            0.022,
        );

        // asr33head.png is 177x186. Keep the complete assembly at its real
        // aspect ratio; only the visible cylinder face is foreshortened below.
        let texture_size = head.size_vec2();
        let head_aspect = texture_size.y / texture_size.x.max(1.0);
        let head_width = TTY_W * 0.060 * (0.96 + 0.07 * lift);
        let head_height = head_width * head_aspect;

        // In repose the wheel sits down inside the mechanism and only its top
        // is visible through the window. During a print strike it snaps up.
        const REST_TOP_RATIO: f32 = 0.355;
        const STRIKE_TOP_RATIO: f32 = 0.322;
        const GLASS_SILL_RATIO: f32 = 0.380;

        let rest_top = TTY_H * REST_TOP_RATIO;
        let strike_top = TTY_H * STRIKE_TOP_RATIO;
        let selection_height = level_pose * TTY_H * 0.0042;
        let top_y = rest_top + (strike_top - rest_top) * lift + selection_height;
        let sill_y = TTY_H * GLASS_SILL_RATIO;

        let head_rect = Rect::from_min_size(
            origin + Vec2::new(
                (center_x - head_width * 0.5) * scale,
                top_y * scale,
            ),
            Vec2::new(head_width * scale, head_height * scale),
        );

        // Clipping is the depth cue that was missing before: the typewheel is
        // genuinely hidden by the photographic glass/body instead of merely
        // being moved on top of the complete teletype image.
        let glass_clip = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.right(), origin.y + sill_y * scale),
        );
        let clipped = ui.painter().with_clip_rect(glass_clip);

        // The bottom ~30% of the sprite is the stationary support/mount. Keep
        // it fixed while the actual type cylinder above it turns.
        const WHEEL_FRACTION: f32 = 0.70;
        let mount_source = Rect::from_min_max(
            Pos2::new(0.0, WHEEL_FRACTION),
            Pos2::new(1.0, 1.0),
        );
        let mount_target = Rect::from_min_max(
            Pos2::new(head_rect.left(), head_rect.top() + head_rect.height() * WHEEL_FRACTION),
            head_rect.max,
        );
        let base_shade = (226.0 + 29.0 * lift).round() as u8;
        clipped.image(
            head.id(),
            mount_target,
            mount_source,
            Color32::from_rgb(base_shade, base_shade, base_shade),
        );

        // Apparent axial rotation of the cylinder. Five to six percent
        // foreshortening is enough to read clearly at the size used by the UI,
        // while remaining much subtler than a fake 2-D sprite rotation.
        let wheel_width = head_width * (1.0 - 0.058 * slot_pose.abs());
        let wheel_center_x = center_x + slot_pose * head_width * 0.070;
        let wheel_target = Rect::from_min_size(
            origin + Vec2::new(
                (wheel_center_x - wheel_width * 0.5) * scale,
                top_y * scale,
            ),
            Vec2::new(wheel_width * scale, head_height * WHEEL_FRACTION * scale),
        );
        let wheel_mid_x = wheel_target.center().x;
        let left_target = Rect::from_min_max(
            wheel_target.min,
            Pos2::new(wheel_mid_x, wheel_target.bottom()),
        );
        let right_target = Rect::from_min_max(
            Pos2::new(wheel_mid_x, wheel_target.top()),
            wheel_target.max,
        );

        // Slide the photographed character face a few percent inside the
        // silhouette. Because the two halves use opposite illumination, the
        // eye reads this as a round metal cylinder turning rather than as a
        // flat bitmap moving sideways.
        let uv_shift = slot_pose * 0.035;
        let left_source = Rect::from_min_max(
            Pos2::new(0.035 + uv_shift, 0.0),
            Pos2::new(0.500 + uv_shift, WHEEL_FRACTION),
        );
        let right_source = Rect::from_min_max(
            Pos2::new(0.500 + uv_shift, 0.0),
            Pos2::new(0.965 + uv_shift, WHEEL_FRACTION),
        );

        let side_delta = (slot_pose * 24.0).round() as i16;
        let left_shade = (base_shade as i16 - side_delta).clamp(175, 255) as u8;
        let right_shade = (base_shade as i16 + side_delta).clamp(175, 255) as u8;
        clipped.image(
            head.id(),
            left_target,
            left_source,
            Color32::from_rgb(left_shade, left_shade, left_shade),
        );
        clipped.image(
            head.id(),
            right_target,
            right_source,
            Color32::from_rgb(right_shade, right_shade, right_shade),
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

        // The moving paper is a separate layer over the photographed machine
        // and under ink/typewheel. As LF advances, its free edge and all ink
        // travel together, so old lines can never end up printed "in the air".
        self.draw_virtual_paper(ui, rect, origin, scale);

        let paper = Rect::from_min_max(
            origin + Vec2::new(teletype::PRINT_LEFT * scale, 0.0),
            origin + Vec2::new(
                (teletype::PRINT_LEFT + teletype::PRINTABLE_WIDTH) * scale,
                teletype::PRINT_TOP * scale,
            ),
        );
        self.draw_paper_text(ui, paper, scale);

        // Restore the real front mechanism after paper + ink. The sheet now
        // appears to travel behind the platen/glass rather than ending on top.
        self.draw_paper_foreground(ui, origin, scale);
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

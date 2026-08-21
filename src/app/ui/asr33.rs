use super::super::*;

impl RusTairApp {
    fn teletype_key_legend(kind: KeyKind) -> String {
        match kind {
            KeyKind::Character(chars) => {
                let mut chars = chars.chars();
                let normal = chars.next().unwrap_or(' ');
                if let Some(shifted) = chars.next() {
                    format!("{shifted}\n{normal}")
                } else {
                    normal.to_string()
                }
            }
            KeyKind::Escape => "ESC".into(),
            KeyKind::LineFeed => "LINE\nFEED".into(),
            KeyKind::CarriageReturn => "RETURN".into(),
            KeyKind::Delete => "DELETE".into(),
            KeyKind::Repeat => "REPT".into(),
            KeyKind::Break => "BREAK".into(),
            KeyKind::HereIs => "HERE\nIS".into(),
            KeyKind::Space => String::new(),
            KeyKind::Control => "CTRL".into(),
            KeyKind::Shift => "SHIFT".into(),
        }
    }

    fn draw_key_pose(
        &self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        kind: KeyKind,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        pose: u8,
        legend_override: Option<&str>,
    ) {
        const KEY_CANVAS_WIDTH: f32 = 170.0;
        const SOCKET_BASE_OFFSET: f32 = 50.0;

        let texture = if pose == 0 {
            &self.tex.tty_key_up
        } else {
            &self.tex.tty_key_mid
        };
        let Some(texture) = texture else { return; };

        let (socket_x, socket_y) = (x + w * 0.5, y + h * 0.5);
        let texture_size = texture.size_vec2();
        let texture_aspect = texture_size.y / texture_size.x.max(1.0);

        let special = legend_override.is_some()
            || matches!(
                kind,
                KeyKind::Escape
                    | KeyKind::LineFeed
                    | KeyKind::CarriageReturn
                    | KeyKind::Delete
                    | KeyKind::Repeat
                    | KeyKind::Break
                    | KeyKind::HereIs
                    | KeyKind::Control
                    | KeyKind::Shift
            );
        let size_factor = if matches!(kind, KeyKind::Control | KeyKind::Shift) {
            1.10
        } else if special {
            1.05
        } else {
            1.0
        };
        let target_w = KEY_CANVAS_WIDTH * size_factor;
        let target_h = target_w * texture_aspect;

        let base_y = socket_y + SOCKET_BASE_OFFSET;
        let target = Rect::from_min_size(
            origin
                + Vec2::new(
                    (socket_x - target_w * 0.5) * scale,
                    (base_y - target_h) * scale,
                ),
            Vec2::new(target_w * scale, target_h * scale),
        );

        ui.painter().image(
            texture.id(),
            target,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        let legend = legend_override
            .map(str::to_owned)
            .unwrap_or_else(|| Self::teletype_key_legend(kind));
        if legend.is_empty() { return; }

        let multiline = legend.contains('\n');
        let special_y_nudge = if special { -2.0 } else { 0.0 };
        let text_color = Color32::from_rgb(47, 44, 38);

        if multiline {
            let face_ratio = if pose == 0 { 0.215 } else { 0.285 };
            let face_y = base_y - target_h * (1.0 - face_ratio) + special_y_nudge;
            let font_factor = if special { 0.115 } else { 0.130 };
            let font_size = (114.0 * font_factor * scale).max(5.0);
            let line_gap = font_size * if special { 1.35 } else { 1.45 };
            let legend_x = origin.x + socket_x * scale;
            let face_screen_y = origin.y + face_y * scale;
            let mut lines = legend.lines();
            let upper = lines.next().unwrap_or("");
            let lower = lines.next().unwrap_or("");

            ui.painter().text(
                Pos2::new(legend_x, face_screen_y - line_gap * 0.5),
                egui::Align2::CENTER_CENTER,
                upper,
                FontId::monospace(font_size),
                text_color,
            );
            ui.painter().text(
                Pos2::new(legend_x, face_screen_y + line_gap * 0.5),
                egui::Align2::CENTER_CENTER,
                lower,
                FontId::monospace(font_size),
                text_color,
            );
        } else {
            let face_ratio = if pose == 0 { 0.245 } else { 0.315 };
            let legend_y = base_y - target_h * (1.0 - face_ratio) + special_y_nudge;
            let compact = legend.len() > 4;
            let font_factor = if compact { 0.145 } else { 0.225 };
            let font_size = (114.0 * font_factor * scale).max(5.0);
            ui.painter().text(
                origin + Vec2::new(socket_x * scale, legend_y * scale),
                egui::Align2::CENTER_CENTER,
                legend,
                FontId::monospace(font_size),
                text_color,
            );
        }
    }

    fn draw_spacebar_pose(
        &self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        w: f32,
        _h: f32,
        pose: u8,
    ) {
        const SPACEBAR_VISUAL_SCALE: f32 = 1.045;
        const SPACEBAR_BASE_OFFSET: f32 = 55.0;

        let texture = if pose == 0 {
            &self.tex.tty_spacebar_up
        } else {
            &self.tex.tty_spacebar_mid
        };
        let Some(texture) = texture else { return; };

        let socket_x = x + w * 0.5;
        let socket_y = y + _h * 0.5;
        let texture_size = texture.size_vec2();
        let texture_aspect = texture_size.y / texture_size.x.max(1.0);
        let target_w = w * SPACEBAR_VISUAL_SCALE;
        let target_h = target_w * texture_aspect;
        let base_y = socket_y + SPACEBAR_BASE_OFFSET;

        let target = Rect::from_min_size(
            origin
                + Vec2::new(
                    (socket_x - target_w * 0.5) * scale,
                    (base_y - target_h) * scale,
                ),
            Vec2::new(target_w * scale, target_h * scale),
        );
        ui.painter().image(
            texture.id(),
            target,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    fn draw_pressed_key(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        let pc_modifiers = ui.ctx().input(|input| input.modifiers);

        for (index, key) in teletype::KEYS.iter().copied().enumerate() {
            let pc_modifier_down = match key.kind {
                KeyKind::Shift => pc_modifiers.shift,
                KeyKind::Control => pc_modifiers.ctrl,
                _ => false,
            };
            let pose = u8::from(
                pc_modifier_down
                    || (self.asr33.keyboard.animated_key == Some(index)
                        && self.asr33.keyboard.displacement > 0.0),
            );

            if matches!(key.kind, KeyKind::Space) {
                self.draw_spacebar_pose(
                    ui,
                    origin,
                    scale,
                    key.x,
                    key.y,
                    key.w,
                    key.h,
                    pose,
                );
            } else {
                self.draw_key_pose(
                    ui,
                    origin,
                    scale,
                    key.kind,
                    key.x,
                    key.y,
                    key.w,
                    key.h,
                    pose,
                    None,
                );
            }
        }
    }

    fn paper_feed_offset(&self, now: Instant, line_height: f32) -> f32 {
        let Some(until) = self.asr33.mechanics.paper_feed_until else { return 0.0; };
        let Some(remaining) = until.checked_duration_since(now) else { return 0.0; };
        let total = PAPER_FEED_TIME.as_secs_f32().max(0.001);
        line_height * (remaining.as_secs_f32() / total).clamp(0.0, 1.0)
    }

    fn ink_character_style(line: usize, column: usize, byte: u8) -> (f32, f32, u8) {
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
        let glyph_width = self.tty.char_width_image_px();
        let font_size = (glyph_width * 1.63 * scale).max(5.0);
        (font_size * 1.03).max(6.0)
    }

    fn paper_char_pitch_image_px(&self) -> f32 {
        const CHAR_PITCH_SCALE: f32 = 1.10;
        self.tty.char_width_image_px() * CHAR_PITCH_SCALE
    }

    fn paper_printable_width(&self) -> f32 {
        self.paper_char_pitch_image_px() * self.tty.paper_width.max(1) as f32
    }

    fn paper_emergence_y(&self) -> f32 {
        const GLASS_SILL_RATIO: f32 = 0.380;
        TTY_H * GLASS_SILL_RATIO
    }

    fn paper_view_offset_lines(&self, ui: &egui::Ui) -> f32 {
        ui.ctx().data(|data| {
            data.get_temp::<f32>(egui::Id::new("asr33-paper-view-offset-lines"))
                .unwrap_or(0.0)
        })
    }

    fn set_paper_view_offset_lines(&self, ui: &egui::Ui, value: f32) {
        let max = self.max_paper_rewind_lines();
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("asr33-paper-view-offset-lines"),
                value.clamp(0.0, max),
            );
        });
    }

    fn max_paper_rewind_lines(&self) -> f32 {
        self.tty.output.split('\n').count().saturating_sub(1) as f32
    }

    fn paper_roller_rect(&self, origin: Pos2, scale: f32) -> Rect {
        Rect::from_min_max(
            origin + Vec2::new(0.0, 650.0 * scale),
            origin + Vec2::new(310.0 * scale, 1050.0 * scale),
        )
    }

    fn reset_paper_view_for_printing(&self, ui: &egui::Ui) {
        if self.asr33.mechanics.printing_active() {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("asr33-paper-view-offset-lines"), 0.0_f32);
                data.insert_temp(egui::Id::new("asr33-paper-roller-dragging"), false);
            });
        }
    }

    fn update_paper_roller_interaction(
        &self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        origin: Pos2,
        scale: f32,
    ) {
        if self.tty.output.is_empty() { return; }

        let roller = self.paper_roller_rect(origin, scale);
        let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
        if pointer.is_some_and(|p| roller.contains(p)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }

        let drag_id = egui::Id::new("asr33-paper-roller-dragging");
        let dragging = ui.ctx().data(|data| data.get_temp::<bool>(drag_id).unwrap_or(false));

        if response.drag_started() && pointer.is_some_and(|p| roller.contains(p)) {
            ui.ctx().data_mut(|data| data.insert_temp(drag_id, true));
        }

        let pointer_down = ui.ctx().input(|i| i.pointer.primary_down());
        let dragging = ui.ctx().data(|data| data.get_temp::<bool>(drag_id).unwrap_or(false));
        if dragging && pointer_down {
            let delta_y = ui.ctx().input(|i| i.pointer.delta().y);
            if delta_y.abs() > f32::EPSILON {
                let line_height = self.paper_line_height(scale).max(1.0);
                let current = self.paper_view_offset_lines(ui);
                let next = current + delta_y / line_height;
                self.set_paper_view_offset_lines(ui, next);
                ui.ctx().request_repaint();
            }
        } else if dragging {
            ui.ctx().data_mut(|data| data.insert_temp(drag_id, false));
        }
    }

    fn draw_virtual_paper(&self, ui: &mut egui::Ui, machine: Rect, origin: Pos2, scale: f32) {
        if self.tty.output.is_empty() { return; }

        let line_height = self.paper_line_height(scale);
        let baseline_inset = TTY_H * 0.024 * scale;
        let print_baseline = origin.y + teletype::PRINT_TOP * scale - baseline_inset;
        let feed_offset = self.paper_feed_offset(Instant::now(), line_height);
        let line_count = self.tty.output.split('\n').count().max(1) as f32;
        let rewind_offset = self.paper_view_offset_lines(ui).min(self.max_paper_rewind_lines())
            * line_height;

        const LEADER_LINES: f32 = 2.0;
        let sheet_top = print_baseline
            - (line_count + LEADER_LINES) * line_height
            + feed_offset
            + rewind_offset;

        let emergence_y = self.paper_emergence_y();
        const PAPER_OVERLAP: f32 = 3.0;
        let sheet_bottom = origin.y + (emergence_y + PAPER_OVERLAP) * scale;
        if sheet_top >= sheet_bottom { return; }

        let char_pitch = self.paper_char_pitch_image_px();
        let side_margin = char_pitch * 1.8 * scale;
        let bottom_left = origin.x + teletype::PRINT_LEFT * scale - side_margin;
        let bottom_right = origin.x
            + (teletype::PRINT_LEFT + self.paper_printable_width()) * scale
            + side_margin;

        let taper = (char_pitch * 0.38 * scale).max(0.8 * scale);
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
        painter.add(egui::Shape::convex_polygon(
            paper_points,
            Color32::from_rgb(220, 218, 211),
            egui::Stroke::new(0.0_f32, Color32::TRANSPARENT),
        ));

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

        let contact_y = origin.y + emergence_y * scale;
        let contact_h = (3.0 * scale).max(0.6);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(bottom_left + 2.0 * scale, contact_y - contact_h),
                Pos2::new(bottom_right - 2.0 * scale, contact_y),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(38, 34, 30, 16),
        );

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

        let char_pitch = self.paper_char_pitch_image_px();
        let side_margin = char_pitch * 2.4;
        let left = (teletype::PRINT_LEFT - side_margin).max(0.0);
        let right = (teletype::PRINT_LEFT + self.paper_printable_width() + side_margin)
            .min(TTY_W);

        let top = self.paper_emergence_y().clamp(0.0, TTY_H);
        let bottom = (top + TTY_H * 0.025).min(TTY_H);

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
        let glyph_width = self.tty.char_width_image_px();
        let char_cell = self.paper_char_pitch_image_px() * scale;
        let font_size = (glyph_width * 1.63 * scale).max(5.0);
        let line_height = (font_size * 1.03).max(6.0);

        let baseline_inset = TTY_H * 0.024 * scale;
        let print_baseline = paper.bottom() - baseline_inset;
        let printable_height = (paper.height() - baseline_inset).max(line_height);
        let max_lines = ((printable_height / line_height).floor() as usize).max(1);

        let now = Instant::now();
        let feed_offset = self.paper_feed_offset(now, line_height);
        let extra_line = usize::from(feed_offset > 0.01);
        let rewind_lines = self.paper_view_offset_lines(ui).min(self.max_paper_rewind_lines());
        let rewind_extra = rewind_lines.ceil() as usize;

        let lines: Vec<&str> = self.tty.output.split('\n').collect();
        let first = lines
            .len()
            .saturating_sub(max_lines + extra_line + rewind_extra);
        let visible = &lines[first..];
        let painter = ui.painter().with_clip_rect(paper);
        let font = FontId::new(font_size, FontFamily::Name("teletype".into()));
        let rewind_offset = rewind_lines * line_height;

        for (row, line) in visible.iter().enumerate() {
            let from_bottom = visible.len() - 1 - row;
            let baseline = print_baseline
                - from_bottom as f32 * line_height
                + feed_offset
                + rewind_offset;
            let absolute_line = first + row;

            for (column, byte) in line.bytes().take(self.tty.paper_width).enumerate() {
                if byte == b' ' {
                    continue;
                }
                let (jitter_x, jitter_y, ink) = Self::ink_character_style(absolute_line, column, byte);
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
        let Some(until) = self.asr33.mechanics.print_head_raise_until else { return 0.0; };
        let Some(remaining) = until.checked_duration_since(now) else { return 0.0; };
        let total = PRINT_HEAD_STRIKE_TIME.as_secs_f32().max(0.001);
        let elapsed = (1.0 - remaining.as_secs_f32() / total).clamp(0.0, 1.0);

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
            .asr33
            .mechanics
            .print_head_carriage_return_until
            .is_some_and(|until| now < until);

        let char_pitch = self.paper_char_pitch_image_px();
        let last_column = self.tty.paper_width.saturating_sub(1);
        let active_column = self.tty.column.saturating_sub(1).min(last_column);
        let target_center_x = teletype::PRINT_LEFT + (active_column as f32 + 0.5) * char_pitch;

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

        let (slot, level) = teletype::typewheel_position(self.asr33.mechanics.print_head_glyph);
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

        let texture_size = head.size_vec2();
        let head_aspect = texture_size.y / texture_size.x.max(1.0);
        let head_width = TTY_W * 0.060 * (0.96 + 0.07 * lift);
        let head_height = head_width * head_aspect;

        const REST_TOP_RATIO: f32 = 0.355;
        const STRIKE_TOP_RATIO: f32 = 0.322;

        let rest_top = TTY_H * REST_TOP_RATIO;
        let strike_top = TTY_H * STRIKE_TOP_RATIO;
        let selection_height = level_pose * TTY_H * 0.0042;
        let top_y = rest_top + (strike_top - rest_top) * lift + selection_height;
        let sill_y = self.paper_emergence_y();

        let head_rect = Rect::from_min_size(
            origin + Vec2::new(
                (center_x - head_width * 0.5) * scale,
                top_y * scale,
            ),
            Vec2::new(head_width * scale, head_height * scale),
        );

        let glass_clip = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.right(), origin.y + sill_y * scale),
        );
        let clipped = ui.painter().with_clip_rect(glass_clip);

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

        if lift > 0.0 || returning {
            ui.ctx().request_repaint_after(Duration::from_millis(8));
        }
    }

    pub(in crate::app) fn draw_teletype(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let scale = (available.x / TTY_W).min(available.y / TTY_H).clamp(0.12, 1.5);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(TTY_W * scale, TTY_H * scale),
            Sense::click_and_drag(),
        );
        let origin = rect.min;

        self.reset_paper_view_for_printing(ui);
        self.update_paper_roller_interaction(ui, &response, origin, scale);

        if let Some(t) = &self.tex.tty_body { Self::image(ui, t, rect); }
        if let Some(t) = &self.tex.tty_keys { Self::image(ui, t, rect); }

        self.draw_virtual_paper(ui, rect, origin, scale);

        let paper = Rect::from_min_max(
            origin + Vec2::new(teletype::PRINT_LEFT * scale, 0.0),
            origin + Vec2::new(
                (teletype::PRINT_LEFT + self.paper_printable_width()) * scale,
                teletype::PRINT_TOP * scale,
            ),
        );
        self.draw_paper_text(ui, paper, scale);
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

        let flashing = self
            .asr33
            .power_flash_until
            .is_some_and(|until| Instant::now() < until);
        if flashing {
            let remaining = self
                .asr33
                .power_flash_until
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
                } else if self.asr33.keyboard.pressed_key.is_none() {
                    let ix = (pointer.x - rect.left()) / scale;
                    let iy = (pointer.y - rect.top()) / scale;
                    if let Some(index) = teletype::KEYS.iter().position(|k| k.contains(ix, iy)) {
                        self.press_tty_key(index, ui.ctx());
                    }
                }
            }
        }

        let pointer_down = ui.ctx().input(|i| i.pointer.any_down());
        if !pointer_down && self.asr33.keyboard.auto_release_at.is_none() {
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

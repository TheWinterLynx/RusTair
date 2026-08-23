use super::super::*;
use super::front_panel_assets::SwitchSpriteId;
use super::front_panel_switches::*;

const MOMENTARY_LATCH_HOLD: Duration = Duration::from_secs(3);

impl RusTairApp {
    fn draw_led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, intensity: f32) {
        if !self.machine.powered { return; }
        let intensity = intensity.clamp(0.0, 1.0).sqrt();
        if intensity < 0.015 { return; }
        let alpha = (255.0 * intensity).round() as u8;
        let center = origin + Vec2::new(x * scale, y * scale);
        ui.painter().circle_filled(center, 10.5 * scale, Color32::from_rgba_unmultiplied(255, 24, 42, alpha));
        ui.painter().circle_filled(center, 5.8 * scale, Color32::from_rgba_unmultiplied(255, 104, 116, alpha));
        ui.painter().circle_filled(center + Vec2::new(-2.8 * scale, -3.0 * scale), 2.0 * scale, Color32::from_rgba_unmultiplied(255, 255, 255, alpha));
    }

    fn switch_texture(&self, sprite: SwitchSpriteId) -> Option<&egui::TextureHandle> {
        self.tex.switch_sprites.get(sprite.asset().path)
    }

    fn draw_switch_sprite(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, switch: SwitchConfig, position: SwitchPosition) {
        let Some(pose) = switch.pose(position) else { return; };
        let asset = pose.sprite.asset();
        let Some(texture) = self.switch_texture(pose.sprite) else { return; };
        let crop_min = Vec2::new(asset.crop_min.0, asset.crop_min.1);
        let crop_max = Vec2::new(asset.crop_max.0, asset.crop_max.1);
        let pivot_px = Vec2::new(asset.pivot.0, asset.pivot.1);
        let crop_size = crop_max - crop_min;
        let pivot_in_crop = pivot_px - crop_min;
        let socket = origin + Vec2::new((switch.socket.0 + pose.offset.0) * scale, (switch.socket.1 + pose.offset.1) * scale);
        let source_to_screen = asset.source_to_panel * pose.scale * scale;
        let rect = Rect::from_min_size(socket - pivot_in_crop * source_to_screen, crop_size * source_to_screen);
        let uv = Rect::from_min_max(
            Pos2::new(crop_min.x / asset.canvas_size.0, crop_min.y / asset.canvas_size.1),
            Pos2::new(crop_max.x / asset.canvas_size.0, crop_max.y / asset.canvas_size.1),
        );
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    }

    fn sense_switch(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32, bit: usize) {
        let switch = SENSE_SWITCHES[bit];
        debug_assert_eq!(switch.kind, SwitchKind::TwoPosition);
        let hit = Self::centered_rect(origin, scale, switch.socket.0, switch.socket.1, switch.hit_size.0, switch.hit_size.1);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() {
            self.machine.toggle_sense_switch(bit);
            self.audio.play_once("assets/click.mp3");
        }
        if response.hovered() { response.clone().on_hover_text(format!("Sense switch {}", switch.name)); }
        let position = if self.machine.panel_switches() & (1u16 << bit) != 0 { SwitchPosition::Up } else { SwitchPosition::Down };
        self.draw_switch_sprite(ui, origin, scale, switch, position);
    }

    fn momentary_switch(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32, switch: SwitchConfig, label: &str) -> Option<bool> {
        debug_assert_eq!(switch.kind, SwitchKind::ThreePosition);
        debug_assert!(switch.center.is_some());
        let hit = Self::centered_rect(origin, scale, switch.socket.0, switch.socket.1, switch.hit_size.0, switch.hit_size.1);
        let response = ui.allocate_rect(hit, Sense::click_and_drag());
        if response.hovered() {
            response.clone().on_hover_text(format!("{label}\nHold for 3 seconds to keep the switch actuated; click it again to release."));
        }

        let now = Instant::now();
        let (primary_down, primary_pressed, primary_released, pointer_pos) = ui.ctx().input(|input| {
            (input.pointer.primary_down(), input.pointer.primary_pressed(), input.pointer.primary_released(), input.pointer.interact_pos())
        });
        let pointer_inside = pointer_pos.is_some_and(|p| hit.contains(p));
        let pointer_position = if pointer_pos.map(|p| p.y >= origin.y + switch.socket.1 * scale).unwrap_or(false) {
            SwitchPosition::Down
        } else {
            SwitchPosition::Up
        };
        let state_id = egui::Id::new(("rustair-momentary-switch", switch.name));

        let (position, action, just_latched, released_latch, tracking_press) = ui.ctx().data_mut(|data| {
            let state = data.get_temp_mut_or(state_id, MomentarySwitchUiState::default());
            let mut action = None;
            let mut just_latched = false;
            let mut released_latch = false;

            if primary_pressed && pointer_inside && state.press_started.is_none() {
                state.press_started = Some(now);
                state.press_direction = Some(pointer_position);
                state.press_began_on_latched = state.latched.is_some();
                state.long_latched_this_press = false;
            }

            if state.press_started.is_some() && primary_down
                && !state.press_began_on_latched
                && state.latched.is_none()
                && !state.long_latched_this_press
                && state.press_started.is_some_and(|started| now.duration_since(started) >= MOMENTARY_LATCH_HOLD)
            {
                let direction = state.press_direction.unwrap_or(pointer_position);
                state.latched = Some(direction);
                state.long_latched_this_press = true;
                action = Some(direction == SwitchPosition::Down);
                just_latched = true;
            }

            if state.press_started.is_some() && primary_released {
                if state.press_began_on_latched {
                    state.latched = None;
                    released_latch = true;
                } else if !state.long_latched_this_press {
                    let direction = state.press_direction.unwrap_or(pointer_position);
                    action = Some(direction == SwitchPosition::Down);
                }
                state.press_started = None;
                state.press_direction = None;
                state.press_began_on_latched = false;
                state.long_latched_this_press = false;
            } else if state.press_started.is_some() && !primary_down && !primary_released {
                state.press_started = None;
                state.press_direction = None;
                state.press_began_on_latched = false;
                state.long_latched_this_press = false;
            }

            let tracking_press = state.press_started.is_some() && primary_down;
            let position = if tracking_press {
                if state.press_began_on_latched {
                    state.latched.unwrap_or(SwitchPosition::Center)
                } else {
                    state.latched.or(state.press_direction).unwrap_or(SwitchPosition::Center)
                }
            } else {
                state.latched.unwrap_or(SwitchPosition::Center)
            };
            (position, action, just_latched, released_latch, tracking_press)
        });

        self.draw_switch_sprite(ui, origin, scale, switch, position);
        if tracking_press { ui.ctx().request_repaint_after(Duration::from_millis(8)); }

        if let Some(down) = action {
            self.audio.play_once("assets/click.mp3");
            if just_latched {
                self.status = format!("{label} held {} — click the switch to release it", if down { "DOWN" } else { "UP" });
            }
            Some(down)
        } else {
            if released_latch {
                self.audio.play_once("assets/click.mp3");
                self.status = format!("{label} released to center");
            }
            None
        }
    }

    fn draw_power(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        let switch = SWITCH_POWER;
        let hit = Self::centered_rect(origin, scale, switch.socket.0, switch.socket.1, switch.hit_size.0, switch.hit_size.1);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() { self.set_altair_power(!self.machine.powered); }
        if response.hovered() { response.clone().on_hover_text("OFF / ON"); }
        let position = if self.machine.powered { SwitchPosition::Down } else { SwitchPosition::Up };
        self.draw_switch_sprite(ui, origin, scale, switch, position);
    }

    pub(in crate::app) fn set_altair_power(&mut self, on: bool) {
        self.machine.power(on);
        self.asr33.tx_started = None;
        self.audio.play_once("assets/powerbtn.mp3");
        if on {
            self.status = "Power on — original Altair 8800 requires STOP + RESET before RUN".into();
            self.audio.start_loop("altair-fan", "assets/fan.mp3");
        } else {
            self.audio.stop_loop("altair-fan");
        }
    }

    pub(in crate::app) fn draw_altair(&mut self, ui: &mut egui::Ui) {
        self.machine.commit_panel_activity(PANEL_FRAME);
        let lamps = self.machine.panel_lamps();

        let available = ui.available_size();
        let scale = (available.x / PANEL_W).min(available.y / PANEL_H).clamp(0.2, 2.5);
        let (whole, _) = ui.allocate_exact_size(Vec2::new(PANEL_W * scale, PANEL_H * scale), Sense::hover());
        let origin = whole.min;
        if let Some(t) = &self.tex.panel { Self::image(ui, t, whole); }
        else { ui.painter().rect_filled(whole, 0.0, Color32::from_rgb(20, 25, 28)); }

        for bit in 0..16 { self.sense_switch(ui, origin, scale, bit); }
        for bit in 0..16 { self.draw_led(ui, origin, scale, ADDR_LED_X[bit], ADDR_LED_Y, lamps.address[bit]); }
        for bit in 0..8 { self.draw_led(ui, origin, scale, DATA_LED_X[bit], DATA_LED_Y, lamps.data[bit]); }

        // Every lamp below is a passive consumer of the emulated S-100 state.
        self.draw_led(ui, origin, scale, STATUS_LED_X[0], STATUS_LED_Y, lamps.inte);
        self.draw_led(ui, origin, scale, STATUS_LED_X[1], STATUS_LED_Y, lamps.prot);
        self.draw_led(ui, origin, scale, STATUS_LED_X[2], STATUS_LED_Y, lamps.memr);
        self.draw_led(ui, origin, scale, STATUS_LED_X[3], STATUS_LED_Y, lamps.inp);
        self.draw_led(ui, origin, scale, STATUS_LED_X[4], STATUS_LED_Y, lamps.m1);
        self.draw_led(ui, origin, scale, STATUS_LED_X[5], STATUS_LED_Y, lamps.out);
        self.draw_led(ui, origin, scale, STATUS_LED_X[6], STATUS_LED_Y, lamps.hlta);
        self.draw_led(ui, origin, scale, STATUS_LED_X[7], STATUS_LED_Y, lamps.stack);
        self.draw_led(ui, origin, scale, STATUS_LED_X[8], STATUS_LED_Y, lamps.wo);
        self.draw_led(ui, origin, scale, STATUS_LED_X[9], STATUS_LED_Y, lamps.int_ack);
        self.draw_led(ui, origin, scale, WAIT_LED.0, WAIT_LED.1, lamps.wait);
        self.draw_led(ui, origin, scale, HLDA_LED.0, HLDA_LED.1, lamps.hlda);

        self.draw_power(ui, origin, scale);
        if let Some(run) = self.momentary_switch(ui, origin, scale, SWITCH_RUN_STOP, "STOP / RUN") { self.machine.set_running(run); }
        if self.momentary_switch(ui, origin, scale, SWITCH_SINGLE_STEP, "SINGLE STEP").is_some() { self.machine.step(); }
        if let Some(next) = self.momentary_switch(ui, origin, scale, SWITCH_EXAMINE, "EXAMINE / EXAMINE NEXT") { self.machine.examine(next); }
        if let Some(next) = self.momentary_switch(ui, origin, scale, SWITCH_DEPOSIT, "DEPOSIT / DEPOSIT NEXT") { self.machine.deposit(next); }
        if let Some(clear) = self.momentary_switch(ui, origin, scale, SWITCH_RESET, "RESET / CLR") {
            if clear {
                self.machine.clear_io();
                self.asr33.tx_started = None;
                self.terminal.tx_started = None;
                self.external_serial.reset_line_timing();
                self.external_com.reset_line_timing();
                self.status = "CLR asserted: external/emulated I/O cleared; CPU state preserved".into();
            } else {
                self.machine.front_panel_reset();
                self.status = "RESET asserted: PC returned to 0000h and machine stopped".into();
            }
        }
        if let Some(unprotect) = self.momentary_switch(ui, origin, scale, SWITCH_PROTECT, "PROTECT / UNPROTECT") {
            self.machine.protect_current_board(!unprotect);
        }
        let _ = self.momentary_switch(ui, origin, scale, SWITCH_AUX1, "AUX 1 (unassigned)");
        let _ = self.momentary_switch(ui, origin, scale, SWITCH_AUX2, "AUX 2 (unassigned)");
    }
}

use super::super::*;
use super::front_panel_assets::SwitchSpriteId;
use super::front_panel_switches::*;

const MOMENTARY_LATCH_HOLD: Duration = Duration::from_secs(3);
const LED_VISIBLE_THRESHOLD: f32 = 0.01;
const LED_HALO_MAX_ALPHA: u8 = 72;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LedVisualResponse {
    halo_alpha: u8,
    body_alpha: u8,
    core_alpha: u8,
    glare_alpha: u8,
}

fn optical_alpha(max_alpha: u8, response: f32) -> u8 {
    (f32::from(max_alpha) * response.clamp(0.0, 1.0)).round() as u8
}

/// Convert the panel integrator's electrical duty cycle into a visual LED
/// response. This is deliberately presentation-only: CPU/S-100 activity stays
/// untouched. The old sqrt curve made low-duty LEDs look too similar to LEDs
/// that were active most of the time. A shallower body response preserves a
/// visible red lens while the core and especially the specular glare require
/// progressively more real activity, which better matches physical red LEDs
/// photographed through the Altair's front panel.
fn led_visual_response(intensity: f32) -> Option<LedVisualResponse> {
    let electrical = intensity.clamp(0.0, 1.0);
    if electrical < LED_VISIBLE_THRESHOLD {
        return None;
    }

    Some(LedVisualResponse {
        // A diffuse halo should only become obvious on strongly driven LEDs.
        halo_alpha: optical_alpha(LED_HALO_MAX_ALPHA, electrical.powf(1.35)),
        // Keep the red lens visible at moderate duty cycle without the old
        // sqrt() compression that over-promoted very weak bus activity.
        body_alpha: optical_alpha(255, electrical.powf(0.80)),
        // The luminous core tracks real duty cycle much more directly.
        core_alpha: optical_alpha(255, electrical),
        // The white hot-spot is a high-intensity optical/camera effect.
        glare_alpha: optical_alpha(255, electrical.powf(1.80)),
    })
}

#[derive(Default)]
struct MomentarySwitchInteraction {
    action: Option<bool>,
    pressed: Option<bool>,
    released: Option<bool>,
}

impl RusTairApp {
    fn draw_led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, intensity: f32, powered: bool) {
        if !powered { return; }
        let Some(light) = led_visual_response(intensity) else { return; };
        let center = origin + Vec2::new(x * scale, y * scale);

        // The unlit LED/lens is part of the panel texture. These overlays model
        // only emitted light: broad halo, red body, bright core and the small
        // camera/eye specular highlight. Each responds differently to duty cycle
        // instead of treating a weak LED as a transparent copy of a strong one.
        if light.halo_alpha > 0 {
            ui.painter().circle_filled(
                center,
                14.5 * scale,
                Color32::from_rgba_unmultiplied(255, 12, 30, light.halo_alpha),
            );
        }
        ui.painter().circle_filled(
            center,
            10.5 * scale,
            Color32::from_rgba_unmultiplied(255, 24, 42, light.body_alpha),
        );
        ui.painter().circle_filled(
            center,
            5.8 * scale,
            Color32::from_rgba_unmultiplied(255, 104, 116, light.core_alpha),
        );
        if light.glare_alpha > 0 {
            ui.painter().circle_filled(
                center + Vec2::new(-2.8 * scale, -3.0 * scale),
                2.0 * scale,
                Color32::from_rgba_unmultiplied(255, 255, 255, light.glare_alpha),
            );
        }
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
        let position = if self.machine.switch_register() & (1u16 << bit) != 0 { SwitchPosition::Up } else { SwitchPosition::Down };
        self.draw_switch_sprite(ui, origin, scale, switch, position);
    }

    fn momentary_switch(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        switch: SwitchConfig,
        label: &str,
    ) -> MomentarySwitchInteraction {
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

        let (position, action, pressed, released, just_latched, released_latch, tracking_press) = ui.ctx().data_mut(|data| {
            let state = data.get_temp_mut_or(state_id, MomentarySwitchUiState::default());
            let mut action = None;
            let mut pressed = None;
            let mut released = None;
            let mut just_latched = false;
            let mut released_latch = false;

            if primary_pressed && pointer_inside && state.press_started.is_none() {
                let already_latched = state.latched.is_some();
                state.press_started = Some(now);
                state.press_direction = Some(pointer_position);
                state.press_began_on_latched = already_latched;
                state.long_latched_this_press = false;
                if !already_latched {
                    pressed = Some(pointer_position == SwitchPosition::Down);
                }
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
                    let direction = state.latched.unwrap_or(pointer_position);
                    state.latched = None;
                    released = Some(direction == SwitchPosition::Down);
                    released_latch = true;
                } else if !state.long_latched_this_press {
                    let direction = state.press_direction.unwrap_or(pointer_position);
                    let down = direction == SwitchPosition::Down;
                    action = Some(down);
                    released = Some(down);
                }
                state.press_started = None;
                state.press_direction = None;
                state.press_began_on_latched = false;
                state.long_latched_this_press = false;
            } else if state.press_started.is_some() && !primary_down && !primary_released {
                if !state.press_began_on_latched && !state.long_latched_this_press {
                    if let Some(direction) = state.press_direction {
                        released = Some(direction == SwitchPosition::Down);
                    }
                }
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
            (position, action, pressed, released, just_latched, released_latch, tracking_press)
        });

        self.draw_switch_sprite(ui, origin, scale, switch, position);
        if tracking_press { ui.ctx().request_repaint_after(Duration::from_millis(8)); }

        if let Some(down) = action {
            self.audio.play_once("assets/click.mp3");
            if just_latched {
                self.status = format!("{label} held {} — click the switch to release it", if down { "DOWN" } else { "UP" });
            }
        } else if released_latch {
            self.audio.play_once("assets/click.mp3");
            self.status = format!("{label} released to center");
        }

        MomentarySwitchInteraction { action, pressed, released }
    }

    fn draw_power(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        let switch = SWITCH_POWER;
        let hit = Self::centered_rect(origin, scale, switch.socket.0, switch.socket.1, switch.hit_size.0, switch.hit_size.1);
        let response = ui.allocate_rect(hit, Sense::click());
        let powered = self.machine.powered();
        if response.clicked() { self.set_altair_power(!powered); }
        if response.hovered() { response.clone().on_hover_text("OFF / ON"); }
        let position = if self.machine.powered() { SwitchPosition::Down } else { SwitchPosition::Up };
        self.draw_switch_sprite(ui, origin, scale, switch, position);
    }

    pub(in crate::app) fn set_altair_power(&mut self, on: bool) {
        let historical_power_on = self.config.compatibility.historical_undefined_run_latch_power_on;
        self.machine
            .power_with_historical_run_latch(on, historical_power_on);
        self.asr33.tx_started = None;
        self.audio.play_once("assets/powerbtn.mp3");
        if on {
            let panel = self.machine.front_panel_state();
            self.status = if historical_power_on {
                if panel.running {
                    "Power on — historical undefined RUN/STOP latch resolved to RUN; CPU may execute immediately"
                        .into()
                } else {
                    "Power on — historical undefined RUN/STOP latch resolved to STOP"
                        .into()
                }
            } else {
                "Power on — safe STOP latch default; original Altair still requires RESET before normal use"
                    .into()
            };
            self.audio.start_loop("altair-fan", "assets/fan.mp3");
        } else {
            self.audio.stop_loop("altair-fan");
        }
    }

    pub(in crate::app) fn draw_altair(&mut self, ui: &mut egui::Ui) {
        self.machine.commit_panel_activity(PANEL_FRAME);
        let panel = self.machine.front_panel_state();
        let lamps = panel.lamps;

        let available = ui.available_size();
        let scale = (available.x / PANEL_W).min(available.y / PANEL_H).clamp(0.2, 2.5);
        let (whole, _) = ui.allocate_exact_size(Vec2::new(PANEL_W * scale, PANEL_H * scale), Sense::hover());
        let origin = whole.min;
        if let Some(t) = &self.tex.panel { Self::image(ui, t, whole); }
        else { ui.painter().rect_filled(whole, 0.0, Color32::from_rgb(20, 25, 28)); }

        for bit in 0..16 { self.sense_switch(ui, origin, scale, bit); }
        for bit in 0..16 { self.draw_led(ui, origin, scale, ADDR_LED_X[bit], ADDR_LED_Y, lamps.address[bit], panel.powered); }
        for bit in 0..8 { self.draw_led(ui, origin, scale, DATA_LED_X[bit], DATA_LED_Y, lamps.data[bit], panel.powered); }

        self.draw_led(ui, origin, scale, STATUS_LED_X[0], STATUS_LED_Y, lamps.inte, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[1], STATUS_LED_Y, lamps.prot, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[2], STATUS_LED_Y, lamps.memr, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[3], STATUS_LED_Y, lamps.inp, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[4], STATUS_LED_Y, lamps.m1, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[5], STATUS_LED_Y, lamps.out, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[6], STATUS_LED_Y, lamps.hlta, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[7], STATUS_LED_Y, lamps.stack, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[8], STATUS_LED_Y, lamps.wo, panel.powered);
        self.draw_led(ui, origin, scale, STATUS_LED_X[9], STATUS_LED_Y, lamps.int_ack, panel.powered);
        self.draw_led(ui, origin, scale, WAIT_LED.0, WAIT_LED.1, lamps.wait, panel.powered);
        self.draw_led(ui, origin, scale, HLDA_LED.0, HLDA_LED.1, lamps.hlda, panel.powered);

        self.draw_power(ui, origin, scale);

        // RUN/STOP is a real momentary control feeding an R-S latch. Use the
        // physical press/release levels rather than a deferred GUI click action.
        let run_stop = self.momentary_switch(ui, origin, scale, SWITCH_RUN_STOP, "STOP / RUN");
        if let Some(run) = run_stop.pressed {
            self.machine.assert_run_stop(run);
            let cpu = self.machine.intel8080_state();
            let panel = self.machine.front_panel_state();
            self.status = if !run && cpu.halted.unwrap_or(false) && panel.running {
                "STOP held while CPU is halted — no PSYNC to capture STOP; hold STOP and assert RESET"
                    .into()
            } else if run {
                "RUN asserted".into()
            } else {
                "STOP asserted".into()
            };
            ui.ctx().request_repaint();
        }
        if let Some(run) = run_stop.released {
            self.machine.release_run_stop(run);
        }

        let single_step = self.momentary_switch(ui, origin, scale, SWITCH_SINGLE STEP", "SINGLE STEP");
        // The selected backend defines the physical stepping granularity: the
        // cycle-accurate core advances one machine cycle; the fast core retains
        // its instruction-level approximation.
        if let Some(down) = single_step.action {
            if !down {
                self.machine.step();
            }
        }

        let examine = self.momentary_switch(ui, origin, scale, SWITCH_EXAMINE, "EXAMINE / EXAMINE NEXT");
        if let Some(next) = examine.action { self.machine.examine(next); }

        let deposit = self.momentary_switch(ui, origin, scale, SWITCH_DEPOSIT, "DEPOSIT / DEPOSIT NEXT");
        if let Some(next) = deposit.action { self.machine.deposit(next); }

        let reset = self.momentary_switch(ui, origin, scale, SWITCH_RESET, "RESET / CLR");
        if let Some(clear) = reset.pressed {
            if clear {
                self.machine.assert_front_panel_clear();
                self.status = "CLR held: S-100 EXT CLR asserted; installed I/O boards cleared".into();
            } else {
                self.machine.assert_front_panel_reset();
                self.status = "RESET held: ADDRESS/DATA on, status lamps off; RUN/STOP latch preserved".into();
            }
            ui.ctx().request_repaint();
        }
        if let Some(clear) = reset.released {
            if clear {
                self.machine.release_front_panel_clear();
                self.status = "CLR released: S-100 EXT CLR inactive".into();
            } else {
                self.machine.release_front_panel_reset();
                self.status = if self.machine.running() {
                    "RESET released: RUN latch preserved; execution resumes from 0000h".into()
                } else {
                    "RESET released: 0000h fetch held in WAIT".into()
                };
            }
            ui.ctx().request_repaint();
        }

        let protect = self.momentary_switch(ui, origin, scale, SWITCH_PROTECT, "PROTECT / UNPROTECT");
        if let Some(unprotect) = protect.action { self.machine.protect_current_board(!unprotect); }

        let _ = self.momentary_switch(ui, origin, scale, SWITCH_AUX1, "AUX 1 (unassigned)");
        let _ = self.momentary_switch(ui, origin, scale, SWITCH_AUX2, "AUX 2 (unassigned)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn led_optics_hide_residual_activity_below_threshold() {
        assert_eq!(led_visual_response(0.0), None);
        assert_eq!(led_visual_response(LED_VISIBLE_THRESHOLD * 0.5), None);
    }

    #[test]
    fn led_optics_keep_weak_activity_red_without_white_glare() {
        let weak = led_visual_response(0.10).unwrap();
        assert!(weak.body_alpha > weak.core_alpha);
        assert!(weak.core_alpha > weak.glare_alpha);
        assert!(weak.glare_alpha <= 5);
    }

    #[test]
    fn led_optics_reach_full_core_and_glare_at_full_duty_cycle() {
        let full = led_visual_response(1.0).unwrap();
        assert_eq!(full.halo_alpha, LED_HALO_MAX_ALPHA);
        assert_eq!(full.body_alpha, 255);
        assert_eq!(full.core_alpha, 255);
        assert_eq!(full.glare_alpha, 255);
    }

    #[test]
    fn led_optics_preserve_more_contrast_than_old_sqrt_curve() {
        let quarter = led_visual_response(0.25).unwrap();
        let half = led_visual_response(0.50).unwrap();
        assert!(half.body_alpha > quarter.body_alpha);
        assert!(half.core_alpha > quarter.core_alpha);
        assert!(half.glare_alpha > quarter.glare_alpha);
        assert!(quarter.body_alpha < 128, "25% duty should no longer render as a 50% body");
    }
}

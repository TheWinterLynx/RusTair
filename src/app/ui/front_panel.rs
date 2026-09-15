use super::super::*;
use super::front_panel_assets::SwitchSpriteId;
use super::front_panel_switches::*;

const MOMENTARY_LATCH_HOLD: Duration = Duration::from_secs(3);
const KILL_BITS_MOMENTARY_PULSE: Duration = Duration::from_millis(90);
const KILL_BITS_FIRST_SENSE_BIT: usize = 8;
const LED_VISIBLE_THRESHOLD: f32 = 0.025;
const LED_HALO_MAX_ALPHA: u8 = 92;
const LED_HALO_ONSET: f32 = 0.08;
const LED_BLOOM_ONSET: f32 = 0.18;

#[derive(Clone, Copy, Debug, PartialEq)]
struct LedDisplaySettings {
    brightness: f32,
    aura: f32,
}

impl Default for LedDisplaySettings {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            aura: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LedVisualResponse {
    halo_alpha: u8,
    body_alpha: u8,
    core_alpha: u8,
    bloom_alpha: u8,
}

fn optical_alpha(max_alpha: u8, response: f32) -> u8 {
    (f32::from(max_alpha) * response.clamp(0.0, 1.0)).round() as u8
}

fn led_display_settings() -> LedDisplaySettings {
    let (brightness, aura) = super::persistence::led_visual_settings();
    LedDisplaySettings { brightness, aura }
}

#[inline]
fn remap_above_threshold(value: f32, threshold: f32) -> f32 {
    ((value - threshold) / (1.0 - threshold)).clamp(0.0, 1.0)
}

#[inline]
fn saturating_optical_response(value: f32, onset: f32, steepness: f32) -> f32 {
    let x = remap_above_threshold(value, onset);
    if x <= 0.0 {
        return 0.0;
    }
    let normalization = 1.0 - (-steepness).exp();
    (1.0 - (-steepness * x).exp()) / normalization
}

/// Convert exact electrical duty into the appearance of the original diffuse
/// red front-panel lamps. KILL THE BIT is an important calibration anchor: its
/// four LDAX D instructions put the displayed DE address on the bus for roughly
/// one quarter of the tight display loop, yet the intended upper address lamp is
/// visually dominant on real hardware. A near-linear alpha mapping therefore
/// under-renders the game. Conversely, lifting every tiny duty value makes bus
/// residue look like a real lamp. The response below has an explicit perceptual
/// floor followed by a fast saturating shoulder, preserving both behaviours.
fn led_visual_response(intensity: f32, settings: LedDisplaySettings) -> Option<LedVisualResponse> {
    let electrical = intensity.clamp(0.0, 1.0);
    if electrical < LED_VISIBLE_THRESHOLD {
        return None;
    }

    let body = saturating_optical_response(electrical, LED_VISIBLE_THRESHOLD, 9.0);
    let core = saturating_optical_response(electrical, LED_VISIBLE_THRESHOLD, 7.0);
    let halo = saturating_optical_response(electrical, LED_HALO_ONSET, 5.0);
    let bloom = saturating_optical_response(electrical, LED_BLOOM_ONSET, 6.0);

    Some(LedVisualResponse {
        halo_alpha: optical_alpha(LED_HALO_MAX_ALPHA, halo * settings.aura),
        body_alpha: optical_alpha(255, body * settings.brightness),
        core_alpha: optical_alpha(255, core * settings.brightness),
        // White saturation is an eye/camera bloom from a bright diffuse red
        // lamp, not a permanent specular dot on a modern clear-lens LED.
        bloom_alpha: optical_alpha(224, bloom * settings.brightness),
    })
}

fn sense_switch_activates_on_press(
    primary_pressed: bool,
    pointer_pos: Option<Pos2>,
    hit: Rect,
) -> bool {
    primary_pressed && pointer_pos.is_some_and(|position| hit.contains(position))
}

fn sense_switch_press_value(current: u16, bit: usize, momentary_enabled: bool) -> (u16, bool) {
    let mask = 1u16 << bit;
    let momentary = momentary_enabled && bit >= KILL_BITS_FIRST_SENSE_BIT;
    if momentary {
        (current | mask, true)
    } else {
        (current ^ mask, false)
    }
}

#[derive(Default)]
struct MomentarySwitchInteraction {
    action: Option<bool>,
    pressed: Option<bool>,
    released: Option<bool>,
}

impl RusTairApp {
    fn kill_bits_momentary_mode_id() -> egui::Id {
        egui::Id::new("rustair-kill-bits-momentary-sense-mode")
    }

    fn kill_bits_momentary_deadlines_id() -> egui::Id {
        egui::Id::new("rustair-kill-bits-momentary-sense-deadlines")
    }

    pub(in crate::app) fn kill_bits_momentary_enabled(&self, ctx: &egui::Context) -> bool {
        ctx.data(|data| {
            data.get_temp::<bool>(Self::kill_bits_momentary_mode_id())
                .unwrap_or(false)
        })
    }

    pub(in crate::app) fn set_kill_bits_momentary_enabled(
        &mut self,
        ctx: &egui::Context,
        enabled: bool,
    ) {
        let pending_mask = ctx.data_mut(|data| {
            data.insert_temp(Self::kill_bits_momentary_mode_id(), enabled);
            if enabled {
                return 0u16;
            }

            let deadlines = data.get_temp_mut_or(
                Self::kill_bits_momentary_deadlines_id(),
                [None::<Instant>; 16],
            );
            let mut pending = 0u16;
            for (bit, deadline) in deadlines.iter_mut().enumerate() {
                if deadline.take().is_some() {
                    pending |= 1u16 << bit;
                }
            }
            pending
        });

        if pending_mask != 0 {
            let switches = self.machine.switch_register();
            self.machine.set_switch_register(switches & !pending_mask);
        }
        self.status = if enabled {
            "KILL THE BIT momentary sense mode ON — A15–A8 auto-return DOWN after one short pulse (session only)".into()
        } else {
            "KILL THE BIT momentary sense mode OFF — A15–A8 are normal latching switches".into()
        };
        ctx.request_repaint();
    }

    pub(in crate::app) fn service_kill_bits_momentary_sense(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
    ) {
        let release_mask = ctx.data_mut(|data| {
            let deadlines = data.get_temp_mut_or(
                Self::kill_bits_momentary_deadlines_id(),
                [None::<Instant>; 16],
            );
            let mut release = 0u16;
            for (bit, deadline) in deadlines.iter_mut().enumerate() {
                if deadline.is_some_and(|due| now >= due) {
                    *deadline = None;
                    release |= 1u16 << bit;
                }
            }
            release
        });

        if release_mask != 0 {
            let switches = self.machine.switch_register();
            self.machine.set_switch_register(switches & !release_mask);
            ctx.request_repaint();
        }
    }

    fn arm_kill_bits_momentary_release(&mut self, ctx: &egui::Context, bit: usize) {
        let due = Instant::now() + KILL_BITS_MOMENTARY_PULSE;
        ctx.data_mut(|data| {
            let deadlines = data.get_temp_mut_or(
                Self::kill_bits_momentary_deadlines_id(),
                [None::<Instant>; 16],
            );
            deadlines[bit] = Some(due);
        });
        ctx.request_repaint_after(KILL_BITS_MOMENTARY_PULSE);
    }

    fn draw_led_visual_controls(&mut self, ctx: &egui::Context) {
        let (mut open, brightness, aura) = super::persistence::led_visual_controls_state();
        if !open {
            return;
        }

        let was_open = open;
        let mut settings = LedDisplaySettings { brightness, aura };
        let mut changed = false;
        let mut reset = false;

        egui::Window::new("LED Visual Controls")
            .id(egui::Id::new("rustair-led-visual-controls-window"))
            .open(&mut open)
            .default_pos(Pos2::new(12.0, 52.0))
            .default_width(285.0)
            .resizable(false)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.small("Presentation only — CPU, S-100 timing and LED duty-cycle are unchanged.");
                ui.separator();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut settings.brightness, 0.25..=3.0)
                            .text("Brightness")
                            .suffix("×")
                            .step_by(0.01),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut settings.aura, 0.0..=3.0)
                            .text("Aura")
                            .suffix("×")
                            .step_by(0.01),
                    )
                    .changed();
                ui.small(format!(
                    "Brightness {:.2}× · Aura {:.2}×",
                    settings.brightness, settings.aura
                ));
                ui.separator();
                if ui.button("Reset to default").clicked() {
                    settings = LedDisplaySettings::default();
                    changed = true;
                    reset = true;
                }
            });

        if changed || open != was_open {
            super::persistence::set_led_visual_controls_state(
                open,
                settings.brightness,
                settings.aura,
            );
            if changed {
                self.status = if reset {
                    "LED visuals reset to default: Brightness 1.00× · Aura 1.00×".into()
                } else {
                    format!(
                        "LED visuals: Brightness {:.2}× · Aura {:.2}×",
                        settings.brightness, settings.aura
                    )
                };
                ctx.request_repaint();
            }
        }
    }

    fn draw_led(
        &self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        intensity: f32,
        powered: bool,
        settings: LedDisplaySettings,
    ) {
        if !powered {
            return;
        }
        let Some(light) = led_visual_response(intensity, settings) else {
            return;
        };
        let center = origin + Vec2::new(x * scale, y * scale);

        // The unlit lens remains in the panel texture. These overlays represent
        // emitted light from the original diffuse red lamp: a restrained aura,
        // red body, luminous red core and, at sufficient duty, central bloom.
        if light.halo_alpha > 0 {
            ui.painter().circle_filled(
                center,
                15.5 * scale,
                Color32::from_rgba_unmultiplied(255, 16, 28, light.halo_alpha),
            );
        }
        ui.painter().circle_filled(
            center,
            10.4 * scale,
            Color32::from_rgba_unmultiplied(255, 24, 38, light.body_alpha),
        );
        ui.painter().circle_filled(
            center,
            5.6 * scale,
            Color32::from_rgba_unmultiplied(255, 96, 108, light.core_alpha),
        );
        if light.bloom_alpha > 0 {
            ui.painter().circle_filled(
                center,
                3.2 * scale,
                Color32::from_rgba_unmultiplied(255, 228, 232, light.bloom_alpha),
            );
        }
    }

    fn switch_texture(&self, sprite: SwitchSpriteId) -> Option<&egui::TextureHandle> {
        self.tex.switch_sprites.get(sprite.asset().path)
    }

    fn draw_switch_sprite(
        &self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        switch: SwitchConfig,
        position: SwitchPosition,
    ) {
        let Some(pose) = switch.pose(position) else {
            return;
        };
        let asset = pose.sprite.asset();
        let Some(texture) = self.switch_texture(pose.sprite) else {
            return;
        };
        let crop_min = Vec2::new(asset.crop_min.0, asset.crop_min.1);
        let crop_max = Vec2::new(asset.crop_max.0, asset.crop_max.1);
        let pivot_px = Vec2::new(asset.pivot.0, asset.pivot.1);
        let crop_size = crop_max - crop_min;
        let pivot_in_crop = pivot_px - crop_min;
        let socket = origin
            + Vec2::new(
                (switch.socket.0 + pose.offset.0) * scale,
                (switch.socket.1 + pose.offset.1) * scale,
            );
        let source_to_screen = asset.source_to_panel * pose.scale * scale;
        let rect = Rect::from_min_size(
            socket - pivot_in_crop * source_to_screen,
            crop_size * source_to_screen,
        );
        let uv = Rect::from_min_max(
            Pos2::new(
                crop_min.x / asset.canvas_size.0,
                crop_min.y / asset.canvas_size.1,
            ),
            Pos2::new(
                crop_max.x / asset.canvas_size.0,
                crop_max.y / asset.canvas_size.1,
            ),
        );
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    }

    fn sense_switch(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32, bit: usize) {
        let switch = SENSE_SWITCHES[bit];
        debug_assert_eq!(switch.kind, SwitchKind::TwoPosition);
        let hit = Self::centered_rect(
            origin,
            scale,
            switch.socket.0,
            switch.socket.1,
            switch.hit_size.0,
            switch.hit_size.1,
        );
        let response = ui.allocate_rect(hit, Sense::click());
        let (primary_pressed, pointer_pos) = ui.ctx().input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.interact_pos(),
            )
        });
        if response.is_pointer_button_down_on()
            && sense_switch_activates_on_press(primary_pressed, pointer_pos, hit)
        {
            let current = self.machine.switch_register();
            let (next, momentary) =
                sense_switch_press_value(current, bit, self.kill_bits_momentary_enabled(ui.ctx()));
            self.machine.set_switch_register(next);
            if momentary {
                self.arm_kill_bits_momentary_release(ui.ctx(), bit);
            }
            self.audio.play_once("assets/click.mp3");
            ui.ctx().request_repaint();
        }
        if response.hovered() {
            let momentary =
                self.kill_bits_momentary_enabled(ui.ctx()) && bit >= KILL_BITS_FIRST_SENSE_BIT;
            response.clone().on_hover_text(if momentary {
                format!(
                    "Sense switch {} — KILL THE BIT momentary mode: one click pulses UP then returns DOWN automatically",
                    switch.name
                )
            } else {
                format!("Sense switch {}", switch.name)
            });
        }
        let position = if self.machine.switch_register() & (1u16 << bit) != 0 {
            SwitchPosition::Up
        } else {
            SwitchPosition::Down
        };
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
        let hit = Self::centered_rect(
            origin,
            scale,
            switch.socket.0,
            switch.socket.1,
            switch.hit_size.0,
            switch.hit_size.1,
        );
        let response = ui.allocate_rect(hit, Sense::click_and_drag());
        if response.hovered() {
            response.clone().on_hover_text(format!("{label}\nHold for 3 seconds to keep the switch actuated; click it again to release."));
        }

        let now = Instant::now();
        let (primary_down, primary_pressed, primary_released, pointer_pos) =
            ui.ctx().input(|input| {
                (
                    input.pointer.primary_down(),
                    input.pointer.primary_pressed(),
                    input.pointer.primary_released(),
                    input.pointer.interact_pos(),
                )
            });
        let pointer_inside = pointer_pos.is_some_and(|p| hit.contains(p));
        let pointer_position = if pointer_pos
            .map(|p| p.y >= origin.y + switch.socket.1 * scale)
            .unwrap_or(false)
        {
            SwitchPosition::Down
        } else {
            SwitchPosition::Up
        };
        let state_id = egui::Id::new(("rustair-momentary-switch", switch.name));

        let (position, action, pressed, released, just_latched, released_latch, tracking_press) =
            ui.ctx().data_mut(|data| {
                let state = data.get_temp_mut_or(state_id, MomentarySwitchUiState::default());
                let mut action = None;
                let mut pressed = None;
                let mut released = None;
                let mut just_latched = false;
                let mut released_latch = false;

                if primary_pressed
                    && response.is_pointer_button_down_on()
                    && pointer_inside
                    && state.press_started.is_none()
                {
                    let already_latched = state.latched.is_some();
                    state.press_started = Some(now);
                    state.press_direction = Some(pointer_position);
                    state.press_began_on_latched = already_latched;
                    state.long_latched_this_press = false;
                    if !already_latched {
                        pressed = Some(pointer_position == SwitchPosition::Down);
                    }
                }

                if state.press_started.is_some()
                    && primary_down
                    && !state.press_began_on_latched
                    && state.latched.is_none()
                    && !state.long_latched_this_press
                    && state
                        .press_started
                        .is_some_and(|started| now.duration_since(started) >= MOMENTARY_LATCH_HOLD)
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
                        state
                            .latched
                            .or(state.press_direction)
                            .unwrap_or(SwitchPosition::Center)
                    }
                } else {
                    state.latched.unwrap_or(SwitchPosition::Center)
                };
                (
                    position,
                    action,
                    pressed,
                    released,
                    just_latched,
                    released_latch,
                    tracking_press,
                )
            });

        self.draw_switch_sprite(ui, origin, scale, switch, position);
        if tracking_press {
            ui.ctx().request_repaint_after(Duration::from_millis(8));
        }

        if let Some(down) = action {
            self.audio.play_once("assets/click.mp3");
            if just_latched {
                self.status = format!(
                    "{label} held {} — click the switch to release it",
                    if down { "DOWN" } else { "UP" }
                );
            }
        } else if released_latch {
            self.audio.play_once("assets/click.mp3");
            self.status = format!("{label} released to center");
        }

        MomentarySwitchInteraction {
            action,
            pressed,
            released,
        }
    }

    fn draw_power(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        let switch = SWITCH_POWER;
        let hit = Self::centered_rect(
            origin,
            scale,
            switch.socket.0,
            switch.socket.1,
            switch.hit_size.0,
            switch.hit_size.1,
        );
        let response = ui.allocate_rect(hit, Sense::click());
        let powered = self.machine.powered();
        if response.clicked() {
            self.set_altair_power(!powered);
        }
        if response.hovered() {
            response.clone().on_hover_text("OFF / ON");
        }
        let position = if self.machine.powered() {
            SwitchPosition::Down
        } else {
            SwitchPosition::Up
        };
        self.draw_switch_sprite(ui, origin, scale, switch, position);
    }

    pub(in crate::app) fn set_altair_power(&mut self, on: bool) {
        let historical_power_on = self
            .config
            .compatibility
            .historical_undefined_run_latch_power_on;
        self.machine
            .power_with_historical_run_latch(on, historical_power_on);
        let now = Instant::now();
        self.execution_clock.reset_at(now);
        self.last_tick = now;
        self.asr33.tx_started = None;
        self.audio.play_once("assets/powerbtn.mp3");
        if on {
            let panel = self.machine.front_panel_state();
            self.status = if historical_power_on {
                if panel.running {
                    "Power on — historical undefined RUN/STOP latch resolved to RUN; CPU may execute immediately"
                        .into()
                } else {
                    "Power on — historical undefined RUN/STOP latch resolved to STOP".into()
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

    pub(in crate::app) fn draw_altair(&mut self, ui: &mut egui::Ui, frame_dt: Duration) {
        self.machine.commit_panel_activity(frame_dt);
        self.draw_led_visual_controls(ui.ctx());
        let led_settings = led_display_settings();
        let panel = self.machine.front_panel_state();
        let lamps = panel.lamps;

        let available = ui.available_size();
        let scale = (available.x / PANEL_W)
            .min(available.y / PANEL_H)
            .clamp(0.2, 2.5);
        let (whole, _) =
            ui.allocate_exact_size(Vec2::new(PANEL_W * scale, PANEL_H * scale), Sense::hover());
        let origin = whole.min;
        if let Some(t) = &self.tex.panel {
            Self::image(ui, t, whole);
        } else {
            ui.painter()
                .rect_filled(whole, 0.0, Color32::from_rgb(20, 25, 28));
        }

        for bit in 0..16 {
            self.sense_switch(ui, origin, scale, bit);
        }
        for bit in 0..16 {
            self.draw_led(
                ui,
                origin,
                scale,
                ADDR_LED_X[bit],
                ADDR_LED_Y,
                lamps.address[bit],
                panel.powered,
                led_settings,
            );
        }
        for bit in 0..8 {
            self.draw_led(
                ui,
                origin,
                scale,
                DATA_LED_X[bit],
                DATA_LED_Y,
                lamps.data[bit],
                panel.powered,
                led_settings,
            );
        }

        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[0],
            STATUS_LED_Y,
            lamps.inte,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[1],
            STATUS_LED_Y,
            lamps.prot,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[2],
            STATUS_LED_Y,
            lamps.memr,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[3],
            STATUS_LED_Y,
            lamps.inp,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[4],
            STATUS_LED_Y,
            lamps.m1,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[5],
            STATUS_LED_Y,
            lamps.out,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[6],
            STATUS_LED_Y,
            lamps.hlta,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[7],
            STATUS_LED_Y,
            lamps.stack,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[8],
            STATUS_LED_Y,
            lamps.wo,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[9],
            STATUS_LED_Y,
            lamps.int_ack,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            WAIT_LED.0,
            WAIT_LED.1,
            lamps.wait,
            panel.powered,
            led_settings,
        );
        self.draw_led(
            ui,
            origin,
            scale,
            HLDA_LED.0,
            HLDA_LED.1,
            lamps.hlda,
            panel.powered,
            led_settings,
        );

        self.draw_power(ui, origin, scale);

        // RUN/STOP is a real momentary control feeding an R-S latch. Use the
        // physical press/release levels rather than a deferred GUI click action.
        let run_stop = self.momentary_switch(ui, origin, scale, SWITCH_RUN_STOP, "STOP / RUN");
        if let Some(run) = run_stop.pressed {
            self.machine.assert_run_stop(run);
            self.execution_clock.reset_at(Instant::now());
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

        let single_step =
            self.momentary_switch(ui, origin, scale, SWITCH_SINGLE_STEP, "SINGLE STEP");
        // The selected backend defines the physical stepping granularity: the
        // cycle-accurate core advances one machine cycle; the fast core retains
        // its instruction-level approximation.
        if let Some(down) = single_step.action {
            if !down {
                self.machine.step();
            }
        }

        let examine =
            self.momentary_switch(ui, origin, scale, SWITCH_EXAMINE, "EXAMINE / EXAMINE NEXT");
        if let Some(next) = examine.action {
            self.machine.examine(next);
        }

        let deposit =
            self.momentary_switch(ui, origin, scale, SWITCH_DEPOSIT, "DEPOSIT / DEPOSIT NEXT");
        if let Some(next) = deposit.action {
            self.machine.deposit(next);
        }

        let reset = self.momentary_switch(ui, origin, scale, SWITCH_RESET, "RESET / CLR");
        if let Some(clear) = reset.pressed {
            if clear {
                self.machine.assert_front_panel_clear();
                self.status =
                    "CLR held: S-100 EXT CLR asserted; installed I/O boards cleared".into();
            } else {
                self.machine.assert_front_panel_reset();
                self.execution_clock.reset_at(Instant::now());
                self.status =
                    "RESET held: ADDRESS/DATA on, status lamps off; RUN/STOP latch preserved"
                        .into();
            }
            ui.ctx().request_repaint();
        }
        if let Some(clear) = reset.released {
            if clear {
                self.machine.release_front_panel_clear();
                self.status = "CLR released: S-100 EXT CLR inactive".into();
            } else {
                self.machine.release_front_panel_reset();
                self.execution_clock.reset_at(Instant::now());
                self.status = if self.machine.running() {
                    "RESET released: RUN latch preserved; execution resumes from 0000h".into()
                } else {
                    "RESET released: 0000h fetch held in WAIT".into()
                };
            }
            ui.ctx().request_repaint();
        }

        let protect =
            self.momentary_switch(ui, origin, scale, SWITCH_PROTECT, "PROTECT / UNPROTECT");
        if let Some(unprotect) = protect.action {
            self.machine.protect_current_board(!unprotect);
        }

        let _ = self.momentary_switch(ui, origin, scale, SWITCH_AUX1, "AUX 1 (unassigned)");
        let _ = self.momentary_switch(ui, origin, scale, SWITCH_AUX2, "AUX 2 (unassigned)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sense_switch_changes_on_mouse_press_not_hold_or_release() {
        let hit = Rect::from_min_max(Pos2::new(10.0, 20.0), Pos2::new(30.0, 40.0));
        let inside = Some(Pos2::new(20.0, 30.0));
        let outside = Some(Pos2::new(5.0, 5.0));

        assert!(sense_switch_activates_on_press(true, inside, hit));
        assert!(!sense_switch_activates_on_press(false, inside, hit));
        assert!(!sense_switch_activates_on_press(true, outside, hit));
        assert!(!sense_switch_activates_on_press(true, None, hit));
    }

    #[test]
    fn kill_bits_momentary_mode_only_pulses_upper_sense_switches() {
        let (upper, upper_momentary) = sense_switch_press_value(0x0000, 15, true);
        assert_eq!(upper, 0x8000);
        assert!(upper_momentary);

        let (upper_again, upper_again_momentary) = sense_switch_press_value(upper, 15, true);
        assert_eq!(
            upper_again, 0x8000,
            "re-clicking extends the pulse instead of toggling it off"
        );
        assert!(upper_again_momentary);

        let (lower, lower_momentary) = sense_switch_press_value(0x0000, 7, true);
        assert_eq!(lower, 0x0080);
        assert!(
            !lower_momentary,
            "A7–A0 remain ordinary latching address switches"
        );

        let (normal, normal_momentary) = sense_switch_press_value(0x8000, 15, false);
        assert_eq!(normal, 0x0000);
        assert!(!normal_momentary);
    }

    #[test]
    fn led_optics_hide_residual_activity_below_threshold() {
        let settings = LedDisplaySettings::default();
        assert_eq!(led_visual_response(0.0, settings), None);
        assert_eq!(led_visual_response(0.02, settings), None);
    }

    #[test]
    fn led_optics_keep_small_real_activity_dim() {
        let weak = led_visual_response(0.05, LedDisplaySettings::default()).unwrap();
        assert!(weak.body_alpha < 65);
        assert!(weak.core_alpha < 55);
        assert_eq!(weak.halo_alpha, 0);
        assert_eq!(weak.bloom_alpha, 0);
    }

    #[test]
    fn led_optics_make_kill_the_bit_quarter_duty_dominant() {
        // Four 7T LDAX D instructions in the 48T tight loop expose DE for 12T.
        // The real game therefore proves that ~25% electrical duty must already
        // look like a strong lamp rather than a quarter-transparent red dot.
        let target = led_visual_response(0.25, LedDisplaySettings::default()).unwrap();
        assert!(target.body_alpha >= 215);
        assert!(target.core_alpha >= 195);
        assert!(target.halo_alpha >= 45);
        assert!(target.bloom_alpha >= 75);
    }

    #[test]
    fn led_optics_reach_full_output_at_full_duty_cycle() {
        let full = led_visual_response(1.0, LedDisplaySettings::default()).unwrap();
        assert_eq!(full.halo_alpha, LED_HALO_MAX_ALPHA);
        assert_eq!(full.body_alpha, 255);
        assert_eq!(full.core_alpha, 255);
        assert_eq!(full.bloom_alpha, 224);
    }

    #[test]
    fn led_optics_preserve_a_black_floor_and_monotonic_response() {
        let settings = LedDisplaySettings::default();
        let tenth = led_visual_response(0.10, settings).unwrap();
        let quarter = led_visual_response(0.25, settings).unwrap();
        let half = led_visual_response(0.50, settings).unwrap();
        let strong = led_visual_response(0.90, settings).unwrap();

        assert!(tenth.body_alpha < quarter.body_alpha);
        assert!(quarter.body_alpha < half.body_alpha);
        assert!(half.body_alpha <= strong.body_alpha);
        assert!(tenth.bloom_alpha == 0);
        assert!(quarter.bloom_alpha > 0);
        assert!(half.bloom_alpha > quarter.bloom_alpha);
    }

    #[test]
    fn led_live_controls_scale_brightness_and_aura_independently() {
        let base = led_visual_response(0.25, LedDisplaySettings::default()).unwrap();
        let brighter = led_visual_response(
            0.25,
            LedDisplaySettings {
                brightness: 1.15,
                aura: 1.0,
            },
        )
        .unwrap();
        let more_aura = led_visual_response(
            0.25,
            LedDisplaySettings {
                brightness: 1.0,
                aura: 1.5,
            },
        )
        .unwrap();

        assert_eq!(brighter.halo_alpha, base.halo_alpha);
        assert!(brighter.body_alpha >= base.body_alpha);
        assert!(brighter.core_alpha >= base.core_alpha);
        assert!(brighter.bloom_alpha >= base.bloom_alpha);

        assert!(more_aura.halo_alpha > base.halo_alpha);
        assert_eq!(more_aura.body_alpha, base.body_alpha);
        assert_eq!(more_aura.core_alpha, base.core_alpha);
        assert_eq!(more_aura.bloom_alpha, base.bloom_alpha);
    }
}

impl RusTairApp {
    // The supplied switch PNGs are 1254 x 1254. The old implementation fitted
    // that entire canvas into a 118 px square. This branch deliberately uses
    // half that linear scale while keeping one common source-pixel scale for
    // UP/CENTER/DOWN, so a switch never grows or shrinks between states.
    const SWITCH_SOURCE_TO_PANEL: f32 = 59.0 / 1254.0;
    const SWITCH_TEX_SIZE: f32 = 1254.0;

    fn draw_led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, on: bool) {
        // The supplied panel already contains the unlit lamp hardware. At
        // startup machine.powered=false, so no lit overlay is painted at all.
        if !self.machine.powered || !on {
            return;
        }

        // No halo, no blurred circle and no cast-shadow layer. A lit LED is a
        // compact emissive face painted directly over the fixed dark lens.
        let center = origin + Vec2::new(x * scale, y * scale);
        ui.painter().circle_filled(
            center,
            10.5 * scale,
            Color32::from_rgb(255, 24, 42),
        );
        ui.painter().circle_filled(
            center,
            5.8 * scale,
            Color32::from_rgb(255, 104, 116),
        );
        ui.painter().circle_filled(
            center + Vec2::new(-2.8 * scale, -3.0 * scale),
            2.0 * scale,
            Color32::WHITE,
        );
    }

    fn switch_texture(&self, position: SwitchPosition) -> Option<&egui::TextureHandle> {
        match position {
            SwitchPosition::Up => self.tex.switch_white[0].as_ref(),
            SwitchPosition::Center => self.tex.switch_white[1].as_ref(),
            SwitchPosition::Down => self.tex.switch_white[2].as_ref(),
        }
    }

    // Return the useful source crop and the physical pivot point in pixels of
    // the untouched 1254 x 1254 PNG. For UP the pivot is the lower end of the
    // metal stem; for DOWN it is the upper end. The CENTER pivot is not guessed
    // from the crop centre: it is the centre of the metal mount in the original
    // source before its ring was removed (approximately 627,455).
    fn switch_geometry(position: SwitchPosition) -> (Vec2, Vec2, Vec2) {
        match position {
            SwitchPosition::Up => (
                Vec2::new(390.0, 40.0),
                Vec2::new(870.0, 1095.0),
                Vec2::new(627.0, 1085.0),
            ),
            SwitchPosition::Center => (
                Vec2::new(390.0, 320.0),
                Vec2::new(860.0, 965.0),
                Vec2::new(627.0, 455.0),
            ),
            SwitchPosition::Down => (
                Vec2::new(390.0, 110.0),
                Vec2::new(865.0, 1145.0),
                Vec2::new(627.0, 118.0),
            ),
        }
    }

    fn draw_switch_sprite(
        &self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        position: SwitchPosition,
    ) {
        let Some(texture) = self.switch_texture(position) else { return; };
        let (crop_min, crop_max, pivot_px) = Self::switch_geometry(position);
        let crop_size = crop_max - crop_min;
        let pivot_in_crop = pivot_px - crop_min;

        // Invariant: every pose maps its own physical pivot onto the exact same
        // fixed socket centre in panel.png. The nut/socket is never part of the
        // moving layer; the much narrower white lever simply covers the centre
        // opening while the fixed metal ring remains visible around it.
        let socket = origin + Vec2::new(x * scale, y * scale);
        let source_to_screen = Self::SWITCH_SOURCE_TO_PANEL * scale;
        let rect = Rect::from_min_size(
            socket - pivot_in_crop * source_to_screen,
            crop_size * source_to_screen,
        );
        let uv = Rect::from_min_max(
            Pos2::new(
                crop_min.x / Self::SWITCH_TEX_SIZE,
                crop_min.y / Self::SWITCH_TEX_SIZE,
            ),
            Pos2::new(
                crop_max.x / Self::SWITCH_TEX_SIZE,
                crop_max.y / Self::SWITCH_TEX_SIZE,
            ),
        );
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    }

    fn sense_switch(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32, bit: usize) {
        let x = SENSE_X[bit];
        let hit = Self::centered_rect(origin, scale, x, SENSE_Y, 72.0, 92.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() {
            self.machine.bus.panel_switches ^= 1u16 << bit;
            self.audio.play_once("assets/click.mp3");
        }
        if response.hovered() {
            response.clone().on_hover_text(format!("Sense switch {bit}"));
        }

        // A15-A0 are all the same white bistable hardware. They have only UP
        // and DOWN, never the spring-centred CENTER pose.
        let position = if self.machine.bus.panel_switches & (1u16 << bit) != 0 {
            SwitchPosition::Up
        } else {
            SwitchPosition::Down
        };
        self.draw_switch_sprite(ui, origin, scale, x, SENSE_Y, position);
    }

    /// Spring-centred three-position switch. The resting state is CENTER; while
    /// held, the top half selects UP and the lower half selects DOWN. Releasing
    /// the mouse automatically returns the drawing to CENTER.
    fn momentary_switch(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        label: &str,
    ) -> Option<bool> {
        let hit = Self::centered_rect(origin, scale, x, y, 76.0, 96.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.hovered() {
            response.clone().on_hover_text(label);
        }

        let down = response
            .interact_pointer_pos()
            .map(|p| p.y >= origin.y + y * scale)
            .unwrap_or(false);

        let position = if response.is_pointer_button_down_on() {
            if down {
                SwitchPosition::Down
            } else {
                SwitchPosition::Up
            }
        } else {
            SwitchPosition::Center
        };
        self.draw_switch_sprite(ui, origin, scale, x, y, position);

        if response.is_pointer_button_down_on() {
            ui.ctx().request_repaint_after(Duration::from_millis(8));
        }

        if response.clicked() {
            self.audio.play_once("assets/click.mp3");
            Some(down)
        } else {
            None
        }
    }

    fn draw_power(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        let hit = Self::centered_rect(origin, scale, POWER.0, POWER.1, 76.0, 96.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() {
            self.set_altair_power(!self.machine.powered);
        }
        if response.hovered() {
            response.clone().on_hover_text("OFF / ON");
        }

        // POWER is bistable and follows the real/reference labelling: UP=OFF,
        // DOWN=ON. It never uses the CENTER texture.
        let position = if self.machine.powered {
            SwitchPosition::Down
        } else {
            SwitchPosition::Up
        };
        self.draw_switch_sprite(ui, origin, scale, POWER.0, POWER.1, position);
    }

    fn set_altair_power(&mut self, on: bool) {
        self.machine.power(on);
        self.tty_tx_started = None;
        self.audio.play_once("assets/powerbtn.mp3");

        if on {
            self.machine.address_leds = 0xffff;
            self.machine.bus.data_leds = 0xff;
            self.reset_flash_until = Some(Instant::now() + Duration::from_millis(500));
            self.audio.start_loop("altair-fan", "assets/fan.mp3");
        } else {
            self.reset_flash_until = None;
            self.audio.stop_loop("altair-fan");
        }
    }

    fn draw_altair(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let scale = (available.x / PANEL_W).min(available.y / PANEL_H).clamp(0.2, 2.5);
        let (whole, _) = ui.allocate_exact_size(
            Vec2::new(PANEL_W * scale, PANEL_H * scale),
            Sense::hover(),
        );
        let origin = whole.min;

        if let Some(t) = &self.tex.panel {
            Self::image(ui, t, whole);
        } else {
            ui.painter().rect_filled(whole, 0.0, Color32::from_rgb(20, 25, 28));
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
                self.machine.address_leds & (1u16 << bit) != 0,
            );
        }
        for bit in 0..8 {
            self.draw_led(
                ui,
                origin,
                scale,
                DATA_LED_X[bit],
                DATA_LED_Y,
                self.machine.bus.data_leds & (1u8 << bit) != 0,
            );
        }

        self.draw_led(ui, origin, scale, STATUS_LED_X[0], STATUS_LED_Y, self.machine.cpu.inte);
        self.draw_led(
            ui,
            origin,
            scale,
            STATUS_LED_X[1],
            STATUS_LED_Y,
            self.machine.current_board_protected(),
        );
        self.draw_led(ui, origin, scale, STATUS_LED_X[2], STATUS_LED_Y, true);
        self.draw_led(ui, origin, scale, STATUS_LED_X[4], STATUS_LED_Y, true);
        self.draw_led(ui, origin, scale, STATUS_LED_X[8], STATUS_LED_Y, true);
        self.draw_led(ui, origin, scale, STATUS_LED_X[6], STATUS_LED_Y, self.machine.cpu.halted);
        self.draw_led(ui, origin, scale, WAIT_LED.0, WAIT_LED.1, self.machine.wait_led);
        self.draw_led(ui, origin, scale, HLDA_LED.0, HLDA_LED.1, false);

        self.draw_power(ui, origin, scale);

        if let Some(run) = self.momentary_switch(ui, origin, scale, RUN_STOP.0, RUN_STOP.1, "STOP / RUN") {
            self.machine.set_running(run);
        }

        if self
            .momentary_switch(ui, origin, scale, SINGLE_STEP.0, SINGLE_STEP.1, "SINGLE STEP")
            .is_some()
        {
            self.machine.step();
        }

        if let Some(next) = self.momentary_switch(
            ui,
            origin,
            scale,
            EXAMINE.0,
            EXAMINE.1,
            "EXAMINE / EXAMINE NEXT",
        ) {
            self.machine.examine(next);
        }

        if let Some(next) = self.momentary_switch(
            ui,
            origin,
            scale,
            DEPOSIT.0,
            DEPOSIT.1,
            "DEPOSIT / DEPOSIT NEXT",
        ) {
            self.machine.deposit(next);
        }

        if self
            .momentary_switch(ui, origin, scale, RESET.0, RESET.1, "RESET / CLR")
            .is_some()
        {
            self.machine.reset();
            self.tty_tx_started = None;
            self.machine.address_leds = 0xffff;
            self.machine.bus.data_leds = 0xff;
            self.reset_flash_until = Some(Instant::now() + Duration::from_millis(500));
        }

        if let Some(unprotect) = self.momentary_switch(
            ui,
            origin,
            scale,
            PROTECT.0,
            PROTECT.1,
            "PROTECT / UNPROTECT",
        ) {
            self.machine.protect_current_board(!unprotect);
        }

        let _ = self.momentary_switch(ui, origin, scale, AUX1.0, AUX1.1, "AUX 1 (unassigned)");
        let _ = self.momentary_switch(ui, origin, scale, AUX2.0, AUX2.1, "AUX 2 (unassigned)");
    }
}
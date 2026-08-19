impl RusTairApp {
    fn draw_led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, on: bool) {
        if !self.machine.powered || !on {
            return;
        }
        let center = origin + Vec2::new(x * scale, y * scale);

        ui.painter().circle_filled(
            center,
            25.0 * scale,
            Color32::from_rgba_unmultiplied(255, 16, 3, 48),
        );
        ui.painter().circle_filled(
            center,
            16.0 * scale,
            Color32::from_rgba_unmultiplied(255, 38, 7, 105),
        );

        // Keep the old atlas only for the illuminated LED lens. Switches no
        // longer use the atlas at all.
        let rect = Self::centered_rect(origin, scale, x, y, 44.0, 44.0);
        if let Some(t) = &self.tex.panel_sprites {
            Self::image_uv(ui, t, rect, Self::sprite_uv(0, 0));
        } else {
            ui.painter().circle_filled(center, 10.0 * scale, Color32::from_rgb(255, 70, 20));
            ui.painter().circle_filled(center, 4.0 * scale, Color32::WHITE);
        }
    }

    fn switch_texture(
        &self,
        family: SwitchFamily,
        position: SwitchPosition,
    ) -> Option<&egui::TextureHandle> {
        match (family, position) {
            (SwitchFamily::Red, SwitchPosition::Up) => self.tex.switch_red[0].as_ref(),
            (SwitchFamily::Red, SwitchPosition::Down) => self.tex.switch_red[1].as_ref(),
            (SwitchFamily::White, SwitchPosition::Up) => self.tex.switch_white[0].as_ref(),
            (SwitchFamily::White, SwitchPosition::Down) => self.tex.switch_white[1].as_ref(),

            (SwitchFamily::Blue, SwitchPosition::Up) => self.tex.switch_blue[0].as_ref(),
            (SwitchFamily::Blue, SwitchPosition::Center) => self.tex.switch_blue[1].as_ref(),
            (SwitchFamily::Blue, SwitchPosition::Down) => self.tex.switch_blue[2].as_ref(),

            (SwitchFamily::Grey, SwitchPosition::Up) => self.tex.switch_grey[0].as_ref(),
            (SwitchFamily::Grey, SwitchPosition::Center) => self.tex.switch_grey[1].as_ref(),
            (SwitchFamily::Grey, SwitchPosition::Down) => self.tex.switch_grey[2].as_ref(),

            // Red/white controls are two-position switches and never request a
            // centre texture.
            (SwitchFamily::Red | SwitchFamily::White, SwitchPosition::Center) => None,
        }
    }

    fn draw_switch_sprite(
        &self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        family: SwitchFamily,
        position: SwitchPosition,
    ) {
        if let Some(texture) = self.switch_texture(family, position) {
            // One fixed destination rectangle for every switch and every state.
            // The application never scales UP/CENTER/DOWN differently.
            let rect = Self::centered_rect(origin, scale, x, y, 118.0, 118.0);
            Self::image(ui, texture, rect);
        }
    }

    fn sense_switch(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32, bit: usize) {
        let x = SENSE_X[bit];
        let hit = Self::centered_rect(origin, scale, x, SENSE_Y, 74.0, 100.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() {
            self.machine.bus.panel_switches ^= 1u16 << bit;
            self.audio.play_once("assets/click.mp3");
        }
        if response.hovered() {
            response.clone().on_hover_text(format!("Sense switch {bit}"));
        }

        // A15-A8 are red, A7-A0 are white. They are bistable and only have
        // physical UP/DOWN states: false = DOWN, true = UP.
        let family = if bit >= 8 {
            SwitchFamily::Red
        } else {
            SwitchFamily::White
        };
        let position = if self.machine.bus.panel_switches & (1u16 << bit) != 0 {
            SwitchPosition::Up
        } else {
            SwitchPosition::Down
        };
        self.draw_switch_sprite(ui, origin, scale, x, SENSE_Y, family, position);
    }

    /// Draw one spring-centred three-position function switch. The resting
    /// state is always CENTER. While held, the upper half uses UP and the lower
    /// half uses DOWN. The switch springs back to CENTER on release.
    fn momentary_switch(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        label: &str,
        family: SwitchFamily,
    ) -> Option<bool> {
        let hit = Self::centered_rect(origin, scale, x, y, 82.0, 104.0);
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
        self.draw_switch_sprite(ui, origin, scale, x, y, family, position);

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

    fn blue_function_switch(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        p: (f32, f32),
        label: &str,
    ) -> Option<bool> {
        self.momentary_switch(ui, origin, scale, p.0, p.1, label, SwitchFamily::Blue)
    }

    fn grey_aux_switch(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        p: (f32, f32),
        label: &str,
    ) -> Option<bool> {
        self.momentary_switch(ui, origin, scale, p.0, p.1, label, SwitchFamily::Grey)
    }

    fn draw_power(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        let hit = Self::centered_rect(origin, scale, POWER.0, POWER.1, 82.0, 106.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() {
            self.set_altair_power(!self.machine.powered);
        }
        if response.hovered() {
            response.clone().on_hover_text("OFF / ON");
        }

        // Reference Altair behaviour: UP = OFF, DOWN = ON. POWER is a
        // two-position white switch, never centred.
        let position = if self.machine.powered {
            SwitchPosition::Down
        } else {
            SwitchPosition::Up
        };
        self.draw_switch_sprite(
            ui,
            origin,
            scale,
            POWER.0,
            POWER.1,
            SwitchFamily::White,
            position,
        );
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

        ui.painter().text(
            origin + Vec2::new(POWER.0 * scale, 632.0 * scale),
            egui::Align2::CENTER_CENTER,
            "ON",
            FontId::proportional(19.0 * scale),
            Color32::from_rgb(226, 226, 214),
        );

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

        if let Some(run) = self.blue_function_switch(ui, origin, scale, RUN_STOP, "STOP / RUN") {
            self.machine.set_running(run);
        }

        if self
            .blue_function_switch(ui, origin, scale, SINGLE_STEP, "SINGLE STEP")
            .is_some()
        {
            self.machine.step();
        }

        if let Some(next) = self.blue_function_switch(
            ui,
            origin,
            scale,
            EXAMINE,
            "EXAMINE / EXAMINE NEXT",
        ) {
            self.machine.examine(next);
        }

        if let Some(next) = self.blue_function_switch(
            ui,
            origin,
            scale,
            DEPOSIT,
            "DEPOSIT / DEPOSIT NEXT",
        ) {
            self.machine.deposit(next);
        }

        if self
            .blue_function_switch(ui, origin, scale, RESET, "RESET / CLR")
            .is_some()
        {
            self.machine.reset();
            self.tty_tx_started = None;
            self.machine.address_leds = 0xffff;
            self.machine.bus.data_leds = 0xff;
            self.reset_flash_until = Some(Instant::now() + Duration::from_millis(500));
        }

        if let Some(unprotect) = self.blue_function_switch(
            ui,
            origin,
            scale,
            PROTECT,
            "PROTECT / UNPROTECT",
        ) {
            self.machine.protect_current_board(!unprotect);
        }

        let _ = self.grey_aux_switch(ui, origin, scale, AUX1, "AUX 1 (unassigned)");
        let _ = self.grey_aux_switch(ui, origin, scale, AUX2, "AUX 2 (unassigned)");
    }
}

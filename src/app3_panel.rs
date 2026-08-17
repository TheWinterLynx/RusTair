impl RusTairApp {
    fn draw_led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, on: bool) {
        if !self.machine.powered || !on {
            return;
        }
        let center = origin + Vec2::new(x * scale, y * scale);

        // A deliberately brighter two-stage bloom than the previous panel.
        ui.painter().circle_filled(
            center,
            23.0 * scale,
            Color32::from_rgba_unmultiplied(255, 18, 5, 34),
        );
        ui.painter().circle_filled(
            center,
            15.0 * scale,
            Color32::from_rgba_unmultiplied(255, 28, 8, 70),
        );

        let rect = Self::centered_rect(origin, scale, x, y, 42.0, 42.0);
        if let Some(t) = &self.tex.panel_sprites {
            Self::image_uv(ui, t, rect, Self::sprite_uv(0, 0));
        } else {
            ui.painter().circle_filled(center, 9.0 * scale, Color32::from_rgb(255, 70, 20));
            ui.painter().circle_filled(center, 4.0 * scale, Color32::WHITE);
        }
    }

    fn draw_panel_sprite(
        &self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        size: f32,
        cell: (usize, usize),
    ) {
        if let Some(t) = &self.tex.panel_sprites {
            Self::image_uv(
                ui,
                t,
                Self::centered_rect(origin, scale, x, y, size, size),
                Self::sprite_uv(cell.0, cell.1),
            );
        }
    }

    fn sense_switch(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32, bit: usize) {
        let x = SENSE_X[bit];
        let hit = Self::centered_rect(origin, scale, x, SENSE_Y, 58.0, 78.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() {
            self.machine.bus.panel_switches ^= 1u16 << bit;
            self.audio.play_once("assets/click.mp3");
        }
        if response.hovered() {
            response.clone().on_hover_text(format!("Sense switch {bit}"));
        }

        let up = self.machine.bus.panel_switches & (1u16 << bit) != 0;
        let cell = if bit >= 8 {
            if up { (1, 0) } else { (2, 0) }
        } else if up {
            (3, 0)
        } else {
            (0, 1)
        };
        self.draw_panel_sprite(ui, origin, scale, x, SENSE_Y, 82.0, cell);
    }

    fn button_response(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        label: &str,
    ) -> egui::Response {
        let hit = Self::centered_rect(origin, scale, x, y, 72.0, 72.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.hovered() {
            response.clone().on_hover_text(label);
        }

        let pressed = response.is_pointer_button_down_on();
        let sprite_size = if pressed { 43.0 } else { 49.0 };
        let sprite_y = y + if pressed { 3.5 } else { 0.0 };
        self.draw_panel_sprite(ui, origin, scale, x, sprite_y, sprite_size, (3, 1));
        response
    }

    fn momentary_top_bottom(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        label: &str,
    ) -> Option<bool> {
        let response = self.button_response(ui, origin, scale, x, y, label);
        if response.clicked() {
            self.audio.play_once("assets/click.mp3");
            let down = response
                .interact_pointer_pos()
                .map(|p| p.y >= (origin.y + y * scale))
                .unwrap_or(false);
            Some(down)
        } else {
            None
        }
    }

    fn draw_power(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32) {
        let hit = Self::centered_rect(origin, scale, POWER.0, POWER.1, 82.0, 92.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() {
            self.set_altair_power(!self.machine.powered);
        }
        if response.hovered() {
            response.clone().on_hover_text("POWER");
        }
        let cell = if self.machine.powered { (3, 0) } else { (0, 1) };
        self.draw_panel_sprite(ui, origin, scale, POWER.0, POWER.1, 92.0, cell);
    }

    fn draw_aux(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32, p: (f32, f32), label: &str) {
        let hit = Self::centered_rect(origin, scale, p.0, p.1, 68.0, 82.0);
        let response = ui.allocate_rect(hit, Sense::click());
        let down = response.is_pointer_button_down_on();
        let cell = if down { (2, 1) } else { (1, 1) };
        self.draw_panel_sprite(ui, origin, scale, p.0, p.1, 78.0, cell);
        if response.clicked() {
            self.audio.play_once("assets/click.mp3");
        }
        if response.hovered() {
            response.clone().on_hover_text(label);
        }
    }

    fn set_altair_power(&mut self, on: bool) {
        self.machine.power(on);
        self.tty_tx_started = None;
        self.audio.play_once("assets/powerbtn.mp3");
        if on {
            self.audio.start_loop("altair-fan", "assets/fan.mp3");
        } else {
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
                ui, origin, scale, ADDR_LED_X[bit], ADDR_LED_Y,
                self.machine.address_leds & (1u16 << bit) != 0,
            );
        }
        for bit in 0..8 {
            self.draw_led(
                ui, origin, scale, DATA_LED_X[bit], DATA_LED_Y,
                self.machine.bus.data_leds & (1u8 << bit) != 0,
            );
        }

        // Status row: only light signals currently represented by the emulator.
        self.draw_led(ui, origin, scale, STATUS_LED_X[0], STATUS_LED_Y, self.machine.cpu.inte); // INTE
        self.draw_led(ui, origin, scale, STATUS_LED_X[1], STATUS_LED_Y, self.machine.current_board_protected()); // PROT
        self.draw_led(ui, origin, scale, STATUS_LED_X[6], STATUS_LED_Y, self.machine.cpu.halted); // HLTA
        self.draw_led(ui, origin, scale, WAIT_LED.0, WAIT_LED.1, self.machine.wait_led);
        self.draw_led(ui, origin, scale, HLDA_LED.0, HLDA_LED.1, false);

        self.draw_power(ui, origin, scale);

        if let Some(run) = self.momentary_top_bottom(
            ui, origin, scale, RUN_STOP.0, RUN_STOP.1, "STOP / RUN",
        ) {
            self.machine.set_running(run);
        }

        if self.button_response(
            ui, origin, scale, SINGLE_STEP.0, SINGLE_STEP.1, "SINGLE STEP",
        ).clicked() {
            self.audio.play_once("assets/click.mp3");
            self.machine.step();
        }

        if let Some(next) = self.momentary_top_bottom(
            ui, origin, scale, EXAMINE.0, EXAMINE.1, "EXAMINE / EXAMINE NEXT",
        ) {
            self.machine.examine(next);
        }

        if let Some(next) = self.momentary_top_bottom(
            ui, origin, scale, DEPOSIT.0, DEPOSIT.1, "DEPOSIT / DEPOSIT NEXT",
        ) {
            self.machine.deposit(next);
        }

        if self.button_response(
            ui, origin, scale, RESET.0, RESET.1, "RESET / CLR",
        ).clicked() {
            self.audio.play_once("assets/click.mp3");
            self.machine.reset();
            self.tty_tx_started = None;
            self.machine.address_leds = 0xffff;
            self.machine.bus.data_leds = 0xff;
            self.reset_flash_until = Some(Instant::now() + Duration::from_millis(500));
        }

        if let Some(unprotect) = self.momentary_top_bottom(
            ui, origin, scale, PROTECT.0, PROTECT.1, "PROTECT / UNPROTECT",
        ) {
            self.machine.protect_current_board(!unprotect);
        }

        self.draw_aux(ui, origin, scale, AUX1, "AUX 1 (unassigned)");
        self.draw_aux(ui, origin, scale, AUX2, "AUX 2 (unassigned)");
    }
}

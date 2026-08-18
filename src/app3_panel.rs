impl RusTairApp {
    fn draw_led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, on: bool) {
        if !self.machine.powered || !on {
            return;
        }
        let center = origin + Vec2::new(x * scale, y * scale);

        // Bright two-stage incandescent bloom. The clean panel already carries
        // the dark/off lens, so this overlay only appears for an illuminated LED.
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

        let rect = Self::centered_rect(origin, scale, x, y, 44.0, 44.0);
        if let Some(t) = &self.tex.panel_sprites {
            Self::image_uv(ui, t, rect, Self::sprite_uv(0, 0));
        } else {
            ui.painter().circle_filled(center, 10.0 * scale, Color32::from_rgb(255, 70, 20));
            ui.painter().circle_filled(center, 4.0 * scale, Color32::WHITE);
        }
    }

    /// Draw one transparent moving-control overlay. The photographed/cleaned
    /// panel itself owns the fixed switch bezel and mounting hole; the atlas
    /// contains only the lever, cap and moving shaft. This is important because
    /// the metal base of a real toggle never rotates or translates with the lever.
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
        let hit = Self::centered_rect(origin, scale, x, SENSE_Y, 74.0, 100.0);
        let response = ui.allocate_rect(hit, Sense::click());
        if response.clicked() {
            self.machine.bus.panel_switches ^= 1u16 << bit;
            self.audio.play_once("assets/click.mp3");
        }
        if response.hovered() {
            response.clone().on_hover_text(format!("Sense switch {bit}"));
        }

        // SENSE switches are bistable: one clear DOWN and one clear UP pose.
        let up = self.machine.bus.panel_switches & (1u16 << bit) != 0;
        let cell = if bit >= 8 {
            if up { (1, 0) } else { (2, 0) }
        } else if up {
            (3, 0)
        } else {
            (0, 1)
        };
        self.draw_panel_sprite(ui, origin, scale, x, SENSE_Y, 118.0, cell);
    }

    /// Draw one spring-centred function switch. `cells` are (up, centre, down).
    /// The fixed metal base is part of panel.jpg. Only the lever overlay changes.
    /// Returns Some(true) for lower/down actuation and Some(false) for upper/up.
    fn momentary_switch(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        label: &str,
        cells: [(usize, usize); 3],
        size: f32,
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

        // These controls rest in a true neutral centre pose. Only while held do
        // they show the modest UP or DOWN spring travel.
        let cell = if response.is_pointer_button_down_on() {
            if down { cells[2] } else { cells[0] }
        } else {
            cells[1]
        };
        self.draw_panel_sprite(ui, origin, scale, x, y, size, cell);

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
        self.momentary_switch(
            ui, origin, scale, p.0, p.1, label,
            [(1, 1), (2, 1), (3, 1)],
            118.0,
        )
    }

    fn black_aux_switch(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        p: (f32, f32),
        label: &str,
    ) -> Option<bool> {
        self.momentary_switch(
            ui, origin, scale, p.0, p.1, label,
            [(0, 2), (1, 2), (2, 2)],
            116.0,
        )
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

        // POWER is a bistable white toggle just like the SENSE switches. The
        // captured altair implementation powers on in the DOWN position.
        let cell = if self.machine.powered { (0, 1) } else { (3, 0) };
        self.draw_panel_sprite(ui, origin, scale, POWER.0, POWER.1, 118.0, cell);
    }

    fn set_altair_power(&mut self, on: bool) {
        self.machine.power(on);
        self.tty_tx_started = None;
        self.audio.play_once("assets/powerbtn.mp3");

        if on {
            // Match sim.html Handle_Power -> Handle_Reset exactly: at power-on
            // all address/data lamps flash for 500 ms and WAIT remains lit.
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

        // The clean-plate generation removed the original lower POWER legend.
        // Restore it as part of the runtime skin so OFF/ON is always visible.
        ui.painter().text(
            origin + Vec2::new(POWER.0 * scale, 660.0 * scale),
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

        // STATUS order on the photographic panel:
        // INTE, PROT, MEMR, INP, M1, OUT, HLTA, STACK, WO, INT.
        self.draw_led(ui, origin, scale, STATUS_LED_X[0], STATUS_LED_Y, self.machine.cpu.inte);
        self.draw_led(ui, origin, scale, STATUS_LED_X[1], STATUS_LED_Y, self.machine.current_board_protected());

        // The reference sim marks MEMR, M1 and WO as AlwaysOn whenever the
        // machine has power. They were missing from the RusTair skin.
        self.draw_led(ui, origin, scale, STATUS_LED_X[2], STATUS_LED_Y, true);
        self.draw_led(ui, origin, scale, STATUS_LED_X[4], STATUS_LED_Y, true);
        self.draw_led(ui, origin, scale, STATUS_LED_X[8], STATUS_LED_Y, true);

        self.draw_led(ui, origin, scale, STATUS_LED_X[6], STATUS_LED_Y, self.machine.cpu.halted);
        self.draw_led(ui, origin, scale, WAIT_LED.0, WAIT_LED.1, self.machine.wait_led);
        self.draw_led(ui, origin, scale, HLDA_LED.0, HLDA_LED.1, false);

        self.draw_power(ui, origin, scale);

        // All lower blue controls are three-position toggle switches. They rest
        // at centre and spring back after the selected up/down action.
        if let Some(run) = self.blue_function_switch(
            ui, origin, scale, RUN_STOP, "STOP / RUN",
        ) {
            self.machine.set_running(run);
        }

        if self.blue_function_switch(
            ui, origin, scale, SINGLE_STEP, "SINGLE STEP",
        ).is_some() {
            self.machine.step();
        }

        if let Some(next) = self.blue_function_switch(
            ui, origin, scale, EXAMINE, "EXAMINE / EXAMINE NEXT",
        ) {
            self.machine.examine(next);
        }

        if let Some(next) = self.blue_function_switch(
            ui, origin, scale, DEPOSIT, "DEPOSIT / DEPOSIT NEXT",
        ) {
            self.machine.deposit(next);
        }

        if self.blue_function_switch(
            ui, origin, scale, RESET, "RESET / CLR",
        ).is_some() {
            self.machine.reset();
            self.tty_tx_started = None;
            self.machine.address_leds = 0xffff;
            self.machine.bus.data_leds = 0xff;
            self.reset_flash_until = Some(Instant::now() + Duration::from_millis(500));
        }

        if let Some(unprotect) = self.blue_function_switch(
            ui, origin, scale, PROTECT, "PROTECT / UNPROTECT",
        ) {
            self.machine.protect_current_board(!unprotect);
        }

        let _ = self.black_aux_switch(ui, origin, scale, AUX1, "AUX 1 (unassigned)");
        let _ = self.black_aux_switch(ui, origin, scale, AUX2, "AUX 2 (unassigned)");
    }
}
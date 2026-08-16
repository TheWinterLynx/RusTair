mod cpu8080;
mod machine;

use std::time::{Duration, Instant};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use machine::{AltairMachine, CLOCK_HZ};

const PANEL_W: f32 = 1450.0;
const PANEL_H: f32 = 545.0;

const SWITCH_X: [f32; 16] = [
    1332., 1278., 1224., 1142., 1087., 1032., 950., 895.,
    840., 758., 703., 648., 566., 512., 457., 376.
];
const ADDR_LED_X: [f32; 16] = [
    1341.,1286.,1231.,1148.,1093.,1037.,955.,900.,845.,763.,708.,653.,573.,518.,463.,381.
];
const DATA_LED_X: [f32; 8] = [1341.,1286.,1231.,1148.,1093.,1037.,955.,900.];

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RusTair — MITS Altair 8800")
            .with_inner_size([1200.0, 620.0])
            .with_min_inner_size([800.0, 430.0]),
        ..Default::default()
    };
    eframe::run_native("RusTair", options, Box::new(|cc| Ok(Box::new(RusTairApp::new(cc)))))
}

struct RusTairApp {
    machine: AltairMachine,
    panel: Option<egui::TextureHandle>,
    led_on: Option<egui::TextureHandle>,
    switch_up: Option<egui::TextureHandle>,
    switch_down: Option<egui::TextureHandle>,
    last_tick: Instant,
    status: String,
}

impl RusTairApp {
    fn load_texture(ctx: &egui::Context, name: &str, path: &str) -> Option<egui::TextureHandle> {
        let bytes = std::fs::read(path).ok()?;
        let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = image.into_raw();
        Some(ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, &pixels), egui::TextureOptions::LINEAR))
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self {
            machine: AltairMachine::default(),
            panel: Self::load_texture(&cc.egui_ctx, "panel", "assets/Altair1.png"),
            led_on: Self::load_texture(&cc.egui_ctx, "led_on", "assets/LEDon.png"),
            switch_up: Self::load_texture(&cc.egui_ctx, "switch_up", "assets/SwitchUp.png"),
            switch_down: Self::load_texture(&cc.egui_ctx, "switch_down", "assets/SwitchDown.png"),
            last_tick: Instant::now(),
            status: "Assets are loaded from ./assets at runtime.".into(),
        }
    }

    fn overlay_image(ui: &mut egui::Ui, texture: &egui::TextureHandle, rect: Rect) {
        ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.,1.)), Color32::WHITE);
    }

    fn led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, on: bool) {
        if !on || !self.machine.powered { return; }
        let r = Rect::from_min_size(origin + Vec2::new(x*scale, y*scale), Vec2::splat(24.*scale));
        if let Some(t) = &self.led_on { Self::overlay_image(ui, t, r); }
        else { ui.painter().circle_filled(r.center(), 8.*scale, Color32::from_rgb(255,40,20)); }
    }

    fn momentary(&mut self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, label: &str) -> bool {
        let rect = Rect::from_min_size(origin + Vec2::new(x*scale, y*scale), Vec2::new(38.*scale, 90.*scale));
        let response = ui.allocate_rect(rect, Sense::click());
        if response.hovered() { ui.painter().rect_stroke(rect, 2., Stroke::new(1., Color32::from_white_alpha(80)), egui::StrokeKind::Inside); }
        response.on_hover_text(label).clicked()
    }

    fn draw_panel(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let scale = (avail.x / PANEL_W).min((avail.y - 40.0).max(100.0) / PANEL_H).max(0.2);
        let size = Vec2::new(PANEL_W*scale, PANEL_H*scale);
        let (panel_rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let origin = panel_rect.min;

        if let Some(panel) = &self.panel {
            Self::overlay_image(ui, panel, panel_rect);
        } else {
            ui.painter().rect_filled(panel_rect, 0., Color32::from_rgb(25,35,43));
            ui.painter().text(panel_rect.center(), egui::Align2::CENTER_CENTER,
                "Altair1.png not found\nRun scripts/import-assets.ps1 or copy assets from the original repo",
                egui::FontId::proportional(22.*scale), Color32::LIGHT_GRAY);
        }

        for bit in 0..16 {
            let x = SWITCH_X[bit];
            let rect = Rect::from_min_size(origin + Vec2::new(x*scale, 301.*scale), Vec2::new(32.*scale, 96.*scale));
            let resp = ui.allocate_rect(rect, Sense::click());
            if resp.clicked() { self.machine.bus.panel_switches ^= 1u16 << bit; }
            let on = self.machine.bus.panel_switches & (1u16 << bit) != 0;
            let tex = if on { self.switch_up.as_ref() } else { self.switch_down.as_ref() };
            if let Some(t) = tex { Self::overlay_image(ui, t, rect); }
        }

        for bit in 0..16 { self.led(ui, origin, scale, ADDR_LED_X[bit], if bit < 4 {233.} else {230.}, self.machine.address_leds & (1u16<<bit) != 0); }
        for bit in 0..8 { self.led(ui, origin, scale, DATA_LED_X[bit], if bit < 4 {122.} else {120.}, self.machine.bus.data_leds & (1u8<<bit) != 0); }
        self.led(ui, origin, scale, 218., 228., self.machine.wait_led);
        self.led(ui, origin, scale, 324., 119., self.machine.powered);
        self.led(ui, origin, scale, 434., 120., self.machine.powered);
        self.led(ui, origin, scale, 654., 120., self.machine.powered);

        let power_rect = Rect::from_min_size(origin + Vec2::new(114.*scale, 408.*scale), Vec2::new(32.*scale, 96.*scale));
        if ui.allocate_rect(power_rect, Sense::click()).clicked() { self.machine.power(!self.machine.powered); }
        let ptex = if self.machine.powered { self.switch_down.as_ref() } else { self.switch_up.as_ref() };
        if let Some(t) = ptex { Self::overlay_image(ui, t, power_rect); }

        if self.momentary(ui, origin, scale, 377.,410.,"RUN / STOP") { self.machine.set_running(!self.machine.running); }
        if self.momentary(ui, origin, scale, 486.,410.,"SINGLE STEP") { self.machine.step(); }
        if self.momentary(ui, origin, scale, 595.,410.,"EXAMINE") { self.machine.examine(false); }
        if self.momentary(ui, origin, scale, 704.,410.,"DEPOSIT") { self.machine.deposit(false); }
        if self.momentary(ui, origin, scale, 813.,410.,"RESET") { self.machine.reset(); }
    }
}

impl eframe::App for RusTairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).min(Duration::from_millis(20));
        self.last_tick = now;
        if self.machine.running {
            let cycles = (CLOCK_HZ as f64 * dt.as_secs_f64()) as u32;
            self.machine.run_cycles(cycles.max(1));
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Load binary…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            match std::fs::read(&path) {
                                Ok(bytes) => { self.machine.bus.load(0, &bytes); self.status = format!("Loaded {} bytes from {}", bytes.len(), path.display()); }
                                Err(e) => self.status = format!("Load failed: {e}"),
                            }
                        }
                        ui.close();
                    }
                    if ui.button("Load bundled 4K BASIC").clicked() {
                        match std::fs::read("assets/4kbas32.bin") {
                            Ok(bytes) => { self.machine.bus.load(0, &bytes); self.machine.cpu.pc = 0; self.status = "Loaded Microsoft 4K BASIC image".into(); }
                            Err(e) => self.status = format!("4K BASIC asset missing: {e}"),
                        }
                        ui.close();
                    }
                });
                ui.separator();
                ui.label(format!("PC {:04X}  SP {:04X}  A {:02X}  F {:02X}", self.machine.cpu.pc, self.machine.cpu.sp, self.machine.cpu.a, self.machine.cpu.f));
                ui.separator();
                ui.label(if self.machine.running { "RUNNING" } else if self.machine.powered { "STOPPED" } else { "POWER OFF" });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| self.draw_panel(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| { ui.small(&self.status); });
    }
}

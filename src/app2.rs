mod cpu8080;
mod disasm;
mod machine;

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};
use machine::{AltairMachine, CLOCK_HZ};
use rustair::audio::AudioEngine;
use rustair::teletype::{self, KeyKind, Mode as TtyMode, PrintEvent, Teletype};

const PANEL_W: f32 = 1573.0;
const PANEL_H: f32 = 647.0;
const DRAWER_W: f32 = 1105.0;
const DRAWER_H: f32 = 235.0;
const DRAWER_X: f32 = 202.0;
const DRAWER_CLOSED_Y: f32 = PANEL_H - 192.0;
const DRAWER_OPEN_Y: f32 = PANEL_H - 6.0;
const TTY_W: f32 = teletype::IMAGE_W;
const TTY_H: f32 = teletype::IMAGE_H;

const PANEL_FRAME: Duration = Duration::from_millis(16);
const TTY_CHAR_TIME: Duration = Duration::from_millis(90);
const KEY_TAP_TIME: Duration = Duration::from_millis(75);

const SWITCH_X: [f32; 16] = [
    1332.,1278.,1224.,1142.,1087.,1032.,950.,895.,
    840.,758.,703.,648.,566.,512.,457.,376.,
];
const SWITCH_Y: [f32; 16] = [
    305.,305.,305.,305.,303.,303.,303.,303.,
    303.,303.,303.,301.,301.,301.,301.,301.,
];
const ADDR_LED_X: [f32; 16] = [
    1341.,1286.,1231.,1148.,1093.,1037.,955.,900.,
    845.,763.,708.,653.,573.,518.,463.,381.,
];
const ADDR_LED_Y: [f32; 16] = [
    233.,233.,233.,233.,231.,231.,231.,230.,
    230.,230.,230.,230.,229.,229.,229.,229.,
];
const DATA_LED_X: [f32; 8] = [1341.,1286.,1231.,1148.,1093.,1037.,955.,900.];
const DATA_LED_Y: [f32; 8] = [122.,122.,122.,122.,120.,120.,120.,120.];

struct Tex {
    panel: Option<egui::TextureHandle>,
    slideout: Option<egui::TextureHandle>,
    led_on: Option<egui::TextureHandle>,
    switch_up: Option<egui::TextureHandle>,
    switch_down: Option<egui::TextureHandle>,
    switch_centre: Option<egui::TextureHandle>,
    tty_body: Option<egui::TextureHandle>,
    tty_keys: Option<egui::TextureHandle>,
    tty_head: Option<egui::TextureHandle>,
    tty_line_local: Option<egui::TextureHandle>,
    tty_knob: Option<egui::TextureHandle>,
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RusTair — MITS Altair 8800")
            .with_inner_size([1400.0, 820.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RusTair",
        options,
        Box::new(|cc| Ok(Box::new(RusTairApp::new(cc)))),
    )
}

struct RusTairApp {
    machine: AltairMachine,
    tex: Tex,
    tty: Teletype,
    tty_window_open: bool,
    audio: AudioEngine,

    last_tick: Instant,
    last_tape_tick: Instant,
    reset_flash_until: Option<Instant>,

    tty_tx_started: Option<Instant>,
    print_head_raise_until: Option<Instant>,
    tty_power_flash_until: Option<Instant>,

    animated_key: Option<usize>,
    pressed_key: Option<usize>,
    key_auto_release_at: Option<Instant>,
    key_displacement: f32,
    key_anim_tick: Instant,

    inside_open: bool,
    inside_slide: f32,
    status: String,
}

impl RusTairApp {
    fn load_texture(ctx: &egui::Context, name: &str, path: &str) -> Option<egui::TextureHandle> {
        let bytes = std::fs::read(path).ok()?;
        let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        Some(ctx.load_texture(
            name,
            egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw()),
            egui::TextureOptions::LINEAR,
        ))
    }

    fn install_teletype_font(ctx: &egui::Context) {
        let Ok(bytes) = std::fs::read("assets/teletype.ttf") else { return; };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "teletype".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts.families.insert(
            FontFamily::Name("teletype".into()),
            vec!["teletype".to_owned()],
        );
        ctx.set_fonts(fonts);
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self::install_teletype_font(&cc.egui_ctx);
        let now = Instant::now();
        Self {
            machine: AltairMachine::default(),
            tex: Tex {
                panel: Self::load_texture(&cc.egui_ctx, "altair-panel", "assets/Altair1.png"),
                slideout: Self::load_texture(&cc.egui_ctx, "altair-slideout", "assets/slideout.png"),
                led_on: Self::load_texture(&cc.egui_ctx, "led-on", "assets/LEDon.png"),
                switch_up: Self::load_texture(&cc.egui_ctx, "switch-up", "assets/SwitchUp.png"),
                switch_down: Self::load_texture(&cc.egui_ctx, "switch-down", "assets/SwitchDown.png"),
                switch_centre: Self::load_texture(&cc.egui_ctx, "switch-centre", "assets/SwitchCentre.png"),
                tty_body: Self::load_texture(&cc.egui_ctx, "tty-body", "assets/asr33 body.jpg"),
                tty_keys: Self::load_texture(&cc.egui_ctx, "tty-keys", "assets/asr33 keys.png"),
                tty_head: Self::load_texture(&cc.egui_ctx, "tty-head", "assets/asr33head.png"),
                tty_line_local: Self::load_texture(&cc.egui_ctx, "tty-line-local", "assets/asrlinelocal.png"),
                tty_knob: Self::load_texture(&cc.egui_ctx, "tty-knob", "assets/asrlinelocalknob.png"),
            },
            tty: Teletype::default(),
            tty_window_open: false,
            audio: AudioEngine::new(),
            last_tick: now,
            last_tape_tick: now,
            reset_flash_until: None,
            tty_tx_started: None,
            print_head_raise_until: None,
            tty_power_flash_until: None,
            animated_key: None,
            pressed_key: None,
            key_auto_release_at: None,
            key_displacement: 0.0,
            key_anim_tick: now,
            inside_open: false,
            inside_slide: 0.0,
            status: "Ready".into(),
        }
    }

    fn image(ui: &mut egui::Ui, texture: &egui::TextureHandle, rect: Rect) {
        Self::image_uv(
            ui,
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        );
    }

    fn image_uv(ui: &mut egui::Ui, texture: &egui::TextureHandle, rect: Rect, uv: Rect) {
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    }

    fn led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, on: bool) {
        if !self.machine.powered || !on { return; }
        let rect = Rect::from_min_size(
            origin + Vec2::new(x * scale, y * scale),
            Vec2::splat(24.0 * scale),
        );
        if let Some(texture) = &self.tex.led_on {
            Self::image(ui, texture, rect);
        } else {
            ui.painter().circle_filled(
                rect.center(),
                8.0 * scale,
                Color32::from_rgb(255, 45, 20),
            );
        }
    }

    fn momentary(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        scale: f32,
        x: f32,
        y: f32,
        label: &str,
    ) -> Option<bool> {
        let rect = Rect::from_min_size(
            origin + Vec2::new(x * scale, y * scale),
            Vec2::new(32.0 * scale, 96.0 * scale),
        );
        let response = ui.allocate_rect(rect, Sense::click());
        let down = response.interact_pointer_pos()
            .map(|p| p.y > rect.center().y)
            .unwrap_or(false);
        let texture = if response.is_pointer_button_down_on() {
            if down { self.tex.switch_down.as_ref() } else { self.tex.switch_up.as_ref() }
        } else {
            self.tex.switch_centre.as_ref()
        };
        if let Some(texture) = texture { Self::image(ui, texture, rect); }
        if response.hovered() { response.clone().on_hover_text(label); }
        if response.clicked() {
            self.audio.play_once("assets/click.mp3");
            Some(down)
        } else {
            None
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
            self.inside_open = false;
        }
    }

    fn flags_text(&self) -> String {
        let f = self.machine.cpu.f;
        let mut flags = Vec::new();
        if f & 0x80 != 0 { flags.push("S"); }
        if f & 0x40 != 0 { flags.push("Z"); }
        if f & 0x10 != 0 { flags.push("AC"); }
        if f & 0x04 != 0 { flags.push("P"); }
        if f & 0x01 != 0 { flags.push("C"); }
        if flags.is_empty() { "-".into() } else { flags.join(" ") }
    }

    fn draw_inside_text(&self, ui: &mut egui::Ui, drawer: Rect, scale: f32) {
        if self.inside_slide < 0.72 { return; }
        let p = ui.painter();
        let mono = |px: f32| FontId::monospace((px * scale).max(7.0));
        let ink = Color32::from_rgb(12, 25, 29);
        let accent = Color32::from_rgb(220, 235, 190);
        let x = drawer.left() + 38.0 * scale;
        let y = drawer.top() + 28.0 * scale;

        p.text(Pos2::new(x, y), egui::Align2::LEFT_TOP, "REGISTERS", mono(14.0), ink);
        p.text(
            Pos2::new(drawer.left() + 370.0 * scale, y),
            egui::Align2::LEFT_TOP,
            "RANDOM ACCESS MEMORY (RAM)",
            mono(14.0),
            ink,
        );

        let c = &self.machine.cpu;
        let regs = [
            format!("A  {:08b}  ${:02X}    F  {:08b}  ${:02X}", c.a, c.a, c.f, c.f),
            format!("B  {:08b}  ${:02X}    C  {:08b}  ${:02X}", c.b, c.b, c.c, c.c),
            format!("D  {:08b}  ${:02X}    E  {:08b}  ${:02X}", c.d, c.d,c.e,c.e),
            format!("H  {:08b}  ${:02X}    L  {:08b}  ${:02X}", c.h,c.h,c.l,c.l),
            format!("SP {:016b}  ${:04X}", c.sp,c.sp),
            format!("PC {:016b}  ${:04X}", c.pc,c.pc),
        ];
        for (i,line) in regs.iter().enumerate() {
            p.text(
                Pos2::new(x,y+(30.0+i as f32*24.0)*scale),
                egui::Align2::LEFT_TOP,
                line,
                mono(11.0),
                accent,
            );
        }
        p.text(
            Pos2::new(x,y+174.0*scale),
            egui::Align2::LEFT_TOP,
            format!("FLAGS  {}",self.flags_text()),
            mono(11.0),
            accent,
        );

        let pc=c.pc as usize;
        let m=&self.machine.bus.memory;
        let op=m.get(pc).copied().unwrap_or(0);
        let b1=m.get(pc.wrapping_add(1)).copied().unwrap_or(0);
        let b2=m.get(pc.wrapping_add(2)).copied().unwrap_or(0);
        p.text(
            Pos2::new(x,y+195.0*scale),
            egui::Align2::LEFT_TOP,
            format!("NEXT   {}",disasm::disassemble(op,b1,b2)),
            mono(11.0),
            accent,
        );

        let start=(pc&!0x0f)
            .saturating_sub(32)
            .min(machine::MEM_SIZE.saturating_sub(16));
        for row in 0..7 {
            let addr=start+row*16;
            if addr>=machine::MEM_SIZE { break; }
            let mut line=format!("{addr:04X}   ");
            for col in 0..16 {
                if let Some(v)=m.get(addr+col) {
                    line.push_str(&format!("{v:02X} "));
                }
            }
            p.text(
                Pos2::new(
                    drawer.left()+370.0*scale,
                    y+(30.0+row as f32*22.0)*scale,
                ),
                egui::Align2::LEFT_TOP,
                line,
                mono(10.0),
                accent,
            );
        }
    }

    fn draw_altair(&mut self, ui:&mut egui::Ui) {
        let a=ui.available_size();
        let total_h=PANEL_H+229.0;
        let scale=(a.x/PANEL_W).min(a.y/total_h).clamp(0.2,2.5);
        let (whole,_)=ui.allocate_exact_size(
            Vec2::new(PANEL_W*scale,total_h*scale),
            Sense::hover(),
        );
        let o=whole.min;
        let panel=Rect::from_min_size(o,Vec2::new(PANEL_W*scale,PANEL_H*scale));

        let drawer_y=DRAWER_CLOSED_Y
            +(DRAWER_OPEN_Y-DRAWER_CLOSED_Y)*self.inside_slide;
        let drawer=Rect::from_min_size(
            o+Vec2::new(DRAWER_X*scale,drawer_y*scale),
            Vec2::new(DRAWER_W*scale,DRAWER_H*scale),
        );
        if let Some(t)=&self.tex.slideout {
            Self::image(ui,t,drawer);
        } else {
            ui.painter().rect_filled(drawer,8.0,Color32::from_rgb(74,76,74));
        }
        self.draw_inside_text(ui,drawer,scale);

        if let Some(t)=&self.tex.panel {
            Self::image(ui,t,panel);
        } else {
            ui.painter().rect_filled(panel,0.0,Color32::from_rgb(25,35,43));
        }

        for bit in 0..16 {
            let r=Rect::from_min_size(
                o+Vec2::new(SWITCH_X[bit]*scale,SWITCH_Y[bit]*scale),
                Vec2::new(32.0*scale,96.0*scale),
            );
            let response=ui.allocate_rect(r,Sense::click());
            if response.clicked() {
                self.machine.bus.panel_switches^=1u16<<bit;
                self.audio.play_once("assets/click.mp3");
            }
            let up=self.machine.bus.panel_switches&(1u16<<bit)!=0;
            if let Some(t)=if up {self.tex.switch_up.as_ref()} else {self.tex.switch_down.as_ref()} {
                Self::image(ui,t,r);
            }
        }

        for bit in 0..16 {
            self.led(
                ui,o,scale,ADDR_LED_X[bit],ADDR_LED_Y[bit],
                self.machine.address_leds&(1u16<<bit)!=0,
            );
        }
        for bit in 0..8 {
            self.led(
                ui,o,scale,DATA_LED_X[bit],DATA_LED_Y[bit],
                self.machine.bus.data_leds&(1u8<<bit)!=0,
            );
        }
        self.led(ui,o,scale,218.0,228.0,self.machine.wait_led);
        self.led(ui,o,scale,269.0,119.0,self.machine.current_board_protected());
        self.led(ui,o,scale,324.0,119.0,self.machine.powered);
        self.led(ui,o,scale,434.0,120.0,self.machine.powered);
        self.led(ui,o,scale,654.0,120.0,self.machine.powered);

        let power_rect=Rect::from_min_size(
            o+Vec2::new(114.0*scale,408.0*scale),
            Vec2::new(32.0*scale,96.0*scale),
        );
        let power_response=ui.allocate_rect(power_rect,Sense::click());
        if power_response.clicked() {
            self.set_altair_power(!self.machine.powered);
        }
        if let Some(t)=if self.machine.powered {self.tex.switch_down.as_ref()} else {self.tex.switch_up.as_ref()} {
            Self::image(ui,t,power_rect);
        }

        if let Some(down)=self.momentary(ui,o,scale,377.0,410.0,"RUN / STOP") {
            self.machine.set_running(down);
        }
        if self.momentary(ui,o,scale,486.0,410.0,"SINGLE STEP").is_some() {
            self.machine.step();
        }
        if let Some(down)=self.momentary(ui,o,scale,595.0,410.0,"EXAMINE / EXAMINE NEXT") {
            self.machine.examine(down);
        }
        if let Some(down)=self.momentary(ui,o,scale,704.0,410.0,"DEPOSIT / DEPOSIT NEXT") {
            self.machine.deposit(down);
        }
        if self.momentary(ui,o,scale,813.0,410.0,"RESET").is_some() {
            self.machine.reset();
            self.tty_tx_started=None;
            self.machine.address_leds=0xffff;
            self.machine.bus.data_leds=0xff;
            self.reset_flash_until=Some(Instant::now()+Duration::from_millis(500));
        }
        if let Some(down)=self.momentary(ui,o,scale,922.0,412.0,"PROTECT / UNPROTECT") {
            self.machine.protect_current_board(!down);
        }
        let _=self.momentary(ui,o,scale,1031.0,412.0,"AUX 1 (unassigned)");
        let _=self.momentary(ui,o,scale,1140.0,412.0,"AUX 2 (unassigned)");

        let handle=Rect::from_min_size(
            drawer.min+Vec2::new(450.0*scale,178.0*scale),
            Vec2::new(205.0*scale,57.0*scale),
        );
        let resp=ui.allocate_rect(handle,Sense::click());
        if resp.clicked()&&self.machine.powered {
            self.inside_open=!self.inside_open;
            self.audio.play_once("assets/click.mp3");
        }
        if resp.hovered() {
            resp.on_hover_text(if self.inside_open {
                "Hide processor state"
            } else {
                "Peek inside processor state"
            });
        }
    }

    fn play_print_events(&mut self,events:&[PrintEvent]) {
        for event in events {
            match event {
                PrintEvent::Printable=>{
                    self.audio.play_once("assets/printcharpadded.mp3");
                    self.print_head_raise_until=
                        Some(Instant::now()+Duration::from_millis(100));
                }
                PrintEvent::CarriageReturn=>{
                    self.audio.play_once("assets/crpadded.mp3");
                }
                PrintEvent::Bell=>{
                    self.audio.play_once("assets/bellpadded.mp3");
                }
            }
        }
    }

    fn set_tty_mode(&mut self,mode:TtyMode) {
        if mode==self.tty.mode { return; }
        self.tty.set_mode(mode);
        self.audio.play_once("assets/powerbtn.mp3");
        self.tty_power_flash_until=None;
        if mode==TtyMode::Off {
            self.audio.stop_loop("tty-motor");
        } else {
            self.audio.start_loop("tty-motor","assets/up-hum4.mp3");
        }
    }

    fn flash_tty_power(&mut self,ctx:&egui::Context) {
        self.tty_power_flash_until=Some(Instant::now()+Duration::from_secs(2));
        ctx.request_repaint_after(PANEL_FRAME);
    }

    fn send_tty_byte(&mut self,byte:u8) {
        if self.tty.mode==TtyMode::Off { return; }
        self.machine.bus.serial_rx.push_back(byte&0x7f);
        let events=self.tty.print_local(byte);
        self.play_print_events(&events);
    }

    fn process_tty_serial(&mut self,ctx:&egui::Context) {
        let now=Instant::now();

        if let Some(started)=self.tty_tx_started {
            if now.duration_since(started)>=TTY_CHAR_TIME {
                self.machine.bus.serial_tx.pop_front();
                self.tty_tx_started=None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(5));
                return;
            }
        }

        if self.tty_tx_started.is_none() {
            if let Some(&byte)=self.machine.bus.serial_tx.front() {
                let was_off=self.tty.mode==TtyMode::Off;
                let events=self.tty.print_serial(byte);
                if was_off&&self.tty.mode==TtyMode::Line {
                    self.audio.play_once("assets/powerbtn.mp3");
                    self.audio.start_loop("tty-motor","assets/up-hum4.mp3");
                }
                self.play_print_events(&events);
                self.tty_tx_started=Some(now);
                ctx.request_repaint_after(PANEL_FRAME);
            }
        }
    }

    fn key_index_for_byte(byte:u8)->Option<usize> {
        teletype::KEYS.iter().position(|key| {
            match key.kind {
                KeyKind::Character(_)=>{
                    teletype::key_to_byte(key.kind,false,false)==Some(byte)
                        ||teletype::key_to_byte(key.kind,true,false)==Some(byte)
                }
                KeyKind::Escape=>byte==0x1b,
                KeyKind::LineFeed=>byte==b'\n',
                KeyKind::CarriageReturn=>byte==b'\r',
                KeyKind::Delete=>byte==0x7f,
                KeyKind::Space=>byte==b' ',
                KeyKind::Control|KeyKind::Shift=>false,
            }
        })
    }

    fn animate_keyboard_byte(&mut self,byte:u8,ctx:&egui::Context) {
        if let Some(index)=Self::key_index_for_byte(byte) {
            self.animated_key=Some(index);
            self.pressed_key=Some(index);
            self.key_auto_release_at=Some(Instant::now()+KEY_TAP_TIME);
            self.key_displacement=0.0;
            self.key_anim_tick=Instant::now();
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    fn process_tty_keyboard(&mut self,ctx:&egui::Context) {
        let mut bytes=Vec::new();
        let mut any_key=false;

        ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Text(text)=>{
                        any_key=true;
                        for b in text.bytes() {
                            bytes.push(b.to_ascii_uppercase());
                        }
                    }
                    egui::Event::Key{key:egui::Key::Enter,pressed:true,..}=>{
                        any_key=true;
                        bytes.push(b'\r');
                    }
                    egui::Event::Key{key:egui::Key::Backspace,pressed:true,..}=>{
                        any_key=true;
                        bytes.push(0x7f);
                    }
                    egui::Event::Key{key:egui::Key::Escape,pressed:true,..}=>{
                        any_key=true;
                        bytes.push(0x1b);
                    }
                    egui::Event::Key{key,pressed:true,modifiers,..} if modifiers.ctrl=>{
                        any_key=true;
                        let letter=match key {
                            egui::Key::A=>Some(b'A'),egui::Key::B=>Some(b'B'),
                            egui::Key::C=>Some(b'C'),egui::Key::D=>Some(b'D'),
                            egui::Key::E=>Some(b'E'),egui::Key::F=>Some(b'F'),
                            egui::Key::G=>Some(b'G'),egui::Key::H=>Some(b'H'),
                            egui::Key::I=>Some(b'I'),egui::Key::J=>Some(b'J'),
                            egui::Key::K=>Some(b'K'),egui::Key::L=>Some(b'L'),
                            egui::Key::M=>Some(b'M'),egui::Key::N=>Some(b'N'),
                            egui::Key::O=>Some(b'O'),egui::Key::P=>Some(b'P'),
                            egui::Key::Q=>Some(b'Q'),egui::Key::R=>Some(b'R'),
                            egui::Key::S=>Some(b'S'),egui::Key::T=>Some(b'T'),
                            egui::Key::U=>Some(b'U'),egui::Key::V=>Some(b'V'),
                            egui::Key::W=>Some(b'W'),egui::Key::X=>Some(b'X'),
                            egui::Key::Y=>Some(b'Y'),egui::Key::Z=>Some(b'Z'),
                            _=>None,
                        };
                        if let Some(letter)=letter { bytes.push(letter-64); }
                    }
                    _=>{}
                }
            }
        });

        if self.tty.mode==TtyMode::Off {
            if any_key { self.flash_tty_power(ctx); }
            return;
        }

        for byte in bytes {
            self.animate_keyboard_byte(byte,ctx);
            self.send_tty_byte(byte);
        }
    }

    fn update_key_animation(&mut self,ctx:&egui::Context) {
        let now=Instant::now();
        if self.key_auto_release_at.is_some_and(|until|now>=until) {
            self.pressed_key=None;
            self.key_auto_release_at=None;
        }

        let dt=now.duration_since(self.key_anim_tick).as_secs_f32().min(0.05);
        self.key_anim_tick=now;
        let velocity=8.0/0.030;

        if self.pressed_key.is_some() {
            self.key_displacement=(self.key_displacement+velocity*dt).min(40.0);
        } else if self.key_displacement>0.0 {
            self.key_displacement=(self.key_displacement-velocity*dt).max(0.0);
            if self.key_displacement==0.0 { self.animated_key=None; }
        }

        if self.key_displacement>0.0||self.pressed_key.is_some() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    fn press_tty_key(&mut self,index:usize,ctx:&egui::Context) {
        if self.tty.mode==TtyMode::Off {
            self.flash_tty_power(ctx);
            return;
        }
        if self.pressed_key.is_some() { return; }

        self.pressed_key=Some(index);
        self.animated_key=Some(index);
        self.key_auto_release_at=None;
        self.key_displacement=0.0;
        self.key_anim_tick=Instant::now();

        let key=teletype::KEYS[index];
        match key.kind {
            KeyKind::Shift=>self.tty.shift_down=true,
            KeyKind::Control=>self.tty.control_down=true,
            kind=>{
                if let Some(byte)=teletype::key_to_byte(
                    kind,
                    self.tty.shift_down,
                    self.tty.control_down,
                ) {
                    self.send_tty_byte(byte);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(8));
    }

    fn release_tty_key(&mut self) {
        if let Some(index)=self.pressed_key.take() {
            match teletype::KEYS[index].kind {
                KeyKind::Shift=>self.tty.shift_down=false,
                KeyKind::Control=>self.tty.control_down=false,
                _=>{}
            }
        }
        self.key_auto_release_at=None;
    }

    fn draw_pressed_key(&self,ui:&mut egui::Ui,origin:Pos2,scale:f32) {
        let Some(index)=self.animated_key else { return; };
        if self.key_displacement<=0.0 { return; }

        let key=teletype::KEYS[index];

        let source=Rect::from_min_max(
            Pos2::new(key.x/TTY_W,key.y/TTY_H),
            Pos2::new((key.x+key.w)/TTY_W,(key.y+key.h+40.0)/TTY_H),
        );
        let target=Rect::from_min_size(
            origin+Vec2::new(key.x*scale,key.y*scale),
            Vec2::new(key.w*scale,(key.h+40.0)*scale),
        );
        if let Some(body)=&self.tex.tty_body {
            Self::image_uv(ui,body,target,source);
        }

        let key_source=Rect::from_min_max(
            Pos2::new(key.x/TTY_W,key.y/TTY_H),
            Pos2::new((key.x+key.w)/TTY_W,(key.y+key.h)/TTY_H),
        );
        let key_target=Rect::from_min_size(
            origin+Vec2::new(key.x*scale,(key.y+self.key_displacement)*scale),
            Vec2::new(key.w*scale,key.h*scale),
        );
        if let Some(keys)=&self.tex.tty_keys {
            Self::image_uv(ui,keys,key_target,key_source);
        }
    }

    fn draw_paper_text(&self,ui:&mut egui::Ui,paper:Rect,scale:f32) {
        let char_width=self.tty.char_width_image_px();
        let font_size=(char_width*1.63*scale).max(5.0);
        let line_height=(font_size*1.03).max(6.0);
        let max_lines=((paper.height()/line_height).floor() as usize).max(1);
        let lines:Vec<&str>=self.tty.output.split('\n').collect();
        let first=lines.len().saturating_sub(max_lines);
        let visible=&lines[first..];
        let painter=ui.painter().with_clip_rect(paper);

        for (row,line) in visible.iter().enumerate() {
            let from_bottom=visible.len()-1-row;
            let y=paper.bottom()-from_bottom as f32*line_height;
            painter.text(
                Pos2::new(paper.left(),y),
                egui::Align2::LEFT_BOTTOM,
                *line,
                FontId::new(font_size,FontFamily::Name("teletype".into())),
                Color32::from_rgb(35,35,30),
            );
        }
    }

    fn draw_teletype(&mut self,ui:&mut egui::Ui) {
        let available=ui.available_size();
        let scale=(available.x/TTY_W).min(available.y/TTY_H).clamp(0.12,1.5);
        let (rect,response)=ui.allocate_exact_size(
            Vec2::new(TTY_W*scale,TTY_H*scale),
            Sense::click_and_drag(),
        );
        let origin=rect.min;

        if let Some(t)=&self.tex.tty_body {
            Self::image(ui,t,rect);
        } else {
            ui.painter().rect_filled(rect,0.0,Color32::from_rgb(80,76,65));
        }
        if let Some(t)=&self.tex.tty_keys { Self::image(ui,t,rect); }

        // In the web original, #printoutholder is positioned at top:34%,
        // but the <pre> itself is absolutely positioned with bottom:0. That
        // makes 34% the current print-line baseline, not the top of the paper.
        // The text therefore grows upward from the platen.
        let paper=Rect::from_min_max(
            origin+Vec2::new(teletype::PRINT_LEFT*scale,0.0),
            origin+Vec2::new(
                (teletype::PRINT_LEFT+teletype::PRINTABLE_WIDTH)*scale,
                teletype::PRINT_TOP*scale,
            ),
        );
        self.draw_paper_text(ui,paper,scale);

        if self.tty.mode!=TtyMode::Off {
            if let Some(t)=&self.tex.tty_head {
                let char_width=self.tty.char_width_image_px();
                let x=teletype::PRINT_LEFT
                    +self.tty.column as f32*char_width
                    -char_width;
                let raised=self.print_head_raise_until
                    .is_some_and(|until|Instant::now()<until);
                let y=teletype::PRINT_HEAD_TOP-if raised {TTY_H*0.02} else {0.0};
                let head_rect=Rect::from_min_size(
                    origin+Vec2::new(x*scale,y*scale),
                    Vec2::new(
                        TTY_W*0.06*scale,
                        TTY_H*(if raised {0.075} else {0.10})*scale,
                    ),
                );
                Self::image(ui,t,head_rect);
            }
        }

        let selector_size=Vec2::new(
            TTY_W*0.18*scale,
            288.0*(TTY_W*0.18/349.0)*scale,
        );
        let mut selector=Rect::from_min_size(
            Pos2::new(rect.right()-selector_size.x,rect.bottom()-selector_size.y),
            selector_size,
        );

        let flashing=self.tty_power_flash_until
            .is_some_and(|until|Instant::now()<until);
        if flashing {
            let remaining=self.tty_power_flash_until
                .and_then(|until|until.checked_duration_since(Instant::now()))
                .map(|d|d.as_secs_f32())
                .unwrap_or(0.0);
            let phase=2.0-remaining;
            let grow=1.0+0.06*(phase*8.0).sin().abs();
            selector=Rect::from_center_size(selector.center(),selector.size()*grow);
            ui.ctx().request_repaint_after(PANEL_FRAME);
        }

        if let Some(t)=&self.tex.tty_line_local { Self::image(ui,t,selector); }

        if response.is_pointer_button_down_on() {
            if let Some(pointer)=response.interact_pointer_pos() {
                if selector.contains(pointer) {
                    let xp=(pointer.x-selector.left())/selector.width();
                    let yp=(pointer.y-selector.top())/selector.height();
                    if yp<0.52 {
                        self.set_tty_mode(TtyMode::Off);
                    } else if xp<0.40&&yp>0.40&&yp<0.80 {
                        self.set_tty_mode(TtyMode::Line);
                    } else if xp>0.56&&yp>0.40&&yp<0.80 {
                        self.set_tty_mode(TtyMode::Local);
                    }
                } else if self.pressed_key.is_none() {
                    let ix=(pointer.x-rect.left())/scale;
                    let iy=(pointer.y-rect.top())/scale;
                    if let Some(index)=teletype::KEYS.iter().position(|k|k.contains(ix,iy)) {
                        self.press_tty_key(index,ui.ctx());
                    }
                }
            }
        }

        let pointer_down=ui.ctx().input(|i|i.pointer.any_down());
        if !pointer_down&&self.key_auto_release_at.is_none() {
            self.release_tty_key();
        }
        self.draw_pressed_key(ui,origin,scale);

        if !flashing {
            if let Some(t)=&self.tex.tty_knob {
                // asrlinelocal.png already contains the static knob. The
                // transparent asrlinelocalknob.png must sit exactly over it;
                // movement is rotation around its centre, not translation.
                let knob_w=TTY_W*0.06*scale;
                let knob_h=knob_w*117.0/116.0;
                let knob_rect=Rect::from_min_size(
                    Pos2::new(
                        rect.right()-TTY_W*0.06*scale-knob_w,
                        rect.bottom()-TTY_H*0.022*scale-knob_h,
                    ),
                    Vec2::new(knob_w,knob_h),
                );
                let target_angle=match self.tty.mode {
                    TtyMode::Line=>-std::f32::consts::FRAC_PI_2,
                    TtyMode::Off=>0.0,
                    TtyMode::Local=>std::f32::consts::FRAC_PI_2,
                };
                let angle=ui.ctx().animate_value_with_time(
                    egui::Id::new("asr33-selector-knob-angle"),
                    target_angle,
                    0.5,
                );
                egui::Image::new(t)
                    .rotate(angle,Vec2::splat(0.5))
                    .paint_at(ui,knob_rect);
            }
        }

        if !self.tty.tape_in.is_empty()||self.tty.capture_to_tape {
            let tape=Rect::from_min_size(
                Pos2::new(rect.left()+18.0*scale,rect.bottom()-250.0*scale),
                Vec2::new(520.0*scale,115.0*scale),
            );
            ui.painter().rect_filled(tape,3.0,Color32::from_rgb(224,210,160));
            let n=if self.tty.capture_to_tape {
                self.tty.tape_out.len()
            } else {
                self.tty.tape_in.len()
            };
            ui.painter().text(
                tape.center(),
                egui::Align2::CENTER_CENTER,
                if self.tty.capture_to_tape {
                    format!("PUNCHING PAPER TAPE  {n} bytes")
                } else {
                    format!("READING PAPER TAPE  {n} bytes")
                },
                FontId::monospace((22.0*scale).max(8.0)),
                Color32::from_rgb(45,42,34),
            );
        }
    }

    fn update_paper_tape(&mut self) {
        if self.last_tape_tick.elapsed()<Duration::from_millis(30) { return; }
        self.last_tape_tick=Instant::now();
        if self.machine.bus.serial_rx.is_empty() {
            if let Some(byte)=self.tty.next_tape_byte() {
                self.machine.bus.serial_rx.push_back(byte);
            }
        }
    }

    fn load_paper_tape(&mut self) {
        let Some(path)=rfd::FileDialog::new()
            .add_filter("Paper tape",&["txt","tap","bin"])
            .pick_file()
        else { return; };
        match std::fs::read(&path) {
            Ok(bytes)=>{
                self.tty.load_tape(&bytes);
                self.status=format!("Paper tape loaded: {} bytes",bytes.len());
            }
            Err(e)=>self.status=format!("Paper tape load failed: {e}"),
        }
    }

    fn save_punched_tape(&mut self) {
        let Some(path)=rfd::FileDialog::new()
            .set_file_name("myPaperTape.txt")
            .save_file()
        else { return; };
        match std::fs::write(&path,&self.tty.tape_out) {
            Ok(_)=>self.status=format!("Punched tape saved: {} bytes",self.tty.tape_out.len()),
            Err(e)=>self.status=format!("Paper tape save failed: {e}"),
        }
    }

    fn load_bundled_basic(&mut self) {
        match std::fs::read("assets/4kbas32.bin") {
            Ok(bytes)=>{
                if !self.machine.powered {
                    self.set_altair_power(true);
                } else {
                    self.machine.set_running(false);
                    self.machine.reset();
                }
                self.tty_tx_started=None;
                self.machine.bus.clear_protection();
                self.machine.bus.load(0,&bytes);
                self.machine.cpu.pc=0;
                self.tty_window_open=true;
                self.machine.set_running(true);
                self.status="Microsoft 4K BASIC loaded and running".into();
            }
            Err(e)=>self.status=format!("4K BASIC asset missing: {e}"),
        }
    }

    fn draw_tty_menu(&mut self,ctx:&egui::Context) {
        self.process_tty_keyboard(ctx);
        egui::TopBottomPanel::top("tty-menu").show(ctx,|ui| {
            egui::MenuBar::new().ui(ui,|ui| {
                ui.label("POWER:");
                if ui.selectable_label(self.tty.mode==TtyMode::Off,"OFF").clicked() {
                    self.set_tty_mode(TtyMode::Off);
                }
                if ui.selectable_label(self.tty.mode==TtyMode::Line,"LINE").clicked() {
                    self.set_tty_mode(TtyMode::Line);
                }
                if ui.selectable_label(self.tty.mode==TtyMode::Local,"LOCAL").clicked() {
                    self.set_tty_mode(TtyMode::Local);
                }
                ui.separator();
                ui.selectable_value(&mut self.tty.paper_width,52,"Large");
                ui.selectable_value(&mut self.tty.paper_width,82,"Normal");
                ui.separator();
                if ui.button("Clear paper").clicked() { self.tty.clear_paper(); }
                if ui.button("Read tape…").clicked() { self.load_paper_tape(); }
                let punch_label=if self.tty.capture_to_tape {"Finish punch"} else {"Punch tape"};
                if ui.button(punch_label).clicked() {
                    if self.tty.capture_to_tape {
                        self.tty.capture_to_tape=false;
                        self.save_punched_tape();
                    } else {
                        self.tty.tape_out.clear();
                        self.tty.capture_to_tape=true;
                    }
                }
            });
        });
    }

    fn draw_tty_window(&mut self,ctx:&egui::Context) {
        self.update_key_animation(ctx);
        self.draw_tty_menu(ctx);

        if self.print_head_raise_until.is_some_and(|until|Instant::now()<until) {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
        if self.tty_power_flash_until.is_some_and(|until|Instant::now()<until) {
            ctx.request_repaint_after(PANEL_FRAME);
        }

        egui::CentralPanel::default().show(ctx,|ui| {
            ui.centered_and_justified(|ui|self.draw_teletype(ui));
        });
        egui::TopBottomPanel::bottom("tty-status").show(ctx,|ui| {
            ui.small(format!(
                "ASR-33 {}  |  RX {}  |  TX {}  |  column {}",
                match self.tty.mode {
                    TtyMode::Off=>"OFF",
                    TtyMode::Line=>"LINE",
                    TtyMode::Local=>"LOCAL",
                },
                self.machine.bus.serial_rx.len(),
                if self.machine.bus.tx_busy() {"BUSY"} else {"READY"},
                self.tty.column,
            ));
        });
    }

    fn show_tty_viewport(&mut self,parent_ctx:&egui::Context) {
        if !self.tty_window_open { return; }
        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-asr33"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — ASR-33 Teletype")
                .with_inner_size([820.0,820.0])
                .with_min_inner_size([520.0,520.0])
                .with_resizable(true),
            |tty_ctx,_class| {
                self.draw_tty_window(tty_ctx);
                if tty_ctx.input(|i|i.viewport().close_requested()) {
                    self.tty_window_open=false;
                    self.set_tty_mode(TtyMode::Off);
                }
            },
        );
    }
}

impl eframe::App for RusTairApp {
    fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame) {
        let now=Instant::now();
        let dt=now.duration_since(self.last_tick).min(Duration::from_millis(50));
        self.last_tick=now;

        self.update_paper_tape();

        let target=if self.inside_open&&self.machine.powered {1.0} else {0.0};
        let step=(dt.as_secs_f32()/0.5).clamp(0.0,1.0);
        self.inside_slide+=(target-self.inside_slide)*step*4.0;
        if (target-self.inside_slide).abs()>0.002 {
            ctx.request_repaint_after(PANEL_FRAME);
        } else {
            self.inside_slide=target;
        }

        if let Some(until)=self.reset_flash_until {
            if now>=until {
                self.machine.address_leds=0;
                self.machine.bus.data_leds=0;
                self.reset_flash_until=None;
            } else {
                ctx.request_repaint_after(PANEL_FRAME);
            }
        }

        if self.machine.running {
            let cycles=(CLOCK_HZ as f64*dt.as_secs_f64())as u32;
            self.machine.run_cycles(cycles.clamp(1,200_000));
            ctx.request_repaint_after(PANEL_FRAME);
        }

        self.process_tty_serial(ctx);

        egui::TopBottomPanel::top("menu").show(ctx,|ui| {
            egui::MenuBar::new().ui(ui,|ui| {
                ui.menu_button("File",|ui| {
                    if ui.button("Load binary…").clicked() {
                        if let Some(path)=rfd::FileDialog::new().pick_file() {
                            match std::fs::read(&path) {
                                Ok(bytes)=>{
                                    self.machine.bus.load(0,&bytes);
                                    self.status=format!(
                                        "Loaded {} bytes from {}",
                                        bytes.len(),
                                        path.display()
                                    );
                                }
                                Err(e)=>self.status=format!("Load failed: {e}"),
                            }
                        }
                        ui.close();
                    }
                    if ui.button("Load bundled Microsoft 4K BASIC").clicked() {
                        self.load_bundled_basic();
                        ui.close();
                    }
                });

                ui.separator();
                if ui.button("ASR-33 TELETYPE").clicked() {
                    self.tty_window_open=true;
                }
                if self.machine.powered {
                    ui.separator();
                    if ui.selectable_label(self.inside_open,"INSIDE").clicked() {
                        self.inside_open=!self.inside_open;
                    }
                }

                ui.separator();
                let mut muted=self.audio.muted();
                if ui.checkbox(&mut muted,"Mute").changed() {
                    self.audio.set_muted(muted);
                }
                ui.separator();
                ui.label(format!(
                    "PC {:04X}  SP {:04X}  A {:02X}  F {:02X}",
                    self.machine.cpu.pc,
                    self.machine.cpu.sp,
                    self.machine.cpu.a,
                    self.machine.cpu.f
                ));
                ui.separator();
                ui.label(if self.machine.running {
                    "RUNNING"
                } else if self.machine.powered {
                    "STOPPED"
                } else {
                    "POWER OFF"
                });
            });
        });

        egui::CentralPanel::default().show(ctx,|ui| {
            ui.centered_and_justified(|ui|self.draw_altair(ui));
        });
        egui::TopBottomPanel::bottom("status").show(ctx,|ui| {
            ui.small(&self.status);
        });

        self.show_tty_viewport(ctx);
    }
}

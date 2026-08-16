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

const SWITCH_X: [f32; 16] = [1332.,1278.,1224.,1142.,1087.,1032.,950.,895.,840.,758.,703.,648.,566.,512.,457.,376.];
const SWITCH_Y: [f32; 16] = [305.,305.,305.,305.,303.,303.,303.,303.,303.,303.,303.,301.,301.,301.,301.,301.];
const ADDR_LED_X: [f32; 16] = [1341.,1286.,1231.,1148.,1093.,1037.,955.,900.,845.,763.,708.,653.,573.,518.,463.,381.];
const ADDR_LED_Y: [f32; 16] = [233.,233.,233.,233.,231.,231.,231.,230.,230.,230.,230.,230.,229.,229.,229.,229.];
const DATA_LED_X: [f32; 8] = [1341.,1286.,1231.,1148.,1093.,1037.,955.,900.];
const DATA_LED_Y: [f32; 8] = [122.,122.,122.,122.,120.,120.,120.,120.];

#[derive(Clone, Copy, PartialEq, Eq)]
enum View { Altair, Teletype }

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
            .with_title("RusTair — MITS Altair 8800 + ASR-33")
            .with_inner_size([1400.0, 820.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("RusTair", options, Box::new(|cc| Ok(Box::new(RusTairApp::new(cc)))))
}

struct RusTairApp {
    machine: AltairMachine,
    tex: Tex,
    view: View,
    tty: Teletype,
    audio: AudioEngine,
    last_tick: Instant,
    last_tape_tick: Instant,
    reset_flash_until: Option<Instant>,
    animated_key: Option<usize>,
    pressed_key: Option<usize>,
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
        Some(ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw()), egui::TextureOptions::LINEAR))
    }

    fn install_teletype_font(ctx: &egui::Context) {
        let Ok(bytes) = std::fs::read("assets/teletype.ttf") else { return };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("teletype".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.insert(FontFamily::Name("teletype".into()), vec!["teletype".to_owned()]);
        ctx.set_fonts(fonts);
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self::install_teletype_font(&cc.egui_ctx);
        let now = Instant::now();
        Self {
            machine: AltairMachine::default(),
            tex: Tex {
                panel: Self::load_texture(&cc.egui_ctx,"altair-panel","assets/Altair1.png"),
                slideout: Self::load_texture(&cc.egui_ctx,"altair-slideout","assets/slideout.png"),
                led_on: Self::load_texture(&cc.egui_ctx,"led-on","assets/LEDon.png"),
                switch_up: Self::load_texture(&cc.egui_ctx,"switch-up","assets/SwitchUp.png"),
                switch_down: Self::load_texture(&cc.egui_ctx,"switch-down","assets/SwitchDown.png"),
                switch_centre: Self::load_texture(&cc.egui_ctx,"switch-centre","assets/SwitchCentre.png"),
                tty_body: Self::load_texture(&cc.egui_ctx,"tty-body","assets/asr33 body.jpg"),
                tty_keys: Self::load_texture(&cc.egui_ctx,"tty-keys","assets/asr33 keys.png"),
                tty_head: Self::load_texture(&cc.egui_ctx,"tty-head","assets/asr33head.png"),
                tty_line_local: Self::load_texture(&cc.egui_ctx,"tty-line-local","assets/asrlinelocal.png"),
                tty_knob: Self::load_texture(&cc.egui_ctx,"tty-knob","assets/asrlinelocalknob.png"),
            },
            view: View::Altair,
            tty: Teletype::default(),
            audio: AudioEngine::new(),
            last_tick: now,
            last_tape_tick: now,
            reset_flash_until: None,
            animated_key: None,
            pressed_key: None,
            key_displacement: 0.0,
            key_anim_tick: now,
            inside_open: false,
            inside_slide: 0.0,
            status: "Ready".into(),
        }
    }

    fn image(ui: &mut egui::Ui, t: &egui::TextureHandle, r: Rect) {
        Self::image_uv(ui, t, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.,1.)));
    }

    fn image_uv(ui: &mut egui::Ui, t: &egui::TextureHandle, r: Rect, uv: Rect) {
        ui.painter().image(t.id(), r, uv, Color32::WHITE);
    }

    fn led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, on: bool) {
        if !self.machine.powered || !on { return; }
        let r = Rect::from_min_size(origin + Vec2::new(x*scale,y*scale), Vec2::splat(24.*scale));
        if let Some(t)=&self.tex.led_on { Self::image(ui,t,r); }
        else { ui.painter().circle_filled(r.center(),8.*scale,Color32::from_rgb(255,45,20)); }
    }

    fn momentary(&mut self, ui:&mut egui::Ui, origin:Pos2, scale:f32, x:f32, y:f32, label:&str) -> Option<bool> {
        let r=Rect::from_min_size(origin+Vec2::new(x*scale,y*scale),Vec2::new(32.*scale,96.*scale));
        let resp=ui.allocate_rect(r,Sense::click());
        let down=resp.interact_pointer_pos().map(|p| p.y>r.center().y).unwrap_or(false);
        let tex=if resp.is_pointer_button_down_on(){if down{self.tex.switch_down.as_ref()}else{self.tex.switch_up.as_ref()}}else{self.tex.switch_centre.as_ref()};
        if let Some(t)=tex { Self::image(ui,t,r); }
        if resp.hovered(){ resp.clone().on_hover_text(label); }
        if resp.clicked(){self.audio.play_once("assets/click.mp3");Some(down)}else{None}
    }

    fn set_altair_power(&mut self, on: bool) {
        self.machine.power(on);
        self.audio.play_once("assets/powerbtn.mp3");
        if on { self.audio.start_loop("altair-fan", "assets/fan.mp3"); }
        else { self.audio.stop_loop("altair-fan"); self.inside_open=false; }
    }

    fn flags_text(&self) -> String {
        let f=self.machine.cpu.f;
        let mut s=Vec::new();
        if f&0x80!=0{s.push("S");}
        if f&0x40!=0{s.push("Z");}
        if f&0x10!=0{s.push("AC");}
        if f&0x04!=0{s.push("P");}
        if f&0x01!=0{s.push("C");}
        if s.is_empty(){"-".into()}else{s.join(" ")}
    }

    fn draw_inside_text(&self, ui:&mut egui::Ui, d:Rect, scale:f32) {
        if self.inside_slide<0.72{return;}
        let p=ui.painter();
        let mono=|px:f32| FontId::monospace((px*scale).max(7.0));
        let ink=Color32::from_rgb(12,25,29);
        let accent=Color32::from_rgb(220,235,190);
        let x=d.left()+38.*scale;
        let y=d.top()+28.*scale;
        p.text(Pos2::new(x,y),egui::Align2::LEFT_TOP,"REGISTERS",mono(14.),ink);
        p.text(Pos2::new(d.left()+370.*scale,y),egui::Align2::LEFT_TOP,"RANDOM ACCESS MEMORY (RAM)",mono(14.),ink);

        let c=&self.machine.cpu;
        let regs=[
            format!("A  {:08b}  ${:02X}    F  {:08b}  ${:02X}",c.a,c.a,c.f,c.f),
            format!("B  {:08b}  ${:02X}    C  {:08b}  ${:02X}",c.b,c.b,c.c,c.c),
            format!("D  {:08b}  ${:02X}    E  {:08b}  ${:02X}",c.d,c.d,c.e,c.e),
            format!("H  {:08b}  ${:02X}    L  {:08b}  ${:02X}",c.h,c.h,c.l,c.l),
            format!("SP {:016b}  ${:04X}",c.sp,c.sp),
            format!("PC {:016b}  ${:04X}",c.pc,c.pc),
        ];
        for (i,line) in regs.iter().enumerate(){
            p.text(Pos2::new(x,y+(30.+i as f32*24.)*scale),egui::Align2::LEFT_TOP,line,mono(11.),accent);
        }
        p.text(Pos2::new(x,y+174.*scale),egui::Align2::LEFT_TOP,format!("FLAGS  {}",self.flags_text()),mono(11.),accent);
        let pc=c.pc as usize;
        let m=&self.machine.bus.memory;
        let op=m.get(pc).copied().unwrap_or(0);
        let b1=m.get(pc.wrapping_add(1)).copied().unwrap_or(0);
        let b2=m.get(pc.wrapping_add(2)).copied().unwrap_or(0);
        p.text(Pos2::new(x,y+195.*scale),egui::Align2::LEFT_TOP,format!("NEXT   {}",disasm::disassemble(op,b1,b2)),mono(11.),accent);

        let start=(pc&!0x0f).saturating_sub(32).min(machine::MEM_SIZE.saturating_sub(16));
        for row in 0..7 {
            let addr=start+row*16;
            if addr>=machine::MEM_SIZE{break;}
            let mut line=format!("{addr:04X}   ");
            for col in 0..16 { if let Some(v)=m.get(addr+col){line.push_str(&format!("{v:02X} "));} }
            p.text(Pos2::new(d.left()+370.*scale,y+(30.+row as f32*22.)*scale),egui::Align2::LEFT_TOP,line,mono(10.),accent);
        }
    }

    fn draw_altair(&mut self, ui:&mut egui::Ui) {
        let a=ui.available_size();
        let total_h=PANEL_H+229.0;
        let scale=(a.x/PANEL_W).min(a.y/total_h).clamp(0.2,2.5);
        let (whole,_)=ui.allocate_exact_size(Vec2::new(PANEL_W*scale,total_h*scale),Sense::hover());
        let o=whole.min;
        let r=Rect::from_min_size(o,Vec2::new(PANEL_W*scale,PANEL_H*scale));

        let drawer_y=DRAWER_CLOSED_Y+(DRAWER_OPEN_Y-DRAWER_CLOSED_Y)*self.inside_slide;
        let drawer=Rect::from_min_size(o+Vec2::new(DRAWER_X*scale,drawer_y*scale),Vec2::new(DRAWER_W*scale,DRAWER_H*scale));
        if let Some(t)=&self.tex.slideout{Self::image(ui,t,drawer);}else{ui.painter().rect_filled(drawer,8.,Color32::from_rgb(74,76,74));}
        self.draw_inside_text(ui,drawer,scale);

        if let Some(t)=&self.tex.panel { Self::image(ui,t,r); }
        else { ui.painter().rect_filled(r,0.,Color32::from_rgb(25,35,43)); }

        for bit in 0..16 {
            let sr=Rect::from_min_size(o+Vec2::new(SWITCH_X[bit]*scale,SWITCH_Y[bit]*scale),Vec2::new(32.*scale,96.*scale));
            let resp=ui.allocate_rect(sr,Sense::click());
            if resp.clicked(){self.machine.bus.panel_switches^=1u16<<bit;self.audio.play_once("assets/click.mp3");}
            let up=self.machine.bus.panel_switches&(1u16<<bit)!=0;
            if let Some(t)=if up{self.tex.switch_up.as_ref()}else{self.tex.switch_down.as_ref()}{Self::image(ui,t,sr);}
        }

        for bit in 0..16{self.led(ui,o,scale,ADDR_LED_X[bit],ADDR_LED_Y[bit],self.machine.address_leds&(1u16<<bit)!=0);}
        for bit in 0..8{self.led(ui,o,scale,DATA_LED_X[bit],DATA_LED_Y[bit],self.machine.bus.data_leds&(1u8<<bit)!=0);}
        self.led(ui,o,scale,218.,228.,self.machine.wait_led);
        self.led(ui,o,scale,324.,119.,self.machine.powered);
        self.led(ui,o,scale,434.,120.,self.machine.powered);
        self.led(ui,o,scale,654.,120.,self.machine.powered);

        let pr=Rect::from_min_size(o+Vec2::new(114.*scale,408.*scale),Vec2::new(32.*scale,96.*scale));
        let power_response=ui.allocate_rect(pr,Sense::click());
        if power_response.clicked(){self.set_altair_power(!self.machine.powered);}
        if let Some(t)=if self.machine.powered{self.tex.switch_down.as_ref()}else{self.tex.switch_up.as_ref()}{Self::image(ui,t,pr);}

        if let Some(d)=self.momentary(ui,o,scale,377.,410.,"RUN / STOP"){self.machine.set_running(d);}
        if self.momentary(ui,o,scale,486.,410.,"SINGLE STEP").is_some(){self.machine.step();}
        if let Some(d)=self.momentary(ui,o,scale,595.,410.,"EXAMINE / EXAMINE NEXT"){self.machine.examine(d);}
        if let Some(d)=self.momentary(ui,o,scale,704.,410.,"DEPOSIT / DEPOSIT NEXT"){self.machine.deposit(d);}
        if self.momentary(ui,o,scale,813.,410.,"RESET").is_some(){
            self.machine.reset();self.machine.address_leds=0xffff;self.machine.bus.data_leds=0xff;
            self.reset_flash_until=Some(Instant::now()+Duration::from_millis(500));
        }

        let handle=Rect::from_min_size(drawer.min+Vec2::new(450.*scale,178.*scale),Vec2::new(205.*scale,57.*scale));
        let hresp=ui.allocate_rect(handle,Sense::click());
        if hresp.clicked() && self.machine.powered{self.inside_open=!self.inside_open;self.audio.play_once("assets/click.mp3");}
        if hresp.hovered(){hresp.on_hover_text(if self.inside_open{"Hide processor state"}else{"Peek inside processor state"});}
    }

    fn play_print_events(&self, events: &[PrintEvent]) {
        for event in events { match event {
            PrintEvent::Printable => self.audio.play_once("assets/printcharpadded.mp3"),
            PrintEvent::CarriageReturn => self.audio.play_once("assets/crpadded.mp3"),
            PrintEvent::Bell => self.audio.play_once("assets/bellpadded.mp3"),
        }}
    }

    fn set_tty_mode(&mut self, mode:TtyMode) {
        if mode==self.tty.mode { return; }
        self.tty.set_mode(mode);self.audio.play_once("assets/powerbtn.mp3");
        if mode==TtyMode::Off { self.audio.stop_loop("tty-motor"); }
        else { self.audio.start_loop("tty-motor", "assets/up-hum4.mp3"); }
    }

    fn send_tty_byte(&mut self, byte:u8) {
        if self.tty.mode==TtyMode::Off { return; }
        self.machine.bus.serial_rx.push_back(byte & 0x7f);
        let events=self.tty.print_local(byte);self.play_print_events(&events);
    }

    fn process_tty_io(&mut self, ctx:&egui::Context) {
        while let Some(b)=self.machine.bus.serial_tx.pop_front(){
            let was_off=self.tty.mode==TtyMode::Off;let events=self.tty.print_serial(b);
            if was_off && self.tty.mode==TtyMode::Line {self.audio.play_once("assets/powerbtn.mp3");self.audio.start_loop("tty-motor", "assets/up-hum4.mp3");}
            self.play_print_events(&events);
        }
        if self.view!=View::Teletype || self.tty.mode==TtyMode::Off { return; }
        let mut bytes=Vec::new();
        ctx.input(|i| for e in &i.events { match e {
            egui::Event::Text(s) => bytes.extend(s.bytes().map(|b|b.to_ascii_uppercase())),
            egui::Event::Key{key:egui::Key::Enter,pressed:true,..} => bytes.push(b'\r'),
            egui::Event::Key{key:egui::Key::Backspace,pressed:true,..} => bytes.push(0x7f),
            egui::Event::Key{key:egui::Key::Escape,pressed:true,..} => bytes.push(0x1b),
            egui::Event::Key{key,pressed:true,modifiers,..} if modifiers.ctrl => {
                let letter=match key {
                    egui::Key::A=>Some(b'A'),egui::Key::B=>Some(b'B'),egui::Key::C=>Some(b'C'),egui::Key::D=>Some(b'D'),
                    egui::Key::E=>Some(b'E'),egui::Key::F=>Some(b'F'),egui::Key::G=>Some(b'G'),egui::Key::H=>Some(b'H'),
                    egui::Key::I=>Some(b'I'),egui::Key::J=>Some(b'J'),egui::Key::K=>Some(b'K'),egui::Key::L=>Some(b'L'),
                    egui::Key::M=>Some(b'M'),egui::Key::N=>Some(b'N'),egui::Key::O=>Some(b'O'),egui::Key::P=>Some(b'P'),
                    egui::Key::Q=>Some(b'Q'),egui::Key::R=>Some(b'R'),egui::Key::S=>Some(b'S'),egui::Key::T=>Some(b'T'),
                    egui::Key::U=>Some(b'U'),egui::Key::V=>Some(b'V'),egui::Key::W=>Some(b'W'),egui::Key::X=>Some(b'X'),
                    egui::Key::Y=>Some(b'Y'),egui::Key::Z=>Some(b'Z'),_=>None,
                };if let Some(b)=letter { bytes.push(b-64); }
            }
            _=>{}
        }});
        for byte in bytes { self.send_tty_byte(byte); }
    }

    fn update_key_animation(&mut self, ctx:&egui::Context) {
        let now=Instant::now();let dt=now.duration_since(self.key_anim_tick).as_secs_f32().min(0.05);self.key_anim_tick=now;
        let velocity=8.0/0.030;
        if self.pressed_key.is_some(){self.key_displacement=(self.key_displacement+velocity*dt).min(40.0);}
        else if self.key_displacement>0.0{self.key_displacement=(self.key_displacement-velocity*dt).max(0.0);if self.key_displacement==0.0{self.animated_key=None;}}
        if self.key_displacement>0.0 { ctx.request_repaint(); }
    }

    fn press_tty_key(&mut self, index:usize) {
        if self.tty.mode==TtyMode::Off || self.pressed_key.is_some(){return;}
        self.pressed_key=Some(index);self.animated_key=Some(index);let key=teletype::KEYS[index];
        match key.kind {
            KeyKind::Shift=>self.tty.shift_down=true,KeyKind::Control=>self.tty.control_down=true,
            kind=>if let Some(byte)=teletype::key_to_byte(kind,self.tty.shift_down,self.tty.control_down){self.send_tty_byte(byte);}
        }
    }

    fn release_tty_key(&mut self) {
        if let Some(index)=self.pressed_key.take(){match teletype::KEYS[index].kind{KeyKind::Shift=>self.tty.shift_down=false,KeyKind::Control=>self.tty.control_down=false,_=>{}}}
    }

    fn draw_pressed_key(&self, ui:&mut egui::Ui, origin:Pos2, scale:f32) {
        let Some(index)=self.animated_key else{return};if self.key_displacement<=0.0{return;}let k=teletype::KEYS[index];
        let source=Rect::from_min_max(Pos2::new(k.x/TTY_W,k.y/TTY_H),Pos2::new((k.x+k.w)/TTY_W,(k.y+k.h+40.0)/TTY_H));
        let target=Rect::from_min_size(origin+Vec2::new(k.x*scale,k.y*scale),Vec2::new(k.w*scale,(k.h+40.0)*scale));
        if let Some(body)=&self.tex.tty_body { Self::image_uv(ui,body,target,source); }
        let key_source=Rect::from_min_max(Pos2::new(k.x/TTY_W,k.y/TTY_H),Pos2::new((k.x+k.w)/TTY_W,(k.y+k.h)/TTY_H));
        let key_target=Rect::from_min_size(origin+Vec2::new(k.x*scale,(k.y+self.key_displacement)*scale),Vec2::new(k.w*scale,k.h*scale));
        if let Some(keys)=&self.tex.tty_keys { Self::image_uv(ui,keys,key_target,key_source); }
    }

    fn draw_teletype(&mut self, ui:&mut egui::Ui) {
        let a=ui.available_size();let scale=(a.x/TTY_W).min(a.y/TTY_H).clamp(0.12,1.5);
        let (r,resp)=ui.allocate_exact_size(Vec2::new(TTY_W*scale,TTY_H*scale),Sense::click_and_drag());let o=r.min;
        if let Some(t)=&self.tex.tty_body{Self::image(ui,t,r);}else{ui.painter().rect_filled(r,0.,Color32::from_rgb(80,76,65));}
        if let Some(t)=&self.tex.tty_keys{Self::image(ui,t,r);}
        let paper=Rect::from_min_size(o+Vec2::new(teletype::PRINT_LEFT*scale,teletype::PRINT_TOP*scale),Vec2::new(teletype::PRINTABLE_WIDTH*scale,TTY_H*0.27*scale));
        let char_width=self.tty.char_width_image_px();let fs=(char_width*1.63*scale).max(5.0);let family=FontFamily::Name("teletype".into());
        ui.painter().text(paper.left_bottom(),egui::Align2::LEFT_BOTTOM,&self.tty.output,FontId::new(fs,family),Color32::from_rgb(35,35,30));
        if self.tty.mode!=TtyMode::Off {if let Some(t)=&self.tex.tty_head{let x=teletype::PRINT_LEFT+(self.tty.column as f32)*char_width-char_width;let hr=Rect::from_min_size(o+Vec2::new(x*scale,teletype::PRINT_HEAD_TOP*scale),Vec2::new(TTY_W*0.06*scale,TTY_H*0.10*scale));Self::image(ui,t,hr);}}
        let lls=Vec2::new(TTY_W*0.18*scale,288.*(TTY_W*0.18/349.)*scale);let ll=Rect::from_min_size(Pos2::new(r.right()-lls.x,r.bottom()-lls.y),lls);if let Some(t)=&self.tex.tty_line_local{Self::image(ui,t,ll);}
        if resp.drag_started() || (resp.is_pointer_button_down_on() && self.pressed_key.is_none()) {if let Some(p)=resp.interact_pointer_pos(){if ll.contains(p){let xp=(p.x-ll.left())/ll.width();let yp=(p.y-ll.top())/ll.height();if yp<0.52{self.set_tty_mode(TtyMode::Off);}else if xp<0.40&&yp>0.40&&yp<0.80{self.set_tty_mode(TtyMode::Line);}else if xp>0.56&&yp>0.40&&yp<0.80{self.set_tty_mode(TtyMode::Local);}}else{let ix=(p.x-r.left())/scale;let iy=(p.y-r.top())/scale;if let Some(index)=teletype::KEYS.iter().position(|k|k.contains(ix,iy)){self.press_tty_key(index);}}}}
        let pointer_down=ui.ctx().input(|i|i.pointer.any_down());if !pointer_down{self.release_tty_key();}self.draw_pressed_key(ui,o,scale);
        if let Some(t)=&self.tex.tty_knob{let kw=TTY_W*0.06*scale;let kh=kw*117./116.;let base=r.right()-TTY_W*0.06*scale-kw;let sh=match self.tty.mode{TtyMode::Line=>-0.35*kw,TtyMode::Off=>0.,TtyMode::Local=>0.35*kw};let kr=Rect::from_min_size(Pos2::new(base+sh,r.bottom()-TTY_H*0.022*scale-kh),Vec2::new(kw,kh));Self::image(ui,t,kr);}
    }

    fn update_paper_tape(&mut self) {
        if self.last_tape_tick.elapsed()<Duration::from_millis(30){return;}self.last_tape_tick=Instant::now();
        if self.machine.bus.serial_rx.is_empty(){if let Some(byte)=self.tty.next_tape_byte(){self.machine.bus.serial_rx.push_back(byte);}}
    }

    fn load_paper_tape(&mut self) {
        let Some(path)=rfd::FileDialog::new().add_filter("Paper tape", &["txt","tap","bin"]).pick_file() else{return};
        match std::fs::read(&path){Ok(bytes)=>{self.tty.load_tape(&bytes);self.status=format!("Paper tape loaded: {} bytes",bytes.len());},Err(e)=>self.status=format!("Paper tape load failed: {e}"),}
    }

    fn save_punched_tape(&mut self) {
        let Some(path)=rfd::FileDialog::new().set_file_name("myPaperTape.txt").save_file() else{return};
        match std::fs::write(&path,&self.tty.tape_out){Ok(_)=>self.status=format!("Punched tape saved: {} bytes",self.tty.tape_out.len()),Err(e)=>self.status=format!("Paper tape save failed: {e}"),}
    }
}

impl eframe::App for RusTairApp {
    fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
        let now=Instant::now();let dt=now.duration_since(self.last_tick).min(Duration::from_millis(20));self.last_tick=now;
        self.update_key_animation(ctx);self.update_paper_tape();
        let target=if self.inside_open && self.machine.powered{1.0}else{0.0};
        let step=(dt.as_secs_f32()/0.5).clamp(0.0,1.0);
        self.inside_slide += (target-self.inside_slide)*step*4.0;
        if (target-self.inside_slide).abs()>0.002{ctx.request_repaint();}else{self.inside_slide=target;}
        if let Some(until)=self.reset_flash_until{if now>=until{self.machine.address_leds=0;self.machine.bus.data_leds=0;self.reset_flash_until=None;}}
        if self.machine.running{let cycles=(CLOCK_HZ as f64*dt.as_secs_f64())as u32;self.machine.run_cycles(cycles.clamp(1,10000));ctx.request_repaint();}
        self.process_tty_io(ctx);
        egui::TopBottomPanel::top("menu").show(ctx,|ui|egui::MenuBar::new().ui(ui,|ui|{
            ui.menu_button("File",|ui|{
                if ui.button("Load binary…").clicked(){if let Some(path)=rfd::FileDialog::new().pick_file(){match std::fs::read(&path){Ok(bytes)=>{self.machine.bus.load(0,&bytes);self.status=format!("Loaded {} bytes from {}",bytes.len(),path.display());},Err(e)=>self.status=format!("Load failed: {e}")}}ui.close();}
                if ui.button("Load bundled Microsoft 4K BASIC").clicked(){match std::fs::read("assets/4kbas32.bin"){Ok(bytes)=>{self.machine.bus.load(0,&bytes);self.machine.cpu.pc=0;self.status="Loaded Microsoft 4K BASIC".into();},Err(e)=>self.status=format!("4K BASIC asset missing: {e}")}ui.close();}
            });
            ui.separator();ui.selectable_value(&mut self.view,View::Altair,"ALTAIR 8800");ui.selectable_value(&mut self.view,View::Teletype,"ASR-33 TELETYPE");
            if self.view==View::Altair && self.machine.powered{ui.separator();if ui.selectable_label(self.inside_open,"INSIDE").clicked(){self.inside_open=!self.inside_open;}}
            if self.view==View::Teletype{ui.separator();ui.selectable_value(&mut self.tty.paper_width,52,"Large");ui.selectable_value(&mut self.tty.paper_width,82,"Normal");if ui.button("Clear paper").clicked(){self.tty.clear_paper();}if ui.button("Read tape…").clicked(){self.load_paper_tape();}let punch_label=if self.tty.capture_to_tape{"Finish punch"}else{"Punch tape"};if ui.button(punch_label).clicked(){if self.tty.capture_to_tape{self.tty.capture_to_tape=false;self.save_punched_tape();}else{self.tty.tape_out.clear();self.tty.capture_to_tape=true;}}}
            ui.separator();let mut muted=self.audio.muted();if ui.checkbox(&mut muted,"Mute").changed(){self.audio.set_muted(muted);}ui.separator();ui.label(format!("PC {:04X}  SP {:04X}  A {:02X}  F {:02X}",self.machine.cpu.pc,self.machine.cpu.sp,self.machine.cpu.a,self.machine.cpu.f));ui.separator();ui.label(if self.machine.running{"RUNNING"}else if self.machine.powered{"STOPPED"}else{"POWER OFF"});
        }));
        egui::CentralPanel::default().show(ctx,|ui|ui.centered_and_justified(|ui|match self.view{View::Altair=>self.draw_altair(ui),View::Teletype=>self.draw_teletype(ui)}));
        egui::TopBottomPanel::bottom("status").show(ctx,|ui|{ui.small(&self.status);});
    }
}

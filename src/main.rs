mod cpu8080;
mod machine;

use std::time::{Duration, Instant};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use machine::{AltairMachine, CLOCK_HZ};

const PANEL_W: f32 = 1573.0;
const PANEL_H: f32 = 647.0;
const TTY_W: f32 = 3008.0;
const TTY_H: f32 = 2983.0;

const SWITCH_X: [f32; 16] = [1332.,1278.,1224.,1142.,1087.,1032.,950.,895.,840.,758.,703.,648.,566.,512.,457.,376.];
const SWITCH_Y: [f32; 16] = [305.,305.,305.,305.,303.,303.,303.,303.,303.,303.,303.,301.,301.,301.,301.,301.];
const ADDR_LED_X: [f32; 16] = [1341.,1286.,1231.,1148.,1093.,1037.,955.,900.,845.,763.,708.,653.,573.,518.,463.,381.];
const ADDR_LED_Y: [f32; 16] = [233.,233.,233.,233.,231.,231.,231.,230.,230.,230.,230.,230.,229.,229.,229.,229.];
const DATA_LED_X: [f32; 8] = [1341.,1286.,1231.,1148.,1093.,1037.,955.,900.];
const DATA_LED_Y: [f32; 8] = [122.,122.,122.,122.,120.,120.,120.,120.];

#[derive(Clone, Copy, PartialEq, Eq)]
enum View { Altair, Teletype }
#[derive(Clone, Copy, PartialEq, Eq)]
enum TtyMode { Off, Line, Local }

struct Tex {
    panel: Option<egui::TextureHandle>, led_on: Option<egui::TextureHandle>,
    switch_up: Option<egui::TextureHandle>, switch_down: Option<egui::TextureHandle>, switch_centre: Option<egui::TextureHandle>,
    tty_body: Option<egui::TextureHandle>, tty_keys: Option<egui::TextureHandle>, tty_head: Option<egui::TextureHandle>,
    tty_line_local: Option<egui::TextureHandle>, tty_knob: Option<egui::TextureHandle>,
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("RusTair — MITS Altair 8800 + ASR-33").with_inner_size([1400.0, 820.0]).with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("RusTair", options, Box::new(|cc| Ok(Box::new(RusTairApp::new(cc)))))
}

struct RusTairApp {
    machine: AltairMachine, tex: Tex, view: View, tty_mode: TtyMode,
    tty_output: String, tty_column: usize, paper_width: usize,
    last_tick: Instant, reset_flash_until: Option<Instant>, status: String,
}

impl RusTairApp {
    fn load_texture(ctx: &egui::Context, name: &str, path: &str) -> Option<egui::TextureHandle> {
        let bytes = std::fs::read(path).ok()?;
        let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        Some(ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw()), egui::TextureOptions::LINEAR))
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self {
            machine: AltairMachine::default(),
            tex: Tex {
                panel: Self::load_texture(&cc.egui_ctx,"altair-panel","assets/Altair1.png"),
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
            view: View::Altair, tty_mode: TtyMode::Off, tty_output: " ".into(), tty_column: 0, paper_width: 82,
            last_tick: Instant::now(), reset_flash_until: None, status: "Ready".into(),
        }
    }

    fn image(ui: &mut egui::Ui, t: &egui::TextureHandle, r: Rect) {
        ui.painter().image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.,1.)), Color32::WHITE);
    }

    fn led(&self, ui: &mut egui::Ui, origin: Pos2, scale: f32, x: f32, y: f32, on: bool) {
        if !self.machine.powered || !on { return; }
        let r = Rect::from_min_size(origin + Vec2::new(x*scale,y*scale), Vec2::splat(24.*scale));
        if let Some(t)=&self.tex.led_on { Self::image(ui,t,r); } else { ui.painter().circle_filled(r.center(),8.*scale,Color32::from_rgb(255,45,20)); }
    }

    fn momentary(&mut self, ui:&mut egui::Ui, origin:Pos2, scale:f32, x:f32, y:f32, label:&str) -> Option<bool> {
        let r=Rect::from_min_size(origin+Vec2::new(x*scale,y*scale),Vec2::new(32.*scale,96.*scale));
        let resp=ui.allocate_rect(r,Sense::click());
        let down=resp.interact_pointer_pos().map(|p| p.y>r.center().y).unwrap_or(false);
        let tex=if resp.is_pointer_button_down_on(){if down{self.tex.switch_down.as_ref()}else{self.tex.switch_up.as_ref()}}else{self.tex.switch_centre.as_ref()};
        if let Some(t)=tex { Self::image(ui,t,r); }
        if resp.hovered(){ resp.clone().on_hover_text(label); }
        if resp.clicked(){Some(down)}else{None}
    }

    fn draw_altair(&mut self, ui:&mut egui::Ui) {
        let a=ui.available_size(); let scale=(a.x/PANEL_W).min(a.y/PANEL_H).clamp(0.2,2.5);
        let (r,_)=ui.allocate_exact_size(Vec2::new(PANEL_W*scale,PANEL_H*scale),Sense::hover()); let o=r.min;
        if let Some(t)=&self.tex.panel { Self::image(ui,t,r); } else { ui.painter().rect_filled(r,0.,Color32::from_rgb(25,35,43)); }
        for bit in 0..16 {
            let sr=Rect::from_min_size(o+Vec2::new(SWITCH_X[bit]*scale,SWITCH_Y[bit]*scale),Vec2::new(32.*scale,96.*scale));
            let resp=ui.allocate_rect(sr,Sense::click()); if resp.clicked(){self.machine.bus.panel_switches^=1u16<<bit;}
            let up=self.machine.bus.panel_switches&(1u16<<bit)!=0;
            if let Some(t)=if up{self.tex.switch_up.as_ref()}else{self.tex.switch_down.as_ref()}{Self::image(ui,t,sr);}
        }
        for bit in 0..16{self.led(ui,o,scale,ADDR_LED_X[bit],ADDR_LED_Y[bit],self.machine.address_leds&(1u16<<bit)!=0);}
        for bit in 0..8{self.led(ui,o,scale,DATA_LED_X[bit],DATA_LED_Y[bit],self.machine.bus.data_leds&(1u8<<bit)!=0);}
        self.led(ui,o,scale,218.,228.,self.machine.wait_led); self.led(ui,o,scale,324.,119.,self.machine.powered); self.led(ui,o,scale,434.,120.,self.machine.powered); self.led(ui,o,scale,654.,120.,self.machine.powered);
        let pr=Rect::from_min_size(o+Vec2::new(114.*scale,408.*scale),Vec2::new(32.*scale,96.*scale));
        if ui.allocate_rect(pr,Sense::click()).clicked(){self.machine.power(!self.machine.powered);}
        if let Some(t)=if self.machine.powered{self.tex.switch_down.as_ref()}else{self.tex.switch_up.as_ref()}{Self::image(ui,t,pr);}
        if let Some(d)=self.momentary(ui,o,scale,377.,410.,"RUN / STOP"){self.machine.set_running(d);}
        if self.momentary(ui,o,scale,486.,410.,"SINGLE STEP").is_some(){self.machine.step();}
        if let Some(d)=self.momentary(ui,o,scale,595.,410.,"EXAMINE / EXAMINE NEXT"){self.machine.examine(d);}
        if let Some(d)=self.momentary(ui,o,scale,704.,410.,"DEPOSIT / DEPOSIT NEXT"){self.machine.deposit(d);}
        if self.momentary(ui,o,scale,813.,410.,"RESET").is_some(){self.machine.reset();self.machine.address_leds=0xffff;self.machine.bus.data_leds=0xff;self.reset_flash_until=Some(Instant::now()+Duration::from_millis(500));}
    }

    fn tty_print(&mut self, byte:u8) {
        if self.tty_mode==TtyMode::Off{self.tty_mode=TtyMode::Line;}
        match byte&0x7f {
            0x07|0x1b=>{}, b'\r'|b'\n'=>{self.tty_output.push('\n');self.tty_output.push(' ');self.tty_column=0;},
            b@0x20..=0x7e=>{if self.tty_column>self.paper_width{self.tty_output.push('\n');self.tty_output.push(' ');self.tty_column=0;}self.tty_output.push((b as char).to_ascii_uppercase());self.tty_column+=1;}, _=>{}
        }
        if self.tty_output.len()>12000{let cut=self.tty_output.char_indices().nth(4000).map(|x|x.0).unwrap_or(0);self.tty_output.drain(..cut);}
    }

    fn process_tty_io(&mut self, ctx:&egui::Context) {
        while let Some(b)=self.machine.bus.serial_tx.pop_front(){self.tty_print(b);}
        if self.view!=View::Teletype||self.tty_mode==TtyMode::Off{return;}
        let mut chars=Vec::new(); ctx.input(|i|for e in &i.events{match e{egui::Event::Text(s)=>chars.extend(s.bytes()),egui::Event::Key{key:egui::Key::Enter,pressed:true,..}=>chars.push(b'\r'),egui::Event::Key{key:egui::Key::Backspace,pressed:true,..}=>chars.push(0x7f),_=>{}}});
        for b in chars{let b=b.to_ascii_uppercase();self.machine.bus.serial_rx.push_back(b);if self.tty_mode==TtyMode::Local{self.tty_print(b);}}
    }

    fn draw_teletype(&mut self, ui:&mut egui::Ui) {
        let a=ui.available_size(); let scale=(a.x/TTY_W).min(a.y/TTY_H).clamp(0.12,1.5);
        let (r,resp)=ui.allocate_exact_size(Vec2::new(TTY_W*scale,TTY_H*scale),Sense::click()); let o=r.min;
        if let Some(t)=&self.tex.tty_body{Self::image(ui,t,r);}else{ui.painter().rect_filled(r,0.,Color32::from_rgb(80,76,65));}
        if let Some(t)=&self.tex.tty_keys{Self::image(ui,t,r);}
        let paper=Rect::from_min_size(o+Vec2::new(TTY_W*0.25*scale,TTY_H*0.34*scale),Vec2::new(TTY_W*0.49*scale,TTY_H*0.27*scale));
        let fs=if self.paper_width<=52{29.*scale}else{18.5*scale}; ui.painter().text(paper.left_bottom(),egui::Align2::LEFT_BOTTOM,&self.tty_output,egui::FontId::monospace(fs.max(5.)),Color32::from_rgb(35,35,30));
        if self.tty_mode!=TtyMode::Off{if let Some(t)=&self.tex.tty_head{let cw=if self.paper_width<=52{31.}else{20.};let x=TTY_W*0.25+(self.tty_column as f32).min(self.paper_width as f32)*cw;let hr=Rect::from_min_size(o+Vec2::new(x*scale,TTY_H*0.33*scale),Vec2::new(180.*scale,190.*scale));Self::image(ui,t,hr);}}
        let lls=Vec2::new(TTY_W*0.18*scale,288.*(TTY_W*0.18/349.)*scale); let ll=Rect::from_min_size(Pos2::new(r.right()-lls.x,r.bottom()-lls.y),lls);
        if let Some(t)=&self.tex.tty_line_local{Self::image(ui,t,ll);}
        if resp.clicked(){if let Some(p)=resp.interact_pointer_pos(){if ll.contains(p){let xp=(p.x-ll.left())/ll.width();let yp=(p.y-ll.top())/ll.height();if yp<0.52{self.tty_mode=TtyMode::Off;}else if xp<0.40&&yp<0.80{self.tty_mode=TtyMode::Line;}else if xp>0.56&&yp<0.80{self.tty_mode=TtyMode::Local;}}}}
        if let Some(t)=&self.tex.tty_knob{let kw=TTY_W*0.06*scale;let kh=kw*117./116.;let base=r.right()-TTY_W*0.06*scale-kw;let sh=match self.tty_mode{TtyMode::Line=>-0.35*kw,TtyMode::Off=>0.,TtyMode::Local=>0.35*kw};let kr=Rect::from_min_size(Pos2::new(base+sh,r.bottom()-TTY_H*0.022*scale-kh),Vec2::new(kw,kh));Self::image(ui,t,kr);}
    }
}

impl eframe::App for RusTairApp {
    fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
        let now=Instant::now();let dt=now.duration_since(self.last_tick).min(Duration::from_millis(20));self.last_tick=now;
        if let Some(until)=self.reset_flash_until{if now>=until{self.machine.address_leds=0;self.machine.bus.data_leds=0;self.reset_flash_until=None;}}
        if self.machine.running{let cycles=(CLOCK_HZ as f64*dt.as_secs_f64())as u32;self.machine.run_cycles(cycles.clamp(1,10000));ctx.request_repaint();}
        self.process_tty_io(ctx);
        egui::TopBottomPanel::top("menu").show(ctx,|ui|egui::MenuBar::new().ui(ui,|ui|{
            ui.menu_button("File",|ui|{if ui.button("Load binary…").clicked(){if let Some(path)=rfd::FileDialog::new().pick_file(){match std::fs::read(&path){Ok(bytes)=>{self.machine.bus.load(0,&bytes);self.status=format!("Loaded {} bytes from {}",bytes.len(),path.display());},Err(e)=>self.status=format!("Load failed: {e}")}}ui.close();}if ui.button("Load bundled Microsoft 4K BASIC").clicked(){match std::fs::read("assets/4kbas32.bin"){Ok(bytes)=>{self.machine.bus.load(0,&bytes);self.machine.cpu.pc=0;self.status="Loaded Microsoft 4K BASIC".into();},Err(e)=>self.status=format!("4K BASIC asset missing: {e}")}ui.close();}});
            ui.separator();ui.selectable_value(&mut self.view,View::Altair,"ALTAIR 8800");ui.selectable_value(&mut self.view,View::Teletype,"ASR-33 TELETYPE");
            if self.view==View::Teletype{ui.separator();ui.selectable_value(&mut self.paper_width,52,"Large type");ui.selectable_value(&mut self.paper_width,82,"Normal type");if ui.button("Clear paper").clicked(){self.tty_output=" ".into();self.tty_column=0;}}
            ui.separator();ui.label(format!("PC {:04X}  SP {:04X}  A {:02X}  F {:02X}",self.machine.cpu.pc,self.machine.cpu.sp,self.machine.cpu.a,self.machine.cpu.f));ui.separator();ui.label(if self.machine.running{"RUNNING"}else if self.machine.powered{"STOPPED"}else{"POWER OFF"});
        }));
        egui::CentralPanel::default().show(ctx,|ui|ui.centered_and_justified(|ui|match self.view{View::Altair=>self.draw_altair(ui),View::Teletype=>self.draw_teletype(ui)}));
        egui::TopBottomPanel::bottom("status").show(ctx,|ui|{ui.small(&self.status);});
    }
}

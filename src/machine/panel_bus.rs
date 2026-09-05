use std::time::Duration;

const LAMP_INTE: usize = 0;
const LAMP_PROT: usize = 1;
const LAMP_MEMR: usize = 2;
const LAMP_INP: usize = 3;
const LAMP_M1: usize = 4;
const LAMP_OUT: usize = 5;
const LAMP_HLTA: usize = 6;
const LAMP_STACK: usize = 7;
const LAMP_WO: usize = 8;
const LAMP_INT: usize = 9;
const LAMP_WAIT: usize = 10;
const LAMP_HLDA: usize = 11;
const LAMP_COUNT: usize = 12;
const PACKED_DATA_SHIFT: usize = 16;
const PACKED_LAMP_SHIFT: usize = 24;
const DUTY_COUNTER_PLANES: usize = u64::BITS as usize;
const STATUS_INSTRUCTION_FETCH: u8 = 0xa2;

// Presentation persistence only. Electrical duty is accumulated independently
// over every CPU-board sample seen since the previous commit. This low-pass maps
// that exact duty onto human-visible persistence without feeding presentation
// state back into the S-100 model.
const VISUAL_PERSISTENCE_SECS: f32 = 0.045;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BusOwner {
    None,
    Cpu,
    FrontPanel,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct S100Signals {
    pub address: u16,
    pub data_in: Option<u8>,
    pub data_out: Option<u8>,
    pub cpu_data: Option<u8>,
    pub panel_data: u8,
    pub phi1: Option<bool>,
    pub phi2: Option<bool>,
    pub cloc: Option<bool>,
    pub psync: bool,
    pub pwr_n: bool,
    pub pdbin: bool,
    pub memr: bool,
    pub inp: bool,
    pub m1: bool,
    pub out: bool,
    pub hlta: bool,
    pub stack: bool,
    pub wo: bool,
    pub int_ack: bool,
    pub inte: bool,
    pub prot: bool,
    pub run: bool,
    pub ready: bool,
    pub front_panel_ready: bool,
    pub memory_ready: bool,
    pub wait: bool,
    pub interrupt: bool,
    pub hold: bool,
    pub hlda: bool,
    pub reset: bool,
    pub ext_clear: bool,
    pub owner: BusOwner,
}

impl Default for S100Signals {
    fn default() -> Self {
        Self {
            address: 0,
            data_in: None,
            data_out: None,
            cpu_data: None,
            panel_data: 0,
            phi1: None,
            phi2: None,
            cloc: None,
            psync: false,
            pwr_n: true,
            pdbin: false,
            memr: false,
            inp: false,
            m1: false,
            out: false,
            hlta: false,
            stack: false,
            wo: false,
            int_ack: false,
            inte: false,
            prot: false,
            run: false,
            ready: false,
            front_panel_ready: false,
            memory_ready: true,
            wait: false,
            interrupt: false,
            hold: false,
            hlda: false,
            reset: false,
            ext_clear: false,
            owner: BusOwner::None,
        }
    }
}

impl S100Signals {
    fn clear_status(&mut self) {
        self.memr = false;
        self.inp = false;
        self.m1 = false;
        self.out = false;
        self.hlta = false;
        self.stack = false;
        self.wo = false;
        self.int_ack = false;
    }

    fn apply_status_word(&mut self, word: u8) {
        self.memr = word & 0x80 != 0;
        self.inp = word & 0x40 != 0;
        self.m1 = word & 0x20 != 0;
        self.out = word & 0x10 != 0;
        self.hlta = word & 0x08 != 0;
        self.stack = word & 0x04 != 0;
        self.wo = word & 0x02 != 0;
        self.int_ack = word & 0x01 != 0;
    }

    fn lamp_states(&self) -> [bool; LAMP_COUNT] {
        [self.inte, self.prot, self.memr, self.inp, self.m1, self.out, self.hlta,
         self.stack, self.wo, self.int_ack, self.wait, self.hlda]
    }

    #[inline]
    fn lamp_mask(&self) -> u16 {
        u16::from(self.inte) << LAMP_INTE
            | u16::from(self.prot) << LAMP_PROT
            | u16::from(self.memr) << LAMP_MEMR
            | u16::from(self.inp) << LAMP_INP
            | u16::from(self.m1) << LAMP_M1
            | u16::from(self.out) << LAMP_OUT
            | u16::from(self.hlta) << LAMP_HLTA
            | u16::from(self.stack) << LAMP_STACK
            | u16::from(self.wo) << LAMP_WO
            | u16::from(self.int_ack) << LAMP_INT
            | u16::from(self.wait) << LAMP_WAIT
            | u16::from(self.hlda) << LAMP_HLDA
    }

    #[inline]
    fn packed_lamp_activity(&self) -> u64 {
        u64::from(self.address)
            | (u64::from(self.panel_data) << PACKED_DATA_SHIFT)
            | (u64::from(self.lamp_mask()) << PACKED_LAMP_SHIFT)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelLampSnapshot {
    pub address: [f32; 16],
    pub data: [f32; 8],
    pub inte: f32,
    pub prot: f32,
    pub memr: f32,
    pub inp: f32,
    pub m1: f32,
    pub out: f32,
    pub hlta: f32,
    pub stack: f32,
    pub wo: f32,
    pub int_ack: f32,
    pub wait: f32,
    pub hlda: f32,
}

impl Default for PanelLampSnapshot {
    fn default() -> Self {
        Self { address: [0.0;16], data: [0.0;8], inte:0.0, prot:0.0, memr:0.0,
            inp:0.0, m1:0.0, out:0.0, hlta:0.0, stack:0.0, wo:0.0,
            int_ack:0.0, wait:0.0, hlda:0.0 }
    }
}

impl PanelLampSnapshot {
    fn lamp_array(self) -> [f32; LAMP_COUNT] {
        [self.inte,self.prot,self.memr,self.inp,self.m1,self.out,self.hlta,self.stack,
         self.wo,self.int_ack,self.wait,self.hlda]
    }
    fn set_lamp_array(&mut self, values:[f32;LAMP_COUNT]) {
        self.inte=values[LAMP_INTE]; self.prot=values[LAMP_PROT]; self.memr=values[LAMP_MEMR];
        self.inp=values[LAMP_INP]; self.m1=values[LAMP_M1]; self.out=values[LAMP_OUT];
        self.hlta=values[LAMP_HLTA]; self.stack=values[LAMP_STACK]; self.wo=values[LAMP_WO];
        self.int_ack=values[LAMP_INT]; self.wait=values[LAMP_WAIT]; self.hlda=values[LAMP_HLDA];
    }
}

struct PanelLampIntegrator {
    on_count_planes:[u64;DUTY_COUNTER_PLANES],
    total_weight:u64,
    raw_duty:PanelLampSnapshot,
    snapshot:PanelLampSnapshot,
}
impl Default for PanelLampIntegrator {
    fn default()->Self { Self { on_count_planes:[0;DUTY_COUNTER_PLANES], total_weight:0,
        raw_duty:PanelLampSnapshot::default(), snapshot:PanelLampSnapshot::default() } }
}
impl PanelLampIntegrator {
    #[inline]
    fn add_mask_at_plane(&mut self, mut carry:u64, mut plane:usize) {
        while carry!=0 && plane<DUTY_COUNTER_PLANES { let next=self.on_count_planes[plane]&carry;
            self.on_count_planes[plane]^=carry; carry=next; plane+=1; }
        debug_assert_eq!(carry,0,"front-panel duty counter overflow");
    }
    #[inline]
    fn add_weighted_mask(&mut self, mask:u64, weight:u64) {
        let mut remaining=weight; let mut plane=0usize;
        while remaining!=0 { if remaining&1!=0 { self.add_mask_at_plane(mask,plane); }
            remaining>>=1; plane+=1; }
    }
    #[inline]
    fn sample(&mut self, signals:&S100Signals, weight:u32) {
        let requested=u64::from(weight); if requested==0{return;}
        let accepted=requested.min(u64::MAX-self.total_weight); if accepted==0{return;}
        self.add_weighted_mask(signals.packed_lamp_activity(),accepted); self.total_weight+=accepted;
    }
    #[inline]
    fn sample_reconstructed_cycle(&mut self, common_mask:u64, first_panel_data:u8,
        final_panel_data:u8, t_states:u32, data_changes_after_t1:bool) {
        self.sample_reconstructed_cycle_with_latched_status(common_mask,common_mask,
            first_panel_data,final_panel_data,t_states,data_changes_after_t1);
    }
    #[inline]
    fn sample_reconstructed_cycle_with_latched_status(&mut self, first_common_mask:u64,
        latched_common_mask:u64, first_panel_data:u8, final_panel_data:u8,
        t_states:u32, data_changes_after_t1:bool) {
        let requested=u64::from(t_states); if requested==0{return;}
        let accepted=requested.min(u64::MAX-self.total_weight); if accepted==0{return;}
        let rest_panel_data=if data_changes_after_t1 { final_panel_data } else { first_panel_data };
        let first_mask=first_common_mask | (u64::from(first_panel_data)<<PACKED_DATA_SHIFT);
        let rest_mask=latched_common_mask | (u64::from(rest_panel_data)<<PACKED_DATA_SHIFT);
        if first_mask==rest_mask { self.add_weighted_mask(first_mask,accepted); }
        else { self.add_weighted_mask(first_mask,1); if accepted>1 { self.add_weighted_mask(rest_mask,accepted-1); } }
        self.total_weight+=accepted;
    }
    fn binary_snapshot(signals:&S100Signals)->PanelLampSnapshot {
        let mut snapshot=PanelLampSnapshot { address:bits16(signals.address), data:bits8(signals.panel_data), ..PanelLampSnapshot::default() };
        let states=signals.lamp_states(); let mut lamps=[0.0;LAMP_COUNT];
        for bit in 0..LAMP_COUNT { lamps[bit]=if states[bit]{1.0}else{0.0}; }
        snapshot.set_lamp_array(lamps); snapshot
    }
    #[inline]
    fn packed_count(&self, packed_bit:usize)->u64 { let mask=1u64<<packed_bit; let mut value=0u64;
        for (plane,bits) in self.on_count_planes.iter().copied().enumerate() { if bits&mask!=0 { value|=1u64<<plane; } } value }
    fn accumulated_duty(&self)->PanelLampSnapshot { debug_assert!(self.total_weight!=0); let total=self.total_weight as f32;
        let mut duty=PanelLampSnapshot::default(); for bit in 0..16 { duty.address[bit]=self.packed_count(bit) as f32/total; }
        for bit in 0..8 { duty.data[bit]=self.packed_count(PACKED_DATA_SHIFT+bit) as f32/total; }
        let mut lamps=[0.0;LAMP_COUNT]; for bit in 0..LAMP_COUNT { lamps[bit]=self.packed_count(PACKED_LAMP_SHIFT+bit) as f32/total; }
        duty.set_lamp_array(lamps); duty }
    fn raw_duty_snapshot(&self)->PanelLampSnapshot { if self.total_weight==0 {self.raw_duty}else{self.accumulated_duty()} }
    fn freeze(&mut self, signals:&S100Signals) { self.clear_activity(); let instant=Self::binary_snapshot(signals); self.raw_duty=instant; self.snapshot=instant; }
    fn commit(&mut self, signals:&S100Signals, dt:Duration, dynamic:bool) { if !dynamic {self.freeze(signals);return;} if self.total_weight==0 {self.sample(signals,1);}
        let target=self.accumulated_duty(); self.raw_duty=target; let dt_secs=dt.as_secs_f32().max(0.000_001);
        let retention=(-dt_secs/VISUAL_PERSISTENCE_SECS).exp().clamp(0.0,1.0); let inject=1.0-retention;
        for bit in 0..16 { self.snapshot.address[bit]=self.snapshot.address[bit]*retention+target.address[bit]*inject; }
        for bit in 0..8 { self.snapshot.data[bit]=self.snapshot.data[bit]*retention+target.data[bit]*inject; }
        let old=self.snapshot.lamp_array(); let new=target.lamp_array(); let mut mixed=[0.0;LAMP_COUNT];
        for bit in 0..LAMP_COUNT { mixed[bit]=old[bit]*retention+new[bit]*inject; } self.snapshot.set_lamp_array(mixed); self.clear_activity(); }
    fn clear(&mut self){self.clear_activity();self.raw_duty=PanelLampSnapshot::default();self.snapshot=PanelLampSnapshot::default();}
    fn clear_activity(&mut self){self.on_count_planes.fill(0);self.total_weight=0;}
}

pub(super) struct S100BusState { signals:S100Signals, lamps:PanelLampIntegrator }
impl Default for S100BusState { fn default()->Self{Self{signals:S100Signals::default(),lamps:PanelLampIntegrator::default()}} }
impl S100BusState {
    pub(super) fn signals(&self)->S100Signals{self.signals}
    pub(super) fn snapshot(&self)->PanelLampSnapshot{self.lamps.snapshot}
    pub(super) fn raw_duty_snapshot(&self)->PanelLampSnapshot{self.lamps.raw_duty_snapshot()}
    #[cfg(test)] pub(super) fn debug_set_snapshot(&mut self,snapshot:PanelLampSnapshot){self.lamps.clear_activity();self.lamps.raw_duty=snapshot;self.lamps.snapshot=snapshot;}
    pub(super) fn power_off(&mut self){self.signals=S100Signals::default();self.lamps.clear();}
    pub(super) fn set_inte(&mut self,enabled:bool){self.signals.inte=enabled;}
    pub(super) fn set_run(&mut self,run:bool){self.signals.run=run;}
    fn recompute_ready(&mut self){self.signals.ready=self.signals.front_panel_ready&&self.signals.memory_ready;}
    pub(super) fn set_ready(&mut self,ready:bool){self.signals.front_panel_ready=ready;self.signals.memory_ready=true;self.recompute_ready();self.signals.wait=!self.signals.ready&&!self.signals.reset;}
    pub(super) fn set_ready_input(&mut self,ready:bool){self.signals.front_panel_ready=ready;self.recompute_ready();}
    pub(super) fn set_memory_ready_input(&mut self,ready:bool){self.signals.memory_ready=ready;self.recompute_ready();}
    pub(super) fn set_interrupt_request(&mut self,asserted:bool){self.signals.interrupt=asserted;}
    pub(super) fn set_hold(&mut self,hold:bool){self.signals.hold=hold;if !hold{self.signals.hlda=false;}}
    pub(super) fn set_hlda(&mut self,ack:bool){self.signals.hlda=ack;}
    pub(super) fn set_ext_clear(&mut self,asserted:bool){self.signals.ext_clear=asserted;}
    pub(super) fn drive_cpu_board_edge(&mut self,phi1:bool,phi2:bool,psync:bool,pdbin:bool,pwr_n:bool){self.signals.phi1=Some(phi1);self.signals.phi2=Some(phi2);if phi1{self.signals.cloc=Some(true);}else if phi2{self.signals.cloc=Some(false);}self.signals.psync=psync;self.signals.pdbin=pdbin;self.signals.pwr_n=pwr_n;}
    pub(super) fn latch_cpu_status(&mut self,word:u8){self.signals.apply_status_word(word);}
    #[inline]
    pub(super) fn drive_reconstructed_cpu_cycle(&mut self,address:u16,data:u8,status_word:u8,t_states:u32,reads:bool,writes:bool,protected:bool,inte:bool,ready:bool,wait:bool){
        let first=self.signals.panel_data;self.signals.reset=false;self.signals.inte=inte;self.signals.ready=ready;self.signals.wait=wait;self.signals.hlda=false;self.signals.owner=BusOwner::Cpu;self.signals.address=address;self.signals.prot=protected;self.signals.apply_status_word(status_word);
        let common=u64::from(address)|(u64::from(self.signals.lamp_mask())<<PACKED_LAMP_SHIFT);let final_data=if reads{data}else{first};self.lamps.sample_reconstructed_cycle(common,first,final_data,t_states,reads);
        if reads{self.signals.cpu_data=Some(data);self.signals.data_in=Some(data);self.signals.data_out=None;self.signals.panel_data=data;}else if writes{self.signals.cpu_data=Some(data);self.signals.data_in=None;self.signals.data_out=Some(data);}else{self.signals.cpu_data=None;self.signals.data_in=None;self.signals.data_out=None;}}
    #[inline]
    pub(super) fn drive_cycle_full_reconstructed_cpu_cycle(&mut self,address:u16,data:u8,status_word:u8,t_states:u32,reads:bool,writes:bool,protected:bool,inte:bool,ready:bool,wait:bool){
        debug_assert!(t_states>=1);debug_assert!(!(reads&&writes));let first=self.signals.panel_data;self.signals.reset=false;self.signals.inte=inte;self.signals.ready=ready;self.signals.wait=wait;self.signals.hlda=false;self.signals.owner=BusOwner::Cpu;self.signals.address=address;self.signals.prot=protected;
        let first_common=u64::from(address)|(u64::from(self.signals.lamp_mask())<<PACKED_LAMP_SHIFT);self.signals.apply_status_word(status_word);let latched=u64::from(address)|(u64::from(self.signals.lamp_mask())<<PACKED_LAMP_SHIFT);let final_data=if reads{data}else{first};self.lamps.sample_reconstructed_cycle_with_latched_status(first_common,latched,first,final_data,t_states,reads);
        if reads{self.signals.panel_data=data;self.signals.cpu_data=Some(data);self.signals.data_in=Some(data);self.signals.data_out=None;}else if writes{self.signals.cpu_data=Some(data);self.signals.data_in=None;self.signals.data_out=Some(data);}else{self.signals.cpu_data=None;self.signals.data_in=None;self.signals.data_out=None;}}
    #[inline]
    pub(super) fn drive_cycle_full_internal_t_states(&mut self,t_states:u32,inte:bool,ready:bool,wait:bool){if t_states==0{return;}self.signals.reset=false;self.signals.inte=inte;self.signals.ready=ready;self.signals.wait=wait;self.signals.hlda=false;self.signals.owner=BusOwner::Cpu;self.signals.cpu_data=None;self.signals.data_in=None;self.signals.data_out=None;self.lamps.sample(&self.signals,t_states);}
    pub(super) fn drive_cpu_t_state(&mut self,address:Option<u16>,cpu_data:Option<u8>,data_in:Option<u8>,data_out:Option<u8>,status_word:Option<u8>,protected:bool,inte:bool,ready:bool,wait:bool,hlda:bool){self.signals.reset=false;self.signals.inte=inte;self.signals.ready=ready;self.signals.wait=wait;self.signals.hlda=hlda;self.signals.cpu_data=cpu_data;self.signals.data_in=data_in;self.signals.data_out=data_out;if let Some(data)=data_in{self.signals.panel_data=data;}if hlda{self.signals.owner=BusOwner::None;self.signals.prot=false;self.signals.cpu_data=None;self.signals.data_in=None;self.signals.data_out=None;self.signals.clear_status();self.lamps.sample(&self.signals,1);return;}self.signals.owner=BusOwner::Cpu;if let Some(a)=address{self.signals.address=a;self.signals.prot=protected;}if let Some(s)=status_word{self.signals.apply_status_word(s);}self.lamps.sample(&self.signals,1);}
    pub(super) fn assert_front_panel_reset(&mut self,run:bool){self.signals.reset=true;self.signals.owner=BusOwner::FrontPanel;self.signals.address=0xffff;self.signals.data_in=Some(0xff);self.signals.data_out=None;self.signals.cpu_data=None;self.signals.panel_data=0xff;self.signals.inte=false;self.signals.prot=false;self.signals.psync=false;self.signals.pdbin=false;self.signals.pwr_n=true;self.signals.clear_status();self.signals.run=run;self.signals.front_panel_ready=run;self.signals.memory_ready=true;self.signals.ready=run;self.signals.wait=false;self.signals.hlda=false;self.lamps.freeze(&self.signals);}
    pub(super) fn release_front_panel_reset(&mut self,address:u16,data:u8,protected:bool,inte:bool,run:bool){self.signals.reset=false;self.signals.owner=BusOwner::Cpu;self.signals.address=address;self.signals.data_in=Some(data);self.signals.data_out=None;self.signals.cpu_data=(!run).then_some(data);self.signals.panel_data=data;self.signals.prot=protected;self.signals.inte=inte;self.signals.run=run;self.signals.psync=false;self.signals.pdbin=!run;self.signals.pwr_n=true;self.signals.clear_status();self.signals.apply_status_word(STATUS_INSTRUCTION_FETCH);self.signals.front_panel_ready=run;self.signals.memory_ready=true;self.signals.ready=run;self.signals.wait=!run;self.signals.hlda=false;self.lamps.freeze(&self.signals);}
    pub(super) fn drive_power_on_state(&mut self,address:u16,data:u8,protected:bool,inte:bool,run:bool){self.signals.reset=false;self.signals.owner=BusOwner::Cpu;self.signals.address=address;self.signals.data_in=Some(data);self.signals.data_out=None;self.signals.cpu_data=(!run).then_some(data);self.signals.panel_data=data;self.signals.prot=protected;self.signals.inte=inte;self.signals.run=run;self.signals.psync=false;self.signals.pdbin=!run;self.signals.pwr_n=true;self.signals.clear_status();if run{self.set_ready(true);}else{self.signals.apply_status_word(STATUS_INSTRUCTION_FETCH);self.signals.front_panel_ready=false;self.signals.memory_ready=true;self.signals.ready=false;self.signals.wait=true;}self.lamps.freeze(&self.signals);}
    pub(super) fn drive_front_panel_deposit(&mut self,address:u16,data:u8,protected:bool,inte:bool){self.signals.reset=false;self.signals.owner=BusOwner::FrontPanel;self.signals.address=address;self.signals.cpu_data=Some(data);self.signals.data_in=None;self.signals.data_out=Some(data);self.signals.prot=protected;self.signals.inte=inte;self.signals.psync=false;self.signals.pdbin=false;self.signals.pwr_n=true;self.signals.clear_status();self.signals.wo=false;self.set_ready(false);self.lamps.freeze(&self.signals);}
    pub(super) fn refresh_protect(&mut self,p:bool){self.signals.prot=p;}
    pub(super) fn freeze(&mut self){self.lamps.freeze(&self.signals);}
    pub(super) fn commit(&mut self,dt:Duration,dynamic:bool){self.lamps.commit(&self.signals,dt,dynamic);}
}

impl super::AltairBus {
    pub fn raw_panel_lamp_duty(&self)->PanelLampSnapshot{self.s100.raw_duty_snapshot()}
    #[inline]
    pub(crate) fn cycle_full_project_panel_cycle(&mut self,address:u16,data:u8,status_word:u8,t_states:u32,reads:bool,writes:bool,inte:bool){let protected=self.memory.is_protected(address);debug_assert!(self.s100.signals().ready,"Cycle Full requires READY high");self.s100.drive_cycle_full_reconstructed_cpu_cycle(address,data,status_word,t_states,reads,writes,protected,inte,true,false);}
    #[inline]
    pub(crate) fn cycle_full_project_internal_t_states(&mut self,t_states:u32,inte:bool){debug_assert!(self.s100.signals().ready,"Cycle Full requires READY high");self.s100.drive_cycle_full_internal_t_states(t_states,inte,true,false);}
}
#[cfg(test)] impl super::AltairBus { pub(crate) fn debug_set_panel_lamp_snapshot_for_test(&mut self,snapshot:PanelLampSnapshot){self.s100.debug_set_snapshot(snapshot);} }
fn bits16(value:u16)->[f32;16]{let mut bits=[0.0;16];for bit in 0..16{bits[bit]=if value&(1u16<<bit)!=0{1.0}else{0.0};}bits}
fn bits8(value:u8)->[f32;8]{let mut bits=[0.0;8];for bit in 0..8{bits[bit]=if value&(1u8<<bit)!=0{1.0}else{0.0};}bits}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn raw_duty_counts_the_entire_interval_instead_of_the_first_visual_window(){let mut i=PanelLampIntegrator::default();let off=S100Signals::default();let mut on=off;on.memr=true;i.sample(&off,40_000);i.sample(&on,40_000);i.commit(&on,Duration::from_millis(16),true);assert_eq!(i.raw_duty.memr,0.5);assert_eq!(i.total_weight,0);}
    #[test] fn raw_duty_is_sample_order_invariant(){let off=S100Signals::default();let mut on=off;on.m1=true;let mut f=PanelLampIntegrator::default();f.sample(&on,3);f.sample(&off,7);f.commit(&off,Duration::from_millis(16),true);let mut r=PanelLampIntegrator::default();r.sample(&off,7);r.sample(&on,3);r.commit(&off,Duration::from_millis(16),true);assert_eq!(f.raw_duty,r.raw_duty);assert_eq!(f.raw_duty.m1,0.3);}
    #[test] fn raw_electrical_duty_is_not_the_optically_persistent_snapshot(){let mut i=PanelLampIntegrator::default();let mut on=S100Signals::default();on.wo=true;i.sample(&on,100);i.commit(&on,Duration::from_millis(1),true);assert_eq!(i.raw_duty.wo,1.0);assert!(i.snapshot.wo>0.0&&i.snapshot.wo<1.0);}
    #[test] fn reconstructed_cycle_matches_expanded_fetch_duty(){let mut e=S100BusState::default();let mut p=S100BusState::default();for b in [&mut e,&mut p]{b.release_front_panel_reset(0,0x5a,false,false,true);b.lamps.clear_activity();}e.drive_cpu_t_state(Some(1),Some(0xa2),None,Some(0xa2),Some(0xa2),false,false,true,false,false);for _ in 0..3{e.drive_cpu_t_state(Some(1),Some(0x33),Some(0x33),None,None,false,false,true,false,false);}p.drive_reconstructed_cpu_cycle(1,0x33,0xa2,4,true,false,false,false,true,false);assert_eq!(p.lamps.total_weight,e.lamps.total_weight);assert_eq!(p.lamps.raw_duty_snapshot(),e.lamps.raw_duty_snapshot());assert_eq!(p.signals().address,e.signals().address);assert_eq!(p.signals().panel_data,e.signals().panel_data);assert_eq!(p.signals().lamp_mask(),e.signals().lamp_mask());}
    #[test] fn cycle_full_weighted_cycle_matches_expanded_8212_latch_delay(){let mut e=S100BusState::default();let mut p=S100BusState::default();for b in [&mut e,&mut p]{b.release_front_panel_reset(0,0x5a,false,false,true);b.lamps.clear_activity();}e.drive_cpu_t_state(Some(0x1234),Some(0x82),None,Some(0x82),None,false,false,true,false,false);e.drive_cpu_t_state(None,Some(0x33),Some(0x33),None,Some(0x82),false,false,true,false,false);e.drive_cpu_t_state(None,Some(0x33),Some(0x33),None,None,false,false,true,false,false);p.drive_cycle_full_reconstructed_cpu_cycle(0x1234,0x33,0x82,3,true,false,false,false,true,false);assert_eq!(p.lamps.total_weight,e.lamps.total_weight);assert_eq!(p.lamps.raw_duty_snapshot(),e.lamps.raw_duty_snapshot());assert_eq!(p.signals().address,e.signals().address);assert_eq!(p.signals().panel_data,e.signals().panel_data);assert_eq!(p.signals().lamp_mask(),e.signals().lamp_mask());}
    #[test] fn cpu_board_edge_lines_are_first_class_backplane_state(){let mut b=S100BusState::default();b.drive_cpu_board_edge(true,false,true,false,true);let s=b.signals();assert_eq!(s.phi1,Some(true));assert_eq!(s.cloc,Some(true));b.drive_cpu_board_edge(false,true,false,true,true);let s=b.signals();assert_eq!(s.phi2,Some(true));assert_eq!(s.cloc,Some(false));}
    #[test] fn status_latch_changes_only_when_cpu_board_clocks_it(){let mut b=S100BusState::default();b.drive_cpu_t_state(Some(0x1234),Some(0xa2),None,Some(0xa2),None,false,false,true,false,false);assert!(!b.signals().m1);b.latch_cpu_status(0xa2);assert!(b.signals().m1&&b.signals().memr&&b.signals().wo);}
    #[test] fn intel_status_and_read_data_keep_cpu_di_do_domains_distinct(){let mut b=S100BusState::default();b.drive_cpu_t_state(Some(0x1234),Some(0xa2),None,Some(0xa2),Some(0xa2),false,false,true,false,false);assert_eq!(b.signals().panel_data,0);b.drive_cpu_t_state(Some(0x1234),Some(0x56),Some(0x56),None,None,false,false,true,false,false);assert_eq!(b.signals().panel_data,0x56);}
    #[test] fn write_data_uses_do_and_does_not_replace_front_panel_di_value(){let mut b=S100BusState::default();b.drive_cpu_t_state(Some(0x0100),Some(0x5a),Some(0x5a),None,Some(0x82),false,false,true,false,false);b.drive_cpu_t_state(Some(0x1234),Some(0xaa),None,Some(0xaa),Some(0x00),false,false,true,false,false);assert_eq!(b.signals().panel_data,0x5a);}
    #[test] fn t_state_path_samples_once_and_preserves_latched_status_on_internal_states(){let mut b=S100BusState::default();b.drive_cpu_t_state(Some(0x1234),Some(0xa2),None,Some(0xa2),Some(0xa2),false,false,true,false,false);assert_eq!(b.lamps.total_weight,1);b.drive_cpu_t_state(None,None,None,None,None,false,false,true,false,false);assert_eq!(b.lamps.total_weight,2);assert_eq!(b.signals().address,0x1234);}
    #[test] fn hold_ack_releases_cpu_bus_and_clears_status_without_faking_panel_data(){let mut b=S100BusState::default();b.drive_cpu_t_state(Some(0x4567),Some(0x6c),Some(0x6c),None,Some(0x82),false,false,true,false,false);b.drive_cpu_t_state(None,None,None,None,None,false,false,true,false,true);let s=b.signals();assert_eq!(s.owner,BusOwner::None);assert_eq!(s.panel_data,0x6c);assert!(s.hlda);}
    #[test] fn front_panel_reset_is_not_reported_as_cpu_package_bus_drive(){let mut b=S100BusState::default();b.assert_front_panel_reset(false);let s=b.signals();assert_eq!(s.owner,BusOwner::FrontPanel);assert_eq!(s.address,0xffff);assert!(s.reset);}
    #[test] fn interrupt_request_and_acknowledge_are_distinct_lines(){let mut b=S100BusState::default();b.set_interrupt_request(true);b.drive_cpu_t_state(Some(0x0100),Some(0x23),None,Some(0x23),Some(0x23),false,false,true,false,false);assert!(b.signals().interrupt&&b.signals().int_ack);}
    #[test] fn stopped_power_on_is_a_cpu_fetch_wait_with_memory_on_di(){let mut b=S100BusState::default();b.drive_power_on_state(0x4321,0xa5,false,false,false);let s=b.signals();assert!(s.memr&&s.m1&&s.wo&&s.wait);assert!(!s.ready);}
    #[test] fn reset_preserves_run_latch_and_changes_ready_on_release(){let mut b=S100BusState::default();b.set_run(true);b.assert_front_panel_reset(true);b.release_front_panel_reset(0,0xa5,false,false,true);assert!(b.signals().run&&b.signals().ready&&!b.signals().wait);}
    #[test] fn front_panel_deposit_drives_cpu_d_and_do_without_overwriting_di_display(){let mut b=S100BusState::default();b.release_front_panel_reset(0x0100,0x33,false,false,false);b.drive_front_panel_deposit(0x0100,0xa5,false,false);let s=b.signals();assert_eq!(s.data_out,Some(0xa5));assert_eq!(s.panel_data,0x33);}
    #[test] fn front_panel_data_lamps_follow_di_not_cpu_status_or_do(){let mut b=S100BusState::default();b.drive_power_on_state(0,0x3c,false,false,false);b.drive_cpu_t_state(Some(0),Some(0xa2),None,Some(0xa2),Some(0xa2),false,false,true,false,false);assert_eq!(b.signals().panel_data,0x3c);b.drive_cpu_t_state(Some(0),Some(0x7e),Some(0x7e),None,None,false,false,true,false,false);assert_eq!(b.signals().panel_data,0x7e);}
    #[test] fn external_data_bus_is_released_during_hold_acknowledge(){let mut b=S100BusState::default();b.drive_cpu_t_state(Some(0x1234),Some(0xa2),None,Some(0xa2),Some(0xa2),false,false,true,false,false);b.drive_cpu_t_state(None,None,None,None,None,false,false,true,false,true);let s=b.signals();assert_eq!(s.cpu_data,None);assert_eq!(s.owner,BusOwner::None);}
}
#[cfg(test)] mod ready_source_tests { use super::*; #[test] fn memory_ready_is_wired_with_front_panel_ready(){let mut b=S100BusState::default();b.set_ready_input(true);assert!(b.signals().ready);b.set_memory_ready_input(false);assert!(!b.signals().ready);b.set_memory_ready_input(true);assert!(b.signals().ready);} }
#[cfg(test)] mod reset_run_ready_tests { use super::*; #[test] fn run_latch_keeps_prdy_released_while_reset_is_held(){let mut b=S100BusState::default();b.assert_front_panel_reset(true);let s=b.signals();assert!(s.reset&&s.run&&s.ready&&!s.wait);} #[test] fn stopped_latch_keeps_prdy_low_while_reset_is_held(){let mut b=S100BusState::default();b.assert_front_panel_reset(false);let s=b.signals();assert!(s.reset&&!s.run&&!s.ready&&!s.wait);} }

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

// Presentation persistence only. The emulated hardware state remains binary;
// this low-pass maps MHz bus transitions onto a ~60 Hz host display.
const VISUAL_PERSISTENCE_SECS: f32 = 0.045;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BusOwner {
    None,
    Cpu,
    FrontPanel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum S100Cycle {
    InstructionFetch,
    MemoryRead,
    MemoryWrite,
    StackRead,
    StackWrite,
    InputRead,
    OutputWrite,
    InterruptAcknowledge,
    HaltAcknowledge,
    InterruptAcknowledgeWhileHalted,
}

impl S100Cycle {
    fn status_word(self) -> u8 {
        match self {
            Self::InstructionFetch => 0xA2,
            Self::MemoryRead => 0x82,
            Self::MemoryWrite => 0x00,
            Self::StackRead => 0x86,
            Self::StackWrite => 0x04,
            Self::InputRead => 0x42,
            Self::OutputWrite => 0x10,
            Self::InterruptAcknowledge => 0x23,
            Self::HaltAcknowledge => 0x8A,
            Self::InterruptAcknowledgeWhileHalted => 0x2B,
        }
    }

    fn t_states(self) -> u32 {
        match self {
            Self::InstructionFetch | Self::HaltAcknowledge => 4,
            _ => 3,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct S100Signals {
    pub address: u16,
    pub data: u8,
    pub memr: bool,
    pub inp: bool,
    pub m1: bool,
    pub out: bool,
    pub hlta: bool,
    pub stack: bool,
    /// Front-panel W/O indicator follows the physical active-low /WO line:
    /// lit for read/input cycles, dark for write/output cycles.
    pub wo: bool,
    pub int_ack: bool,
    pub inte: bool,
    pub prot: bool,
    /// Q output of the Display/Control RUN/STOP R-S latch (S-100 pin 71).
    pub run: bool,
    pub ready: bool,
    pub wait: bool,
    pub hold: bool,
    pub hlda: bool,
    pub reset: bool,
    /// EXT CLR, S-100 pin 54. `true` means the front-panel line is asserted.
    pub ext_clear: bool,
    pub owner: BusOwner,
}

impl Default for S100Signals {
    fn default() -> Self {
        Self {
            address: 0,
            data: 0,
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
            wait: false,
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
        [
            self.inte,
            self.prot,
            self.memr,
            self.inp,
            self.m1,
            self.out,
            self.hlta,
            self.stack,
            self.wo,
            self.int_ack,
            self.wait,
            self.hlda,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
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
        Self {
            address: [0.0; 16],
            data: [0.0; 8],
            inte: 0.0,
            prot: 0.0,
            memr: 0.0,
            inp: 0.0,
            m1: 0.0,
            out: 0.0,
            hlta: 0.0,
            stack: 0.0,
            wo: 0.0,
            int_ack: 0.0,
            wait: 0.0,
            hlda: 0.0,
        }
    }
}

impl PanelLampSnapshot {
    fn lamp_array(self) -> [f32; LAMP_COUNT] {
        [
            self.inte,
            self.prot,
            self.memr,
            self.inp,
            self.m1,
            self.out,
            self.hlta,
            self.stack,
            self.wo,
            self.int_ack,
            self.wait,
            self.hlda,
        ]
    }

    fn set_lamp_array(&mut self, values: [f32; LAMP_COUNT]) {
        self.inte = values[LAMP_INTE];
        self.prot = values[LAMP_PROT];
        self.memr = values[LAMP_MEMR];
        self.inp = values[LAMP_INP];
        self.m1 = values[LAMP_M1];
        self.out = values[LAMP_OUT];
        self.hlta = values[LAMP_HLTA];
        self.stack = values[LAMP_STACK];
        self.wo = values[LAMP_WO];
        self.int_ack = values[LAMP_INT];
        self.wait = values[LAMP_WAIT];
        self.hlda = values[LAMP_HLDA];
    }
}

#[derive(Default)]
struct PanelLampIntegrator {
    address_on: [u64; 16],
    data_on: [u64; 8],
    lamps_on: [u64; LAMP_COUNT],
    total_weight: u64,
    snapshot: PanelLampSnapshot,
}

impl PanelLampIntegrator {
    fn sample(&mut self, signals: &S100Signals, weight: u32) {
        let weight = u64::from(weight);
        if weight == 0 {
            return;
        }
        for bit in 0..16 {
            if signals.address & (1u16 << bit) != 0 {
                self.address_on[bit] += weight;
            }
        }
        for bit in 0..8 {
            if signals.data & (1u8 << bit) != 0 {
                self.data_on[bit] += weight;
            }
        }
        let lamps = signals.lamp_states();
        for bit in 0..LAMP_COUNT {
            if lamps[bit] {
                self.lamps_on[bit] += weight;
            }
        }
        self.total_weight += weight;
    }

    fn freeze(&mut self, signals: &S100Signals) {
        self.clear_activity();
        self.snapshot.address = bits16(signals.address);
        self.snapshot.data = bits8(signals.data);
        let states = signals.lamp_states();
        let mut lamps = [0.0; LAMP_COUNT];
        for bit in 0..LAMP_COUNT {
            lamps[bit] = if states[bit] { 1.0 } else { 0.0 };
        }
        self.snapshot.set_lamp_array(lamps);
    }

    fn commit(&mut self, signals: &S100Signals, dt: Duration, dynamic: bool) {
        if !dynamic {
            self.freeze(signals);
            return;
        }
        if self.total_weight == 0 {
            self.sample(signals, 1);
        }

        let total = self.total_weight as f32;
        let mut target = PanelLampSnapshot::default();
        for bit in 0..16 {
            target.address[bit] = self.address_on[bit] as f32 / total;
        }
        for bit in 0..8 {
            target.data[bit] = self.data_on[bit] as f32 / total;
        }
        let mut target_lamps = [0.0; LAMP_COUNT];
        for bit in 0..LAMP_COUNT {
            target_lamps[bit] = self.lamps_on[bit] as f32 / total;
        }
        target.set_lamp_array(target_lamps);

        let dt_secs = dt.as_secs_f32().max(0.000_001);
        let retention = (-dt_secs / VISUAL_PERSISTENCE_SECS).exp().clamp(0.0, 1.0);
        let inject = 1.0 - retention;
        for bit in 0..16 {
            self.snapshot.address[bit] =
                self.snapshot.address[bit] * retention + target.address[bit] * inject;
        }
        for bit in 0..8 {
            self.snapshot.data[bit] =
                self.snapshot.data[bit] * retention + target.data[bit] * inject;
        }
        let old_lamps = self.snapshot.lamp_array();
        let new_lamps = target.lamp_array();
        let mut mixed = [0.0; LAMP_COUNT];
        for bit in 0..LAMP_COUNT {
            mixed[bit] = old_lamps[bit] * retention + new_lamps[bit] * inject;
        }
        self.snapshot.set_lamp_array(mixed);
        self.clear_activity();
    }

    fn clear(&mut self) {
        self.clear_activity();
        self.snapshot = PanelLampSnapshot::default();
    }

    fn clear_activity(&mut self) {
        self.address_on.fill(0);
        self.data_on.fill(0);
        self.lamps_on.fill(0);
        self.total_weight = 0;
    }
}

pub(super) struct S100BusState {
    signals: S100Signals,
    lamps: PanelLampIntegrator,
}

impl Default for S100BusState {
    fn default() -> Self {
        Self {
            signals: S100Signals::default(),
            lamps: PanelLampIntegrator::default(),
        }
    }
}

impl S100BusState {
    pub(super) fn signals(&self) -> S100Signals {
        self.signals
    }

    pub(super) fn snapshot(&self) -> PanelLampSnapshot {
        self.lamps.snapshot
    }

    pub(super) fn power_off(&mut self) {
        self.signals = S100Signals::default();
        self.lamps.clear();
    }

    pub(super) fn set_inte(&mut self, enabled: bool) {
        self.signals.inte = enabled;
    }

    pub(super) fn set_run(&mut self, run: bool) {
        self.signals.run = run;
    }

    pub(super) fn set_ready(&mut self, ready: bool) {
        self.signals.ready = ready;
        self.signals.wait = !ready && !self.signals.reset;
    }

    pub(super) fn set_hold(&mut self, hold: bool) {
        self.signals.hold = hold;
        if !hold {
            self.signals.hlda = false;
        }
    }

    pub(super) fn set_hlda(&mut self, acknowledged: bool) {
        self.signals.hlda = acknowledged;
    }

    pub(super) fn set_ext_clear(&mut self, asserted: bool) {
        self.signals.ext_clear = asserted;
    }

    pub(super) fn drive_cpu_cycle(
        &mut self,
        address: u16,
        data: u8,
        cycle: S100Cycle,
        protected: bool,
        inte: bool,
    ) {
        self.signals.reset = false;
        self.signals.owner = BusOwner::Cpu;
        self.signals.address = address;
        self.signals.prot = protected;
        self.signals.inte = inte;
        self.signals.apply_status_word(cycle.status_word());

        self.signals.data = cycle.status_word();
        self.lamps.sample(&self.signals, 1);
        self.signals.data = data;
        self.lamps
            .sample(&self.signals, cycle.t_states().saturating_sub(1));
    }

    /// Electrical state while the physical RESET switch is held. MITS' 1975
    /// checkout procedure specifies all ADDRESS/DATA lamps on and all status
    /// lamps off for this phase.
    pub(super) fn assert_front_panel_reset(&mut self) {
        self.signals.reset = true;
        self.signals.owner = BusOwner::FrontPanel;
        self.signals.address = 0xffff;
        self.signals.data = 0xff;
        self.signals.inte = false;
        self.signals.prot = false;
        self.signals.clear_status();
        self.signals.ready = false;
        self.signals.wait = false;
        self.signals.hlda = false;
        self.lamps.freeze(&self.signals);
    }

    /// State reached after RESET is released at location zero. If RUN is set,
    /// READY is released and execution can continue; if STOP is set, the CPU is
    /// held in the fetch cycle at zero. RESET itself never changes RUN/STOP.
    pub(super) fn release_front_panel_reset(
        &mut self,
        address: u16,
        data: u8,
        protected: bool,
        inte: bool,
        run: bool,
    ) {
        self.signals.reset = false;
        self.signals.owner = BusOwner::Cpu;
        self.signals.address = address;
        self.signals.data = data;
        self.signals.prot = protected;
        self.signals.inte = inte;
        self.signals.run = run;
        self.signals.clear_status();
        self.signals.apply_status_word(S100Cycle::InstructionFetch.status_word());
        self.signals.ready = run;
        self.signals.wait = !run;
        self.signals.hlda = false;
        self.lamps.freeze(&self.signals);
    }

    /// Power-on bus state. With the safe default RUN/STOP latch forced to STOP,
    /// the real machine is still a CPU-owned bus stalled in an instruction
    /// fetch at its undefined power-on PC: MEMR, M1, W/O and WAIT are therefore
    /// visible while ADDRESS/DATA remain dependent on the undefined CPU/RAM
    /// state. Historical mode may instead power up with RUN set.
    pub(super) fn drive_power_on_state(
        &mut self,
        address: u16,
        data: u8,
        protected: bool,
        inte: bool,
        run: bool,
    ) {
        self.signals.reset = false;
        self.signals.owner = BusOwner::Cpu;
        self.signals.address = address;
        self.signals.data = data;
        self.signals.prot = protected;
        self.signals.inte = inte;
        self.signals.run = run;
        self.signals.clear_status();
        if run {
            self.set_ready(true);
        } else {
            self.signals.apply_status_word(S100Cycle::InstructionFetch.status_word());
            self.signals.ready = false;
            self.signals.wait = true;
        }
        self.lamps.freeze(&self.signals);
    }

    pub(super) fn drive_front_panel_examine(
        &mut self,
        address: u16,
        data: u8,
        protected: bool,
        inte: bool,
    ) {
        self.signals.reset = false;
        self.signals.owner = BusOwner::FrontPanel;
        self.signals.address = address;
        self.signals.data = data;
        self.signals.prot = protected;
        self.signals.inte = inte;
        self.signals.clear_status();
        self.signals.memr = true;
        self.signals.m1 = true;
        self.signals.wo = true;
        self.set_ready(false);
        self.lamps.freeze(&self.signals);
    }

    pub(super) fn drive_front_panel_deposit(
        &mut self,
        address: u16,
        data: u8,
        protected: bool,
        inte: bool,
    ) {
        self.signals.reset = false;
        self.signals.owner = BusOwner::FrontPanel;
        self.signals.address = address;
        self.signals.data = data;
        self.signals.prot = protected;
        self.signals.inte = inte;
        self.signals.clear_status();
        // A write/output cycle drives /WO low, so the physical W/O lamp is dark.
        self.signals.wo = false;
        self.set_ready(false);
        self.lamps.freeze(&self.signals);
    }

    pub(super) fn refresh_protect(&mut self, protected: bool) {
        self.signals.prot = protected;
    }

    pub(super) fn freeze(&mut self) {
        self.lamps.freeze(&self.signals);
    }

    pub(super) fn commit(&mut self, dt: Duration, dynamic: bool) {
        self.lamps.commit(&self.signals, dt, dynamic);
    }
}

fn bits16(value: u16) -> [f32; 16] {
    let mut bits = [0.0; 16];
    for bit in 0..16 {
        bits[bit] = if value & (1u16 << bit) != 0 { 1.0 } else { 0.0 };
    }
    bits
}

fn bits8(value: u8) -> [f32; 8] {
    let mut bits = [0.0; 8];
    for bit in 0..8 {
        bits[bit] = if value & (1u8 << bit) != 0 { 1.0 } else { 0.0 };
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_status_words_drive_s100_status_lines_with_physical_wo_polarity() {
        let mut bus = S100BusState::default();
        bus.set_ready(true);
        bus.drive_cpu_cycle(0x1234, 0x56, S100Cycle::InstructionFetch, false, false);
        let s = bus.signals();
        assert_eq!(s.address, 0x1234);
        assert_eq!(s.data, 0x56);
        assert!(s.memr);
        assert!(s.m1);
        assert!(s.wo);

        bus.drive_cpu_cycle(0x1234, 0xaa, S100Cycle::MemoryWrite, false, false);
        let s = bus.signals();
        assert!(!s.memr);
        assert!(!s.wo);
    }

    #[test]
    fn stopped_power_on_is_a_cpu_fetch_wait_not_front_panel_idle() {
        let mut bus = S100BusState::default();
        bus.drive_power_on_state(0x4321, 0xa5, false, false, false);
        let s = bus.signals();
        assert_eq!(s.owner, BusOwner::Cpu);
        assert_eq!(s.address, 0x4321);
        assert_eq!(s.data, 0xa5);
        assert!(s.memr && s.m1 && s.wo && s.wait);
        assert!(!s.ready);
    }

    #[test]
    fn reset_preserves_run_latch_and_changes_ready_on_release() {
        let mut bus = S100BusState::default();
        bus.set_run(true);
        bus.assert_front_panel_reset();
        assert!(bus.signals().run);
        bus.release_front_panel_reset(0, 0xa5, false, false, true);
        let running = bus.signals();
        assert!(running.run && running.ready && !running.wait);
        assert_eq!(running.owner, BusOwner::Cpu);

        bus.set_run(false);
        bus.assert_front_panel_reset();
        bus.release_front_panel_reset(0, 0xa5, false, false, false);
        let stopped = bus.signals();
        assert!(!stopped.run && !stopped.ready && stopped.wait);
        assert!(stopped.memr && stopped.m1 && stopped.wo);
        assert_eq!(stopped.owner, BusOwner::Cpu);
    }

    #[test]
    fn ext_clear_is_a_real_s100_signal() {
        let mut bus = S100BusState::default();
        bus.set_ext_clear(true);
        assert!(bus.signals().ext_clear);
        bus.set_ext_clear(false);
        assert!(!bus.signals().ext_clear);
    }

    #[test]
    fn hold_and_hlda_are_bus_state_not_render_constants() {
        let mut bus = S100BusState::default();
        bus.set_hold(true);
        bus.set_hlda(true);
        bus.freeze();
        assert_eq!(bus.snapshot().hlda, 1.0);
        bus.set_hold(false);
        bus.freeze();
        assert_eq!(bus.snapshot().hlda, 0.0);
    }
}

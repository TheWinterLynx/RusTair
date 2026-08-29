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
const STATUS_INSTRUCTION_FETCH: u8 = 0xa2;

// Presentation persistence only. The emulated hardware state remains binary;
// this low-pass maps MHz bus transitions onto a ~60 Hz host display.
const VISUAL_PERSISTENCE_SECS: f32 = 0.045;
// One 16 ms visual frame at the authentic 2 MHz clock is about 32,000 T-states.
// Accelerated/Unlimited execution may advance far more guest time per host
// frame, but averaging all of it makes the lamps unnaturally static. Limit the
// presentation integrator to one authentic visual window; CPU/bus execution is
// completely unaffected.
const VISUAL_SAMPLE_TSTATES: u64 = 32_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BusOwner {
    None,
    Cpu,
    FrontPanel,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct S100Signals {
    pub address: u16,
    /// S-100 DI0-DI7: data travelling toward the processor board. The original
    /// MITS documentation defines IN/OUT relative to the processor.
    pub data_in: Option<u8>,
    /// S-100 DO0-DO7: data travelling away from the processor board.
    pub data_out: Option<u8>,
    /// Intel 8080 package D0-D7 after the CPU-board input/output buffering.
    /// `None` represents a released/undriven processor data bus.
    pub cpu_data: Option<u8>,
    /// Electrical source used by the eight front-panel DATA lamps. Schematic
    /// 880-105 wires those lamps to S-100 DI0-DI7, not DO0-DO7. Keep the last
    /// DI value here when DI is temporarily undriven so write/status activity
    /// cannot masquerade as front-panel input data.
    pub panel_data: u8,
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
    /// Effective PRDY level seen by the CPU after all S-100 contributors.
    pub ready: bool,
    /// Display/Control contribution to PRDY (RUN/SINGLE STEP/EXAMINE side).
    pub front_panel_ready: bool,
    /// Selected memory-card contribution to PRDY. Slow RAM may pull this low.
    pub memory_ready: bool,
    pub wait: bool,
    /// PINT, S-100 pin 73. Internal booleans use `true` for asserted even
    /// though the physical line is active-low on the original backplane.
    pub interrupt: bool,
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
            data_in: None,
            data_out: None,
            cpu_data: None,
            panel_data: 0,
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
        let remaining = VISUAL_SAMPLE_TSTATES.saturating_sub(self.total_weight);
        let weight = u64::from(weight).min(remaining);
        if weight == 0 {
            return;
        }
        for bit in 0..16 {
            if signals.address & (1u16 << bit) != 0 {
                self.address_on[bit] += weight;
            }
        }
        for bit in 0..8 {
            if signals.panel_data & (1u8 << bit) != 0 {
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
        self.snapshot.data = bits8(signals.panel_data);
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

    fn recompute_ready(&mut self) {
        self.signals.ready = self.signals.front_panel_ready && self.signals.memory_ready;
    }

    /// Instruction-level/Fast compatibility helper. That backend has no exact
    /// T2->TW transition to supply WAIT, so it deliberately approximates WAIT as
    /// the inverse of READY while stopped.
    pub(super) fn set_ready(&mut self, ready: bool) {
        self.signals.front_panel_ready = ready;
        self.signals.memory_ready = true;
        self.recompute_ready();
        self.signals.wait = !self.signals.ready && !self.signals.reset;
    }

    /// Exact CPU-board path: mutate only the Display/Control contribution to
    /// PRDY. Memory cards keep their own wired-AND contribution.
    pub(super) fn set_ready_input(&mut self, ready: bool) {
        self.signals.front_panel_ready = ready;
        self.recompute_ready();
    }

    /// Memory-board PRDY contribution. `true` means the selected card is ready;
    /// `false` means a slow card is actively stretching the read cycle.
    pub(super) fn set_memory_ready_input(&mut self, ready: bool) {
        self.signals.memory_ready = ready;
        self.recompute_ready();
    }

    pub(super) fn set_interrupt_request(&mut self, asserted: bool) {
        self.signals.interrupt = asserted;
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

    /// Drive exactly one CPU T-state into the S-100/front-panel model.
    ///
    /// `cpu_data` is the Intel 8080 package D0-D7 level. `data_in` and
    /// `data_out` are the two physically separate S-100 directions created by
    /// the original CPU-board buffers. The front-panel DATA display follows DI
    /// only (880-105), so DO/status traffic never updates `panel_data`.
    ///
    /// `status_word` is the status latch value associated with the current
    /// machine cycle. `None` means that no new external status byte exists
    /// (for example an internal DAD cycle), so the previously latched S-100
    /// status lines remain untouched.
    pub(super) fn drive_cpu_t_state(
        &mut self,
        address: Option<u16>,
        cpu_data: Option<u8>,
        data_in: Option<u8>,
        data_out: Option<u8>,
        status_word: Option<u8>,
        protected: bool,
        inte: bool,
        ready: bool,
        wait: bool,
        hlda: bool,
    ) {
        self.signals.reset = false;
        self.signals.inte = inte;
        self.signals.ready = ready;
        self.signals.wait = wait;
        self.signals.hlda = hlda;
        self.signals.cpu_data = cpu_data;
        self.signals.data_in = data_in;
        self.signals.data_out = data_out;
        if let Some(data) = data_in {
            self.signals.panel_data = data;
        }

        if hlda {
            self.signals.owner = BusOwner::None;
            self.signals.prot = false;
            self.signals.cpu_data = None;
            self.signals.data_in = None;
            self.signals.data_out = None;
            self.signals.clear_status();
            self.lamps.sample(&self.signals, 1);
            return;
        }

        self.signals.owner = BusOwner::Cpu;
        if let Some(address) = address {
            self.signals.address = address;
            self.signals.prot = protected;
        }
        if let Some(status_word) = status_word {
            self.signals.apply_status_word(status_word);
        }
        self.lamps.sample(&self.signals, 1);
    }

    /// Electrical state while the physical RESET switch is held. MITS' 1975
    /// checkout procedure specifies all ADDRESS/DATA lamps on and all status
    /// lamps off for this phase.
    pub(super) fn assert_front_panel_reset(&mut self) {
        self.signals.reset = true;
        self.signals.owner = BusOwner::FrontPanel;
        self.signals.address = 0xffff;
        self.signals.data_in = Some(0xff);
        self.signals.data_out = None;
        self.signals.cpu_data = None;
        self.signals.panel_data = 0xff;
        self.signals.inte = false;
        self.signals.prot = false;
        self.signals.clear_status();
        self.signals.front_panel_ready = false;
        self.signals.memory_ready = true;
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
        self.signals.data_in = Some(data);
        self.signals.data_out = None;
        self.signals.cpu_data = (!run).then_some(data);
        self.signals.panel_data = data;
        self.signals.prot = protected;
        self.signals.inte = inte;
        self.signals.run = run;
        self.signals.clear_status();
        self.signals.apply_status_word(STATUS_INSTRUCTION_FETCH);
        self.signals.front_panel_ready = run;
        self.signals.memory_ready = true;
        self.signals.ready = run;
        self.signals.wait = !run;
        self.signals.hlda = false;
        self.lamps.freeze(&self.signals);
    }

    /// Power-on bus state. With the safe default RUN/STOP latch forced to STOP,
    /// the real machine is still a CPU-owned bus stalled in an instruction
    /// fetch at its undefined power-on PC: MEMR, M1, W/O and WAIT are therefore
    /// visible while ADDRESS/DI remain dependent on the undefined CPU/RAM state.
    /// Historical mode may instead power up with RUN set.
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
        self.signals.data_in = Some(data);
        self.signals.data_out = None;
        self.signals.cpu_data = (!run).then_some(data);
        self.signals.panel_data = data;
        self.signals.prot = protected;
        self.signals.inte = inte;
        self.signals.run = run;
        self.signals.clear_status();
        if run {
            self.set_ready(true);
        } else {
            self.signals.apply_status_word(STATUS_INSTRUCTION_FETCH);
            self.signals.front_panel_ready = false;
            self.signals.memory_ready = true;
            self.signals.ready = false;
            self.signals.wait = true;
        }
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
        // The D/C board gates SA0-SA7 onto the processor's bidirectional D bus;
        // the CPU-board output buffers therefore present the deposit byte on DO.
        // The DATA lamps remain tied to DI and intentionally retain their last
        // input value during this write pulse.
        self.signals.cpu_data = Some(data);
        self.signals.data_in = None;
        self.signals.data_out = Some(data);
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
    fn intel_status_and_read_data_keep_cpu_di_do_domains_distinct() {
        let mut bus = S100BusState::default();
        bus.drive_cpu_t_state(
            Some(0x1234), Some(0xa2), None, Some(0xa2), Some(0xa2), false, false,
            true, false, false,
        );
        let t1 = bus.signals();
        assert_eq!(t1.cpu_data, Some(0xa2));
        assert_eq!(t1.data_in, None);
        assert_eq!(t1.data_out, Some(0xa2));
        assert_eq!(t1.panel_data, 0x00, "status on CPU D/DO must not drive the DI-wired DATA lamps");

        bus.drive_cpu_t_state(
            Some(0x1234), Some(0x56), Some(0x56), None, None, false, false,
            true, false, false,
        );
        let read = bus.signals();
        assert_eq!(read.address, 0x1234);
        assert_eq!(read.cpu_data, Some(0x56));
        assert_eq!(read.data_in, Some(0x56));
        assert_eq!(read.data_out, None);
        assert_eq!(read.panel_data, 0x56);
        assert!(read.memr);
        assert!(read.m1);
        assert!(read.wo);
    }

    #[test]
    fn write_data_uses_do_and_does_not_replace_front_panel_di_value() {
        let mut bus = S100BusState::default();
        bus.drive_cpu_t_state(
            Some(0x0100), Some(0x5a), Some(0x5a), None, Some(0x82), false, false,
            true, false, false,
        );
        assert_eq!(bus.signals().panel_data, 0x5a);

        bus.drive_cpu_t_state(
            Some(0x1234), Some(0x00), None, Some(0x00), Some(0x00), false, false,
            true, false, false,
        );
        bus.drive_cpu_t_state(
            Some(0x1234), Some(0xaa), None, Some(0xaa), None, false, false,
            true, false, false,
        );
        let write = bus.signals();
        assert_eq!(write.cpu_data, Some(0xaa));
        assert_eq!(write.data_in, None);
        assert_eq!(write.data_out, Some(0xaa));
        assert_eq!(write.panel_data, 0x5a, "DO must not feed the DI-wired DATA lamps");
        assert!(!write.memr);
        assert!(!write.wo);
    }

    #[test]
    fn t_state_path_samples_once_and_preserves_latched_status_on_internal_states() {
        let mut bus = S100BusState::default();
        bus.drive_cpu_t_state(
            Some(0x1234), Some(0xa2), None, Some(0xa2), Some(0xa2), false, false,
            true, false, false,
        );
        assert_eq!(bus.lamps.total_weight, 1);
        let fetch = bus.signals();
        assert_eq!(fetch.address, 0x1234);
        assert_eq!(fetch.cpu_data, Some(0xa2));
        assert!(fetch.memr && fetch.m1 && fetch.wo);

        bus.drive_cpu_t_state(None, None, None, None, None, false, false, true, false, false);
        assert_eq!(bus.lamps.total_weight, 2);
        let internal = bus.signals();
        assert_eq!(internal.address, 0x1234);
        assert_eq!(internal.cpu_data, None);
        assert_eq!(internal.data_in, None);
        assert_eq!(internal.data_out, None);
        assert!(internal.memr && internal.m1 && internal.wo);
    }

    #[test]
    fn t_state_path_relinquishes_all_data_domains_while_hlda_is_asserted() {
        let mut bus = S100BusState::default();
        bus.set_hold(true);
        bus.drive_cpu_t_state(
            Some(0x2000), Some(0x82), None, Some(0x82), Some(0x82), false, true,
            true, false, false,
        );
        bus.drive_cpu_t_state(None, None, None, None, None, false, true, true, false, true);

        let held = bus.signals();
        assert_eq!(held.owner, BusOwner::None);
        assert!(held.hold && held.hlda && held.inte);
        assert_eq!(held.cpu_data, None);
        assert_eq!(held.data_in, None);
        assert_eq!(held.data_out, None);
        assert!(!held.memr && !held.inp && !held.m1 && !held.out && !held.stack);
        assert_eq!(bus.lamps.total_weight, 2);
    }

    #[test]
    fn cycle_ready_input_does_not_fabricate_cpu_wait_output() {
        let mut bus = S100BusState::default();
        bus.drive_cpu_t_state(
            Some(0x0000), Some(0xa2), None, Some(0xa2), Some(0xa2), false, false,
            true, false, false,
        );
        bus.set_ready_input(false);
        let requested = bus.signals();
        assert!(!requested.ready);
        assert!(!requested.wait, "WAIT belongs to the CPU and must remain low until TW is sampled");

        bus.drive_cpu_t_state(
            Some(0x0000), Some(0x00), Some(0x00), None, None, false, false,
            false, true, false,
        );
        assert!(bus.signals().wait);
    }

    #[test]
    fn pint_request_is_an_input_and_does_not_fabricate_sinta() {
        let mut bus = S100BusState::default();
        bus.set_interrupt_request(true);
        assert!(bus.signals().interrupt);
        assert!(!bus.signals().int_ack, "PINT request must not fabricate the CPU SINTA response");

        bus.drive_cpu_t_state(
            Some(0x0100), Some(0x23), None, Some(0x23), Some(0x23), false, false,
            true, false, false,
        );
        assert!(bus.signals().interrupt, "level-sensitive PINT remains asserted until the device clears it");
        assert!(bus.signals().int_ack);

        bus.set_interrupt_request(false);
        assert!(!bus.signals().interrupt);
    }

    #[test]
    fn stopped_power_on_is_a_cpu_fetch_wait_with_memory_on_di() {
        let mut bus = S100BusState::default();
        bus.drive_power_on_state(0x4321, 0xa5, false, false, false);
        let s = bus.signals();
        assert_eq!(s.owner, BusOwner::Cpu);
        assert_eq!(s.address, 0x4321);
        assert_eq!(s.data_in, Some(0xa5));
        assert_eq!(s.data_out, None);
        assert_eq!(s.cpu_data, Some(0xa5));
        assert_eq!(s.panel_data, 0xa5);
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
        assert_eq!(running.data_in, Some(0xa5));
        assert_eq!(running.cpu_data, None, "no exact running CPU D sample exists before the first tick");

        bus.set_run(false);
        bus.assert_front_panel_reset();
        bus.release_front_panel_reset(0, 0xa5, false, false, false);
        let stopped = bus.signals();
        assert!(!stopped.run && !stopped.ready && stopped.wait);
        assert!(stopped.memr && stopped.m1 && stopped.wo);
        assert_eq!(stopped.data_in, Some(0xa5));
        assert_eq!(stopped.cpu_data, Some(0xa5));
        assert_eq!(stopped.owner, BusOwner::Cpu);
    }

    #[test]
    fn front_panel_deposit_drives_cpu_d_and_do_without_overwriting_di_display() {
        let mut bus = S100BusState::default();
        bus.release_front_panel_reset(0x0100, 0x33, false, false, false);
        bus.drive_front_panel_deposit(0x0100, 0xa5, false, false);
        let s = bus.signals();
        assert_eq!(s.cpu_data, Some(0xa5));
        assert_eq!(s.data_out, Some(0xa5));
        assert_eq!(s.data_in, None);
        assert_eq!(s.panel_data, 0x33);
        assert_eq!(bus.snapshot().data, bits8(0x33));
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

    #[test]
    fn accelerated_activity_is_limited_to_one_authentic_visual_window() {
        let mut integrator = PanelLampIntegrator::default();
        let signals = S100Signals {
            address: 0xffff,
            data_in: Some(0xff),
            cpu_data: Some(0xff),
            panel_data: 0xff,
            memr: true,
            ..Default::default()
        };
        integrator.sample(&signals, VISUAL_SAMPLE_TSTATES as u32);
        assert_eq!(integrator.total_weight, VISUAL_SAMPLE_TSTATES);
        integrator.sample(&signals, 1000);
        assert_eq!(integrator.total_weight, VISUAL_SAMPLE_TSTATES);
    }
}

#[cfg(test)]
mod ready_source_tests {
    use super::*;

    #[test]
    fn memory_ready_is_wired_with_front_panel_ready() {
        let mut bus = S100BusState::default();
        bus.set_ready_input(true);
        assert!(bus.signals().ready);
        bus.set_memory_ready_input(false);
        assert!(!bus.signals().ready);
        bus.set_memory_ready_input(true);
        assert!(bus.signals().ready);
        bus.set_ready_input(false);
        assert!(!bus.signals().ready);
    }
}

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
    /// S-100 DI0-DI7: data travelling toward the processor board. The original
    /// MITS documentation defines IN/OUT relative to the processor.
    pub data_in: Option<u8>,
    /// S-100 DO0-DI7: data travelling away from the processor board.
    pub data_out: Option<u8>,
    /// Intel 8080 package D0-D7 after the CPU-board input/output buffering.
    /// `None` represents a released/undriven processor data bus.
    pub cpu_data: Option<u8>,
    /// Electrical source used by the eight front-panel DATA lamps. Schematic
    /// 880-105 wires those lamps to S-100 DI0-DI7, not DO0-DI7. Keep the last
    /// DI value here when DI is temporarily undriven so write/status activity
    /// cannot masquerade as front-panel input data.
    pub panel_data: u8,
    /// Original Altair CPU-board clock outputs on the S-100 backplane.
    /// PHI2 is pin 24, PHI1 pin 25, and CLOC is the separately buffered 2.000 MHz
    /// oscillator on pin 49. `None` means there is no exact instantaneous Cycle
    /// sample; instruction-level Fast must not invent sub-instruction clock state.
    pub phi1: Option<bool>,
    pub phi2: Option<bool>,
    pub cloc: Option<bool>,
    /// Processor command/control outputs buffered by the MITS CPU board:
    /// PSYNC pin 76, active-low PWR pin 77 and PDBIN pin 78.
    pub psync: bool,
    pub pwr_n: bool,
    pub pdbin: bool,
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

struct PanelLampIntegrator {
    /// Bit-sliced unsigned counters. Bit N of every plane is one binary digit of
    /// the accumulated ON time for packed lamp N. A normal Cycle sample with
    /// weight=1 increments all 36 visible lamps in parallel instead of executing
    /// 36 branches and 36 independent saturating additions.
    on_count_planes: [u64; DUTY_COUNTER_PLANES],
    total_weight: u64,
    /// Last complete electrical-duty window, before any optical filtering.
    raw_duty: PanelLampSnapshot,
    /// Human-visible, wall-clock-persistent representation.
    snapshot: PanelLampSnapshot,
}

impl Default for PanelLampIntegrator {
    fn default() -> Self {
        Self {
            on_count_planes: [0; DUTY_COUNTER_PLANES],
            total_weight: 0,
            raw_duty: PanelLampSnapshot::default(),
            snapshot: PanelLampSnapshot::default(),
        }
    }
}

impl PanelLampIntegrator {
    #[inline]
    fn add_mask_at_plane(&mut self, mut carry: u64, mut plane: usize) {
        while carry != 0 && plane < DUTY_COUNTER_PLANES {
            let next = self.on_count_planes[plane] & carry;
            self.on_count_planes[plane] ^= carry;
            carry = next;
            plane += 1;
        }
        debug_assert_eq!(carry, 0, "front-panel duty counter overflow");
    }

    #[inline]
    fn add_weighted_mask(&mut self, mask: u64, weight: u64) {
        let mut remaining = weight;
        let mut plane = 0usize;
        while remaining != 0 {
            if remaining & 1 != 0 {
                self.add_mask_at_plane(mask, plane);
            }
            remaining >>= 1;
            plane += 1;
        }
    }

    #[inline]
    fn sample(&mut self, signals: &S100Signals, weight: u32) {
        let requested = u64::from(weight);
        if requested == 0 {
            return;
        }
        // Preserve the previous saturating behavior even for pathological host
        // intervals: once the total duration reaches u64::MAX there is no further
        // representable electrical time to add to any per-lamp counter.
        let accepted = requested.min(u64::MAX - self.total_weight);
        if accepted == 0 {
            return;
        }
        self.add_weighted_mask(signals.packed_lamp_activity(), accepted);
        self.total_weight += accepted;
    }

    /// Add one reconstructed instruction-level machine cycle without replaying
    /// its identical T2/T3(/T4) presentation samples one at a time. ADDRESS and
    /// latched status are constant across the cycle; only the DI-wired DATA lamp
    /// byte may change after T1 on a read. This is therefore exactly equivalent
    /// to the old Fast adapter's 3/4 calls to `sample`, but reduces them to a few
    /// packed counter additions and does not touch the physical S-100 fabric.
    #[inline]
    fn sample_reconstructed_cycle(
        &mut self,
        common_mask: u64,
        first_panel_data: u8,
        final_panel_data: u8,
        t_states: u32,
        data_changes_after_t1: bool,
    ) {
        let requested = u64::from(t_states);
        if requested == 0 {
            return;
        }
        let accepted = requested.min(u64::MAX - self.total_weight);
        if accepted == 0 {
            return;
        }

        self.add_weighted_mask(common_mask, accepted);
        if data_changes_after_t1 {
            self.add_weighted_mask(u64::from(first_panel_data) << PACKED_DATA_SHIFT, 1);
            if accepted > 1 {
                self.add_weighted_mask(
                    u64::from(final_panel_data) << PACKED_DATA_SHIFT,
                    accepted - 1,
                );
            }
        } else {
            self.add_weighted_mask(
                u64::from(first_panel_data) << PACKED_DATA_SHIFT,
                accepted,
            );
        }
        self.total_weight += accepted;
    }

    fn binary_snapshot(signals: &S100Signals) -> PanelLampSnapshot {
        let mut snapshot = PanelLampSnapshot {
            address: bits16(signals.address),
            data: bits8(signals.panel_data),
            ..PanelLampSnapshot::default()
        };
        let states = signals.lamp_states();
        let mut lamps = [0.0; LAMP_COUNT];
        for bit in 0..LAMP_COUNT {
            lamps[bit] = if states[bit] { 1.0 } else { 0.0 };
        }
        snapshot.set_lamp_array(lamps);
        snapshot
    }

    #[inline]
    fn packed_count(&self, packed_bit: usize) -> u64 {
        let mask = 1u64 << packed_bit;
        let mut value = 0u64;
        for (plane, bits) in self.on_count_planes.iter().copied().enumerate() {
            if bits & mask != 0 {
                value |= 1u64 << plane;
            }
        }
        value
    }

    fn accumulated_duty(&self) -> PanelLampSnapshot {
        debug_assert!(self.total_weight != 0);
        let total = self.total_weight as f32;
        let mut duty = PanelLampSnapshot::default();
        for bit in 0..16 {
            duty.address[bit] = self.packed_count(bit) as f32 / total;
        }
        for bit in 0..8 {
            duty.data[bit] = self.packed_count(PACKED_DATA_SHIFT + bit) as f32 / total;
        }
        let mut lamps = [0.0; LAMP_COUNT];
        for bit in 0..LAMP_COUNT {
            lamps[bit] = self.packed_count(PACKED_LAMP_SHIFT + bit) as f32 / total;
        }
        duty.set_lamp_array(lamps);
        duty
    }

    fn raw_duty_snapshot(&self) -> PanelLampSnapshot {
        if self.total_weight == 0 {
            self.raw_duty
        } else {
            self.accumulated_duty()
        }
    }

    fn freeze(&mut self, signals: &S100Signals) {
        self.clear_activity();
        let instant = Self::binary_snapshot(signals);
        self.raw_duty = instant;
        self.snapshot = instant;
    }

    fn commit(&mut self, signals: &S100Signals, dt: Duration, dynamic: bool) {
        if !dynamic {
            self.freeze(signals);
            return;
        }
        if self.total_weight == 0 {
            self.sample(signals, 1);
        }

        let target = self.accumulated_duty();
        self.raw_duty = target;

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
        self.raw_duty = PanelLampSnapshot::default();
        self.snapshot = PanelLampSnapshot::default();
    }

    fn clear_activity(&mut self) {
        self.on_count_planes.fill(0);
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

    pub(super) fn raw_duty_snapshot(&self) -> PanelLampSnapshot {
        self.lamps.raw_duty_snapshot()
    }

    #[cfg(test)]
    pub(super) fn debug_set_snapshot(&mut self, snapshot: PanelLampSnapshot) {
        self.lamps.clear_activity();
        self.lamps.raw_duty = snapshot;
        self.lamps.snapshot = snapshot;
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

    /// Drive the CPU-board clock and command/control outputs exactly at one
    /// observed edge. These are real backplane nets, not UI reconstructions:
    /// PHI2=pin24, PHI1 pin 25, CLOC=pin49, PSYNC=pin76, /PWR=pin77, PDBIN=pin78.
    pub(super) fn drive_cpu_board_edge(
        &mut self,
        phi1: bool,
        phi2: bool,
        psync: bool,
        pdbin: bool,
        pwr_n: bool,
    ) {
        self.signals.phi1 = Some(phi1);
        self.signals.phi2 = Some(phi2);
        // MITS derives the processor phases and the buffered 2 MHz CLOC from the
        // same crystal oscillator. At our digital claim boundary PHI1 rising is
        // the high transition of CLOC and PHI2 rising its low transition. During
        // the non-overlap dead-times CLOC retains its previous level; analog
        // pulse width, one-shot tolerance and propagation delay remain non-claims.
        if phi1 {
            self.signals.cloc = Some(true);
        } else if phi2 {
            self.signals.cloc = Some(false);
        }
        self.signals.psync = psync;
        self.signals.pdbin = pdbin;
        self.signals.pwr_n = pwr_n;
    }

    /// The MITS CPU board latches the processor status byte into its 8212 when
    /// SYNC and PHI1 coincide. Cycle calls this only at that physical edge;
    /// instruction-level Fast retains its explicitly reconstructed status path.
    pub(super) fn latch_cpu_status(&mut self, word: u8) {
        self.signals.apply_status_word(word);
    }

    /// Fast/reconstructed counterpart of `drive_cpu_t_state`. The instruction-
    /// level core knows that T1 presents the status byte and the remaining
    /// machine-cycle states present one stable data phase. Fold those repeated
    /// samples directly into the bit-sliced lamp integrator while leaving the
    /// final observable reconstructed bus state identical to the old adapter.
    #[inline]
    pub(super) fn drive_reconstructed_cpu_cycle(
        &mut self,
        address: u16,
        data: u8,
        status_word: u8,
        t_states: u32,
        reads_data_from_s100: bool,
        writes_data_to_s100: bool,
        protected: bool,
        inte: bool,
        ready: bool,
        wait: bool,
    ) {
        debug_assert!(t_states >= 1);
        debug_assert!(!(reads_data_from_s100 && writes_data_to_s100));

        let first_panel_data = self.signals.panel_data;
        self.signals.reset = false;
        self.signals.inte = inte;
        self.signals.ready = ready;
        self.signals.wait = wait;
        self.signals.hlda = false;
        self.signals.owner = BusOwner::Cpu;
        self.signals.address = address;
        self.signals.prot = protected;
        self.signals.apply_status_word(status_word);

        let common_mask = u64::from(address)
            | (u64::from(self.signals.lamp_mask()) << PACKED_LAMP_SHIFT);
        let final_panel_data = if reads_data_from_s100 {
            data
        } else {
            first_panel_data
        };
        self.lamps.sample_reconstructed_cycle(
            common_mask,
            first_panel_data,
            final_panel_data,
            t_states,
            reads_data_from_s100,
        );

        if reads_data_from_s100 {
            self.signals.cpu_data = Some(data);
            self.signals.data_in = Some(data);
            self.signals.data_out = None;
            self.signals.panel_data = data;
        } else if writes_data_to_s100 {
            self.signals.cpu_data = Some(data);
            self.signals.data_in = None;
            self.signals.data_out = Some(data);
        } else {
            self.signals.cpu_data = None;
            self.signals.data_in = None;
            self.signals.data_out = None;
        }
    }

    /// Drive exactly one CPU T-state into the S-100/front-panel model.
    ///
    /// `cpu_data` is the Intel 8080 package D0-D7 level. `data_in` and
    /// `data_out` are the two physically separate S-100 directions created by
    /// the original CPU-board buffers. The front-panel DATA display follows DI
    /// only (880-105), so DO/status traffic never updates `panel_data`.
    ///
    /// `status_word` is used by the instruction-level Fast reconstruction. In
    /// exact Cycle execution the 8212 status latch is driven separately from the
    /// real SYNC+PHI1 edge and this argument is normally `None`.
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
    pub(super) fn assert_front_panel_reset(&mut self, run: bool) {
        self.signals.reset = true;
        self.signals.owner = BusOwner::FrontPanel;
        self.signals.address = 0xffff;
        self.signals.data_in = Some(0xff);
        self.signals.data_out = None;
        self.signals.cpu_data = None;
        self.signals.panel_data = 0xff;
        self.signals.inte = false;
        self.signals.prot = false;
        self.signals.psync = false;
        self.signals.pdbin = false;
        self.signals.pwr_n = true;
        self.signals.clear_status();
        // PRESET/RESET belongs to the processor input path and does not clear
        // the original Display/Control RUN/STOP R-S latch. PRDY therefore still
        // follows RUN while RESET is physically held.
        self.signals.run = run;
        self.signals.front_panel_ready = run;
        self.signals.memory_ready = true;
        self.signals.ready = run;
        // WAIT is an 8080 output and is inactive while RESET is asserted.
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
        self.signals.psync = false;
        self.signals.pdbin = !run;
        self.signals.pwr_n = true;
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
        self.signals.psync = false;
        self.signals.pdbin = !run;
        self.signals.pwr_n = true;
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
        self.signals.psync = false;
        self.signals.pdbin = false;
        self.signals.pwr_n = true;
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

impl super::AltairBus {
    /// Electrical duty accumulated from CPU-board/S-100 samples. This is not the
    /// optically filtered value drawn by the UI and is safe for diagnostics and
    /// deterministic fidelity tests.
    pub fn raw_panel_lamp_duty(&self) -> PanelLampSnapshot {
        self.s100.raw_duty_snapshot()
    }
}

#[cfg(test)]
impl super::AltairBus {
    pub(crate) fn debug_set_panel_lamp_snapshot_for_test(&mut self, snapshot: PanelLampSnapshot) {
        self.s100.debug_set_snapshot(snapshot);
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
    fn raw_duty_counts_the_entire_interval_instead_of_the_first_visual_window() {
        let mut integrator = PanelLampIntegrator::default();
        let off = S100Signals::default();
        let mut on = off;
        on.memr = true;

        // This deliberately exceeds the old 32,000-sample presentation cap in
        // the first OFF half. The correct electrical duty is still exactly 50%.
        integrator.sample(&off, 40_000);
        integrator.sample(&on, 40_000);
        integrator.commit(&on, Duration::from_millis(16), true);

        assert_eq!(integrator.raw_duty.memr, 0.5);
        assert_eq!(integrator.total_weight, 0);
    }

    #[test]
    fn raw_duty_is_sample_order_invariant() {
        let off = S100Signals::default();
        let mut on = off;
        on.m1 = true;

        let mut forward = PanelLampIntegrator::default();
        forward.sample(&on, 3);
        forward.sample(&off, 7);
        forward.commit(&off, Duration::from_millis(16), true);

        let mut reverse = PanelLampIntegrator::default();
        reverse.sample(&off, 7);
        reverse.sample(&on, 3);
        reverse.commit(&off, Duration::from_millis(16), true);

        assert_eq!(forward.raw_duty, reverse.raw_duty);
        assert_eq!(forward.raw_duty.m1, 0.3);
    }

    #[test]
    fn raw_electrical_duty_is_not_the_optically_persistent_snapshot() {
        let mut integrator = PanelLampIntegrator::default();
        let mut on = S100Signals::default();
        on.wo = true;

        integrator.sample(&on, 100);
        integrator.commit(&on, Duration::from_millis(1), true);

        assert_eq!(integrator.raw_duty.wo, 1.0);
        assert!(integrator.snapshot.wo > 0.0);
        assert!(integrator.snapshot.wo < 1.0);
    }

    #[test]
    fn reconstructed_cycle_matches_expanded_fetch_duty() {
        let mut expanded = S100BusState::default();
        let mut packed = S100BusState::default();
        for bus in [&mut expanded, &mut packed] {
            bus.release_front_panel_reset(0, 0x5a, false, false, true);
            bus.lamps.clear_activity();
        }

        expanded.drive_cpu_t_state(
            Some(1), Some(0xa2), None, Some(0xa2), Some(0xa2), false, false,
            true, false, false,
        );
        for _ in 0..3 {
            expanded.drive_cpu_t_state(
                Some(1), Some(0x33), Some(0x33), None, None, false, false,
                true, false, false,
            );
        }
        packed.drive_reconstructed_cpu_cycle(
            1, 0x33, 0xa2, 4, true, false, false, false, true, false,
        );

        assert_eq!(packed.lamps.total_weight, expanded.lamps.total_weight);
        assert_eq!(packed.lamps.raw_duty_snapshot(), expanded.lamps.raw_duty_snapshot());
        assert_eq!(packed.signals().address, expanded.signals().address);
        assert_eq!(packed.signals().panel_data, expanded.signals().panel_data);
        assert_eq!(packed.signals().data_in, expanded.signals().data_in);
        assert_eq!(packed.signals().data_out, expanded.signals().data_out);
        assert_eq!(packed.signals().lamp_mask(), expanded.signals().lamp_mask());
    }

    #[test]
    fn cpu_board_edge_lines_are_first_class_backplane_state() {
        let mut bus = S100BusState::default();
        bus.drive_cpu_board_edge(true, false, true, false, true);
        let phi1 = bus.signals();
        assert_eq!(phi1.phi1, Some(true));
        assert_eq!(phi1.phi2, Some(false));
        assert_eq!(phi1.cloc, Some(true));
        assert!(phi1.psync);
        assert!(!phi1.pdbin);
        assert!(phi1.pwr_n);

        // CLOC is a separate oscillator net: it retains its level through the
        // non-overlap dead time instead of becoming unknown when PHI1/PHI2 are 0.
        bus.drive_cpu_board_edge(false, false, false, false, true);
        let dead_after_phi1 = bus.signals();
        assert_eq!(dead_after_phi1.phi1, Some(false));
        assert_eq!(dead_after_phi1.phi2, Some(false));
        assert_eq!(dead_after_phi1.cloc, Some(true));

        bus.drive_cpu_board_edge(false, true, false, true, true);
        let phi2 = bus.signals();
        assert_eq!(phi2.phi1, Some(false));
        assert_eq!(phi2.phi2, Some(true));
        assert_eq!(phi2.cloc, Some(false));
        assert!(!phi2.psync);
        assert!(phi2.pdbin);

        bus.drive_cpu_board_edge(false, false, false, true, true);
        assert_eq!(bus.signals().cloc, Some(false));
    }

    #[test]
    fn status_latch_changes_only_when_cpu_board_clocks_it() {
        let mut bus = S100BusState::default();
        bus.drive_cpu_t_state(
            Some(0x1234), Some(0xa2), None, Some(0xa2), None, false, false,
            true, false, false,
        );
        assert!(!bus.signals().m1);
        assert!(!bus.signals().memr);
        bus.latch_cpu_status(0xa2);
        assert!(bus.signals().m1);
        assert!(bus.signals().memr);
        assert!(bus.signals().wo);
    }

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
    fn hold_ack_releases_cpu_bus_and_clears_status_without_faking_panel_data() {
        let mut bus = S100BusState::default();
        bus.drive_cpu_t_state(
            Some(0x4567), Some(0x82), None, Some(0x82), Some(0x82), false, false,
            true, false, false,
        );
        bus.drive_cpu_t_state(
            Some(0x4567), Some(0x6c), Some(0x6c), None, None, false, false,
            true, false, false,
        );
        assert_eq!(bus.signals().panel_data, 0x6c);

        bus.drive_cpu_t_state(None, None, None, None, None, false, false, true, false, true);
        let s = bus.signals();
        assert_eq!(s.owner, BusOwner::None);
        assert_eq!(s.cpu_data, None);
        assert_eq!(s.data_in, None);
        assert_eq!(s.data_out, None);
        assert_eq!(s.panel_data, 0x6c, "HLDA must not synthesize DATA lamp activity");
        assert!(!s.memr && !s.m1 && !s.wo);
        assert!(s.hlda);
    }

    #[test]
    fn front_panel_reset_is_not_reported_as_cpu_package_bus_drive() {
        let mut bus = S100BusState::default();
        bus.assert_front_panel_reset(false);
        let s = bus.signals();
        assert_eq!(s.owner, BusOwner::FrontPanel);
        assert_eq!(s.address, 0xffff);
        assert_eq!(s.data_in, Some(0xff));
        assert_eq!(s.data_out, None);
        assert_eq!(s.cpu_data, None);
        assert_eq!(s.panel_data, 0xff);
        assert!(s.reset);
        assert!(!s.wait);
        assert!(!s.psync && !s.pdbin && s.pwr_n);
    }

    #[test]
    fn interrupt_request_and_acknowledge_are_distinct_lines() {
        let mut bus = S100BusState::default();
        bus.set_interrupt_request(true);
        assert!(bus.signals().interrupt);
        assert!(!bus.signals().int_ack);

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
        assert!(!s.psync && s.pdbin && s.pwr_n);
    }

    #[test]
    fn reset_preserves_run_latch_and_changes_ready_on_release() {
        let mut bus = S100BusState::default();
        bus.set_run(true);
        bus.assert_front_panel_reset(true);
        assert!(bus.signals().run);
        assert!(bus.signals().ready);
        assert!(!bus.signals().wait);
        bus.release_front_panel_reset(0, 0xa5, false, false, true);
        let running = bus.signals();
        assert!(running.run && running.ready && !running.wait);
        assert_eq!(running.owner, BusOwner::Cpu);
        assert_eq!(running.data_in, Some(0xa5));
        assert_eq!(running.cpu_data, None, "no exact running CPU D sample exists before the first tick");

        bus.set_run(false);
        bus.assert_front_panel_reset(false);
        assert!(!bus.signals().ready);
        assert!(!bus.signals().wait);
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
        assert_eq!(s.owner, BusOwner::FrontPanel);
        assert!(!s.wo);
    }

    #[test]
    fn front_panel_data_lamps_follow_di_not_cpu_status_or_do() {
        let mut bus = S100BusState::default();
        bus.drive_power_on_state(0x0000, 0x3c, false, false, false);
        assert_eq!(bus.signals().panel_data, 0x3c);

        // Status byte leaves the CPU through DO and must not change DATA LEDs.
        bus.drive_cpu_t_state(
            Some(0x0000), Some(0xa2), None, Some(0xa2), Some(0xa2), false, false,
            true, false, false,
        );
        assert_eq!(bus.signals().panel_data, 0x3c);

        // Memory data on DI is the source that updates the physical DATA LEDs.
        bus.drive_cpu_t_state(
            Some(0x0000), Some(0x7e), Some(0x7e), None, None, false, false,
            true, false, false,
        );
        assert_eq!(bus.signals().panel_data, 0x7e);

        // A CPU write on DO must leave the last DI-derived lamp value alone.
        bus.drive_cpu_t_state(
            Some(0x0001), Some(0x00), None, Some(0x00), Some(0x00), false, false,
            true, false, false,
        );
        bus.drive_cpu_t_state(
            Some(0x0001), Some(0xa5), None, Some(0xa5), None, false, false,
            true, false, false,
        );
        assert_eq!(bus.signals().panel_data, 0x7e);
    }

    #[test]
    fn external_data_bus_is_released_during_hold_acknowledge() {
        let mut bus = S100BusState::default();
        bus.drive_cpu_t_state(
            Some(0x1234), Some(0xa2), None, Some(0xa2), Some(0xa2), false, false,
            true, false, false,
        );
        bus.drive_cpu_t_state(None, None, None, None, None, false, false, true, false, true);
        let s = bus.signals();
        assert_eq!(s.cpu_data, None);
        assert_eq!(s.data_in, None);
        assert_eq!(s.data_out, None);
        assert_eq!(s.panel_data, 0x00);
        assert_eq!(s.owner, BusOwner::None);
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

#[cfg(test)]
mod reset_run_ready_tests {
    use super::*;

    #[test]
    fn run_latch_keeps_prdy_released_while_reset_is_held() {
        let mut bus = S100BusState::default();
        bus.assert_front_panel_reset(true);
        let signals = bus.signals();
        assert!(signals.reset);
        assert!(signals.run);
        assert!(signals.front_panel_ready);
        assert!(signals.ready);
        assert!(!signals.wait, "WAIT is an 8080 output and is inactive during RESET");
    }

    #[test]
    fn stopped_latch_keeps_prdy_low_while_reset_is_held() {
        let mut bus = S100BusState::default();
        bus.assert_front_panel_reset(false);
        let signals = bus.signals();
        assert!(signals.reset);
        assert!(!signals.run);
        assert!(!signals.front_panel_ready);
        assert!(!signals.ready);
        assert!(!signals.wait);
    }
}

use std::time::Duration;

const STATUS_MEMR: usize = 0;
const STATUS_INP: usize = 1;
const STATUS_M1: usize = 2;
const STATUS_OUT: usize = 3;
const STATUS_HLTA: usize = 4;
const STATUS_STACK: usize = 5;
const STATUS_WO: usize = 6;
const STATUS_INT: usize = 7;
const STATUS_COUNT: usize = 8;

// Human-visible persistence. The real LEDs followed the bus electrically; this
// low-pass is only the rendering bridge needed to make MHz bus activity visible
// on a 60-ish Hz display without inventing a latched display register.
const VISUAL_PERSISTENCE_SECS: f32 = 0.045;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PanelCycle {
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

impl PanelCycle {
    /// Raw 8080 status word placed on D7..D0 during SYNC at the beginning of a
    /// machine cycle. Bit 1 is the processor's active-low write/output status:
    /// it is 0 for memory-write/output cycles and 1 for read/input cycles.
    fn status_word(self) -> u8 {
        match self {
            Self::InstructionFetch => 0xA2,              // MEMR | M1 | WO(raw)
            Self::MemoryRead => 0x82,                   // MEMR | WO(raw)
            Self::MemoryWrite => 0x00,
            Self::StackRead => 0x86,                    // MEMR | STACK | WO(raw)
            Self::StackWrite => 0x04,                   // STACK
            Self::InputRead => 0x42,                    // INP | WO(raw)
            Self::OutputWrite => 0x10,                  // OUT
            Self::InterruptAcknowledge => 0x23,         // INTA | M1 | WO(raw)
            Self::HaltAcknowledge => 0x8A,              // MEMR | HLTA | WO(raw)
            Self::InterruptAcknowledgeWhileHalted => 0x2B, // INTA | HLTA | M1 | WO(raw)
        }
    }

    /// Approximate T-state occupancy of the externally visible machine cycle.
    /// We intentionally stay machine-cycle sampled rather than making the CPU
    /// core T-state resumable.
    fn weight(self) -> u32 {
        match self {
            Self::InstructionFetch => 4,
            Self::HaltAcknowledge => 4,
            Self::InterruptAcknowledge | Self::InterruptAcknowledgeWhileHalted => 3,
            _ => 3,
        }
    }

    fn display_status(self) -> [bool; STATUS_COUNT] {
        let word = self.status_word();
        [
            word & 0x80 != 0, // MEMR
            word & 0x40 != 0, // INP
            word & 0x20 != 0, // M1
            word & 0x10 != 0, // OUT
            word & 0x08 != 0, // HLTA
            word & 0x04 != 0, // STACK
            word & 0x02 == 0, // panel WO lamp is active for WRITE/OUTPUT
            word & 0x01 != 0, // INT/INTA
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PanelLampSnapshot {
    pub address: [f32; 16],
    pub data: [f32; 8],
    pub memr: f32,
    pub inp: f32,
    pub m1: f32,
    pub out: f32,
    pub hlta: f32,
    pub stack: f32,
    pub wo: f32,
    pub int_ack: f32,
}

impl Default for PanelLampSnapshot {
    fn default() -> Self {
        Self {
            address: [0.0; 16],
            data: [0.0; 8],
            memr: 0.0,
            inp: 0.0,
            m1: 0.0,
            out: 0.0,
            hlta: 0.0,
            stack: 0.0,
            wo: 0.0,
            int_ack: 0.0,
        }
    }
}

impl PanelLampSnapshot {
    fn status_array(self) -> [f32; STATUS_COUNT] {
        [
            self.memr,
            self.inp,
            self.m1,
            self.out,
            self.hlta,
            self.stack,
            self.wo,
            self.int_ack,
        ]
    }

    fn set_status_array(&mut self, values: [f32; STATUS_COUNT]) {
        self.memr = values[STATUS_MEMR];
        self.inp = values[STATUS_INP];
        self.m1 = values[STATUS_M1];
        self.out = values[STATUS_OUT];
        self.hlta = values[STATUS_HLTA];
        self.stack = values[STATUS_STACK];
        self.wo = values[STATUS_WO];
        self.int_ack = values[STATUS_INT];
    }
}

pub(super) struct PanelBusMonitor {
    live_address: u16,
    live_data: u8,
    live_cycle: Option<PanelCycle>,
    address_on: [u64; 16],
    data_on: [u64; 8],
    status_on: [u64; STATUS_COUNT],
    total_weight: u64,
    snapshot: PanelLampSnapshot,
}

impl Default for PanelBusMonitor {
    fn default() -> Self {
        Self {
            live_address: 0,
            live_data: 0,
            live_cycle: None,
            address_on: [0; 16],
            data_on: [0; 8],
            status_on: [0; STATUS_COUNT],
            total_weight: 0,
            snapshot: PanelLampSnapshot::default(),
        }
    }
}

impl PanelBusMonitor {
    pub(super) fn observe(&mut self, address: u16, data: u8, cycle: PanelCycle) {
        self.live_address = address;
        self.live_data = data;
        self.live_cycle = Some(cycle);

        let weight = u64::from(cycle.weight());
        let status_weight = 1u64.min(weight);
        let data_weight = weight.saturating_sub(status_weight);
        let status_word = cycle.status_word();
        let status = cycle.display_status();

        for bit in 0..16 {
            if address & (1u16 << bit) != 0 {
                self.address_on[bit] += weight;
            }
        }

        // During T1/SYNC the 8080 puts the status word on the bidirectional data
        // bus. For the remaining externally visible portion of the cycle, the
        // actual memory/I/O byte dominates the data lamps.
        for bit in 0..8 {
            if status_word & (1u8 << bit) != 0 {
                self.data_on[bit] += status_weight;
            }
            if data & (1u8 << bit) != 0 {
                self.data_on[bit] += data_weight;
            }
        }

        for bit in 0..STATUS_COUNT {
            if status[bit] {
                self.status_on[bit] += weight;
            }
        }

        self.total_weight += weight;
    }

    pub(super) fn force_static(&mut self, address: u16, data: u8) {
        self.live_address = address;
        self.live_data = data;
        self.live_cycle = None;
        self.clear_activity();
        self.snapshot.address = bits16(address);
        self.snapshot.data = bits8(data);
        self.snapshot.set_status_array([0.0; STATUS_COUNT]);
    }

    pub(super) fn freeze_live(&mut self) {
        self.clear_activity();
        self.snapshot.address = bits16(self.live_address);
        self.snapshot.data = bits8(self.live_data);
        let status = self
            .live_cycle
            .map(PanelCycle::display_status)
            .unwrap_or([false; STATUS_COUNT]);
        let mut values = [0.0; STATUS_COUNT];
        for bit in 0..STATUS_COUNT {
            values[bit] = if status[bit] { 1.0 } else { 0.0 };
        }
        self.snapshot.set_status_array(values);
    }

    pub(super) fn commit_activity(&mut self, dt: Duration, dynamic: bool) {
        if !dynamic {
            self.freeze_live();
            return;
        }
        if self.total_weight == 0 {
            return;
        }

        let total = self.total_weight as f32;
        let mut target = PanelLampSnapshot::default();
        for bit in 0..16 {
            target.address[bit] = self.address_on[bit] as f32 / total;
        }
        for bit in 0..8 {
            target.data[bit] = self.data_on[bit] as f32 / total;
        }
        let mut target_status = [0.0; STATUS_COUNT];
        for bit in 0..STATUS_COUNT {
            target_status[bit] = self.status_on[bit] as f32 / total;
        }
        target.set_status_array(target_status);

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
        let old_status = self.snapshot.status_array();
        let new_status = target.status_array();
        let mut mixed_status = [0.0; STATUS_COUNT];
        for bit in 0..STATUS_COUNT {
            mixed_status[bit] = old_status[bit] * retention + new_status[bit] * inject;
        }
        self.snapshot.set_status_array(mixed_status);
        self.clear_activity();
    }

    pub(super) fn snapshot(&self) -> PanelLampSnapshot {
        self.snapshot
    }

    pub(super) fn live_address(&self) -> u16 {
        self.live_address
    }

    pub(super) fn live_data(&self) -> u8 {
        self.live_data
    }

    fn clear_activity(&mut self) {
        self.address_on.fill(0);
        self.data_on.fill(0);
        self.status_on.fill(0);
        self.total_weight = 0;
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
    fn intel_status_words_map_to_panel_labels() {
        assert_eq!(PanelCycle::InstructionFetch.status_word(), 0xA2);
        assert_eq!(PanelCycle::MemoryRead.status_word(), 0x82);
        assert_eq!(PanelCycle::MemoryWrite.status_word(), 0x00);
        assert_eq!(PanelCycle::StackRead.status_word(), 0x86);
        assert_eq!(PanelCycle::StackWrite.status_word(), 0x04);
        assert_eq!(PanelCycle::InputRead.status_word(), 0x42);
        assert_eq!(PanelCycle::OutputWrite.status_word(), 0x10);
        assert_eq!(PanelCycle::InterruptAcknowledge.status_word(), 0x23);
        assert_eq!(PanelCycle::HaltAcknowledge.status_word(), 0x8A);
        assert_eq!(PanelCycle::InterruptAcknowledgeWhileHalted.status_word(), 0x2B);

        let write = PanelCycle::MemoryWrite.display_status();
        assert!(write[STATUS_WO]);
        assert!(!write[STATUS_MEMR]);
        let read = PanelCycle::MemoryRead.display_status();
        assert!(read[STATUS_MEMR]);
        assert!(!read[STATUS_WO]);
    }

    #[test]
    fn repeated_high_address_reads_dominate_killbit_style_activity() {
        let mut monitor = PanelBusMonitor::default();
        monitor.observe(0x0000, 0x1a, PanelCycle::InstructionFetch);
        for _ in 0..4 {
            monitor.observe(0x8000, 0x00, PanelCycle::MemoryRead);
        }
        monitor.commit_activity(Duration::from_secs(1), true);
        let frame = monitor.snapshot();
        assert!(frame.address[15] > 0.70);
        assert!(frame.memr > 0.99);
    }

    #[test]
    fn status_word_contributes_to_data_bus_visibility() {
        let mut monitor = PanelBusMonitor::default();
        monitor.observe(0x0000, 0x00, PanelCycle::InstructionFetch);
        monitor.commit_activity(Duration::from_secs(1), true);
        let frame = monitor.snapshot();
        // Fetch status word A2h occupies T1, so D7/D5/D1 are visible even when
        // the fetched opcode itself is 00h.
        assert!(frame.data[7] > 0.20);
        assert!(frame.data[5] > 0.20);
        assert!(frame.data[1] > 0.20);
    }
}
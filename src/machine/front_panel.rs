/// Guest-visible front-panel state attached to I/O port 0xFF.
///
/// The CPU sees the high byte of the sense switches on input and drives the
/// eight data lamps on output. UI code must go through `AltairMachine` rather
/// than mutating this state directly.
#[derive(Default)]
pub(super) struct FrontPanelPort {
    switches: u16,
    data_leds: u8,
}

impl FrontPanelPort {
    pub(super) fn input(&self) -> u8 {
        (self.switches >> 8) as u8
    }

    pub(super) fn output(&mut self, value: u8) {
        self.data_leds = value;
    }

    pub(super) fn switches(&self) -> u16 {
        self.switches
    }

    pub(super) fn set_switches(&mut self, value: u16) {
        self.switches = value;
    }

    pub(super) fn toggle_switch(&mut self, bit: usize) {
        if bit < 16 {
            self.switches ^= 1u16 << bit;
        }
    }

    pub(super) fn data_leds(&self) -> u8 {
        self.data_leds
    }

    pub(super) fn set_data_leds(&mut self, value: u8) {
        self.data_leds = value;
    }
}

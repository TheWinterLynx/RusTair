/// Guest-visible sense-switch state attached to I/O port FFh.
///
/// On the real Altair the front-panel DATA lamps are wired to the data bus;
/// they are not a software-addressable output register. `OUT FFh` therefore
/// remains an ordinary I/O bus cycle but does not latch a display value here.
#[derive(Default)]
pub(super) struct FrontPanelPort {
    switches: u16,
}

impl FrontPanelPort {
    pub(super) fn input(&self) -> u8 {
        (self.switches >> 8) as u8
    }

    pub(super) fn switches(&self) -> u16 {
        self.switches
    }

    pub(super) fn toggle_switch(&mut self, bit: usize) {
        if bit < 16 {
            self.switches ^= 1u16 << bit;
        }
    }
}

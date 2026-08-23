/// Emulated Altair 8800 front-panel control hardware.
///
/// The 16 sense/address switches are physical panel inputs. The controller also
/// keeps the address selected by RESET/EXAMINE/EXAMINE NEXT/DEPOSIT NEXT; this
/// is a hardware control latch, not a display register.
#[derive(Default)]
pub(super) struct FrontPanelController {
    switches: u16,
    address_latch: u16,
}

impl FrontPanelController {
    /// IN FFh reads the upper eight sense switches on the original Altair.
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

    pub(super) fn reset_address(&mut self) -> u16 {
        self.address_latch = 0;
        self.address_latch
    }

    pub(super) fn examine_address(&mut self) -> u16 {
        self.address_latch = self.switches;
        self.address_latch
    }

    pub(super) fn examine_next_address(&mut self) -> u16 {
        self.address_latch = self.address_latch.wrapping_add(1);
        self.address_latch
    }

    pub(super) fn deposit_address(&self) -> u16 {
        self.address_latch
    }

    pub(super) fn deposit_next_address(&mut self) -> u16 {
        self.address_latch = self.address_latch.wrapping_add(1);
        self.address_latch
    }

    pub(super) fn set_address_latch(&mut self, address: u16) {
        self.address_latch = address;
    }

    pub(super) fn address_latch(&self) -> u16 {
        self.address_latch
    }
}

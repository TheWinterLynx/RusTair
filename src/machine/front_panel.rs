/// Emulated Altair 8800 front-panel control hardware.
///
/// The 16 sense/address switches are physical panel inputs. The address latch is
/// only a mirror used by reset/presentation helpers; EXAMINE/EXAMINE NEXT and
/// DEPOSIT NEXT no longer derive their sequencing from it. Those operations are
/// driven through the CPU-board/S-100 path instead.
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

    pub(super) fn set_address_latch(&mut self, address: u16) {
        self.address_latch = address;
    }
}

// Keep the instruction-level Fast machine on the same physical RESET-release
// path as the CPU-independent chassis. The bus-suffixed helper is the single
// implementation; this façade only preserves the AltairMachine call boundary.
impl super::AltairBus {
    pub(super) fn release_front_panel_reset(&mut self, address: u16, run: bool) {
        self.release_front_panel_reset_bus(address, run);
    }
}

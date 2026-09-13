//! MITS 88-DCDD Board #1 read-data latch and NRDA electronics.
//!
//! The FD-400 supplies time-stamped physical byte events. This state belongs to
//! controller Board #1: a new byte overwrites the previous latch whether or not
//! the 8080 consumed it, NRDA stays asserted until IN 0Ah, and reading data does
//! not stop the physical byte stream.

#[derive(Debug)]
pub(super) struct Board1ReadElectronics {
    data_latch: u8,
    generation: Option<u64>,
    nrda: bool,
}

impl Board1ReadElectronics {
    pub(super) const fn new(power_up_latch: u8) -> Self {
        Self {
            data_latch: power_up_latch,
            generation: None,
            nrda: false,
        }
    }

    /// Catch up to the newest physical byte event. Re-observing the same event
    /// must not reassert NRDA after the CPU has already consumed that byte.
    pub(super) fn observe_event(&mut self, event: Option<(u64, u8)>) {
        let Some((generation, byte)) = event else {
            return;
        };
        if self.generation == Some(generation) {
            return;
        }
        self.generation = Some(generation);
        self.data_latch = byte;
        self.nrda = true;
    }

    pub(super) const fn nrda(&self) -> bool {
        self.nrda
    }

    pub(super) const fn data_latch(&self) -> u8 {
        self.data_latch
    }

    /// IN 0Ah returns the retained latch and resets NRDA. A later physical byte
    /// event can immediately overwrite the latch and assert NRDA again.
    pub(super) fn consume_data(&mut self) -> u8 {
        self.nrda = false;
        self.data_latch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_bytes_are_overwritten_by_the_newest_physical_event() {
        let mut read = Board1ReadElectronics::new(0x5a);
        assert!(!read.nrda());
        assert_eq!(read.data_latch(), 0x5a);

        read.observe_event(Some((10, 0x81)));
        read.observe_event(Some((11, 0x22)));
        read.observe_event(Some((12, 0x33)));
        assert!(read.nrda());
        assert_eq!(read.data_latch(), 0x33);
    }

    #[test]
    fn data_in_clears_nrda_without_rearming_on_the_same_generation() {
        let mut read = Board1ReadElectronics::new(0x00);
        read.observe_event(Some((42, 0xa5)));
        assert!(read.nrda());
        assert_eq!(read.consume_data(), 0xa5);
        assert!(!read.nrda());

        read.observe_event(Some((42, 0xa5)));
        assert!(!read.nrda(), "the same physical byte cannot arrive twice");

        read.observe_event(Some((43, 0x19)));
        assert!(read.nrda());
        assert_eq!(read.data_latch(), 0x19);
    }
}

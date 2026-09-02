use super::{MachineCycle, TState};
use super::super::{Cpu8080Cycle, Cpu8080Inputs, Cpu8080Pins, TickTrace};

impl Cpu8080Cycle {
    /// Execute one complete T-state through the Intel 8080's two non-overlapping
    /// clock phases while exposing the package-pin state after every physical
    /// clock edge. This is the edge-authoritative path intended for the Cycle
    /// backend. The existing `tick()` remains the complete-T-state semantic
    /// authority and is invoked exactly once, at PHI2 rising, where the Intel
    /// part updates address/SYNC/DBIN/data and samples T-state inputs.
    ///
    /// The observer is called four times, in this invariant order:
    /// PHI1 rising, PHI1 falling, PHI2 rising, PHI2 falling. The pin levels
    /// themselves identify the edge without introducing a second timing enum at
    /// the backend boundary.
    pub(crate) fn tick_with_pin_edges(
        &mut self,
        inputs: Cpu8080Inputs,
        mut observe: impl FnMut(Cpu8080Pins),
    ) -> TickTrace {
        self.clock_phi1_rising();
        observe(self.pins);

        self.pins.phi1 = false;
        observe(self.pins);

        self.pins.phi2 = true;
        let mut trace = self.tick(inputs);
        // RESET is asynchronous in the semantic core and rebuilds the pin
        // structure. The physical clock generator does not stop while RESET is
        // held, so restore the clock input level after that reset action.
        self.pins.phi1 = false;
        self.pins.phi2 = true;
        trace.pins.phi1 = false;
        trace.pins.phi2 = true;
        observe(self.pins);

        self.pins.phi2 = false;
        observe(self.pins);

        trace
    }

    /// Apply the outputs whose documented transition is tied to PHI1. Do not
    /// touch address, data, SYNC or DBIN here: those remain at their preceding
    /// levels until the PHI2 rising edge updates them.
    fn clock_phi1_rising(&mut self) {
        self.pins.phi1 = true;
        self.pins.phi2 = false;
        self.pins.inte = self.inte;

        if self.t_state == TState::Thold || self.holding {
            self.pins.address = None;
            self.pins.data_out = None;
            self.pins.wr_n = true;
            self.pins.wait = false;
            self.pins.hlda = true;
            return;
        }

        self.pins.hlda = false;
        self.pins.wait = self.t_state == TState::Tw;

        let output_cycle = matches!(
            self.machine_cycle,
            MachineCycle::MemoryWrite | MachineCycle::StackWrite | MachineCycle::OutputWrite
        );
        // Intel specifies the negative-going /WR edge from the first PHI1
        // following T2. That is PHI1 of the first TW when READY stretches an
        // output cycle, otherwise PHI1 of T3. The signal returns inactive at
        // PHI1 of the state following T3.
        self.pins.wr_n = !(output_cycle && matches!(self.t_state, TState::Tw | TState::T3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_t_state_exposes_four_non_overlapping_clock_edges() {
        let mut cpu = Cpu8080Cycle::new();
        let mut samples = Vec::new();
        let trace = cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |pins| samples.push(pins));

        assert_eq!(samples.len(), 4);
        assert_eq!((samples[0].phi1, samples[0].phi2), (true, false));
        assert_eq!((samples[1].phi1, samples[1].phi2), (false, false));
        assert_eq!((samples[2].phi1, samples[2].phi2), (false, true));
        assert_eq!((samples[3].phi1, samples[3].phi2), (false, false));

        // T1 address/status/SYNC are PHI2-derived, not fabricated at PHI1.
        assert!(!samples[0].sync);
        assert!(samples[2].sync);
        assert_eq!(samples[2].data_out, Some(0xa2));
        assert_eq!(trace.t_state, TState::T1);
        assert_eq!(cpu.t_state(), TState::T2);
    }

    #[test]
    fn write_strobe_falls_on_phi1_of_t3_before_phi2_updates_other_pins() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.machine_cycle = MachineCycle::MemoryWrite;
        cpu.machine_cycle_index = 2;
        cpu.t_state = TState::T3;
        cpu.cycle_address = 0x3456;
        cpu.cycle_data_out = Some(0xa5);
        cpu.pins.address = Some(0x3456);
        cpu.pins.data_out = Some(0xa5);
        cpu.pins.wr_n = true;

        cpu.clock_phi1_rising();

        assert!(cpu.pins.phi1);
        assert!(!cpu.pins.phi2);
        assert!(!cpu.pins.wr_n, "/WR must assert on PHI1 before the T3 PHI2 edge");
        assert_eq!(cpu.pins.address, Some(0x3456));
        assert_eq!(cpu.pins.data_out, Some(0xa5));
    }

    #[test]
    fn ready_low_selects_tw_at_t2_then_wait_asserts_on_next_phi1() {
        let mut cpu = Cpu8080Cycle::new();
        // T1 -> T2.
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |_| {});
        assert_eq!(cpu.t_state(), TState::T2);

        let mut t2 = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs { ready: false, ..Cpu8080Inputs::default() },
            |pins| t2.push(pins),
        );
        assert!(!t2[0].wait, "WAIT cannot be fabricated before the processor enters TW");
        assert_eq!(cpu.t_state(), TState::Tw);

        let mut tw = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs { ready: false, ..Cpu8080Inputs::default() },
            |pins| tw.push(pins),
        );
        assert!(tw[0].phi1);
        assert!(tw[0].wait, "WAIT must assert at PHI1 of the real TW state");
        assert!(tw[2].dbin, "read DBIN must remain asserted through TW");
        assert_eq!(cpu.t_state(), TState::Tw);
    }

    #[test]
    fn hold_dwell_releases_address_and_data_and_asserts_hlda_from_phi1() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.holding = true;
        cpu.t_state = TState::Thold;
        cpu.pins.address = Some(0x1234);
        cpu.pins.data_out = Some(0x56);

        cpu.clock_phi1_rising();

        assert!(cpu.pins.hlda);
        assert_eq!(cpu.pins.address, None);
        assert_eq!(cpu.pins.data_out, None);
        assert!(cpu.pins.wr_n);
    }
}

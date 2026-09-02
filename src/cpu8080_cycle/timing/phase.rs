use super::{ClockEdge, MachineCycle, TState};
use super::super::{Cpu8080Cycle, Cpu8080Inputs, Cpu8080Pins, TickTrace};

impl Cpu8080Cycle {
    /// Execute one complete T-state through the Intel 8080's two non-overlapping
    /// clock phases while exposing package-pin truth after each physical edge.
    ///
    /// `tick()` remains the architectural/T-state transition authority so the
    /// already-certified instruction behavior is not duplicated. This wrapper
    /// supplies the sub-T-state ordering that the old abstraction could not
    /// express: PHI1-owned outputs are updated first, PHI1 falls, PHI2 rises and
    /// performs the certified T-state transition, then PHI2 falls.
    ///
    /// The observer is called exactly four times in `ClockEdge::ALL` order. The
    /// `TickTrace` pin sample is the PHI2-rising observation for the completed
    /// T-state; the core's live `pins()` state after return is the following
    /// dead-time state with both clock inputs LOW.
    pub(crate) fn tick_with_pin_edges(
        &mut self,
        inputs: Cpu8080Inputs,
        mut observe: impl FnMut(ClockEdge, Cpu8080Pins),
    ) -> TickTrace {
        let t_state = self.t_state;

        self.clock_phi1_rising(inputs);
        observe(ClockEdge::Phi1Rising, self.pins);

        self.pins.phi1 = false;
        observe(ClockEdge::Phi1Falling, self.pins);

        // WAIT and /WR are PHI1-owned flip-flop outputs. `tick()` still drives
        // its historical T-state snapshot, so preserve the physical PHI1 result
        // across the PHI2 transition instead of letting that compatibility
        // projection overwrite the edge-authoritative values.
        let wait_after_phi1 = self.pins.wait;
        let wr_n_after_phi1 = self.pins.wr_n;
        let hlda_after_phi1 = self.pins.hlda;

        self.pins.phi2 = true;
        let mut trace = self.tick(inputs);

        // RESET is asynchronous in the semantic core and rebuilds Cpu8080Pins.
        // The physical CPU-board oscillator itself does not stop on RESET, so
        // restore the clock level for the PHI2 sample after that reset action.
        self.pins.phi1 = false;
        self.pins.phi2 = true;

        if !trace.reset {
            self.pins.wait = wait_after_phi1;
            self.pins.wr_n = wr_n_after_phi1;
            // HLDA rises from the PHI1-domain state. Bus high-impedance itself
            // is applied by the PHI2/T-state path when THOLD is driven.
            self.pins.hlda = hlda_after_phi1 || self.pins.hlda;
            // INTE is a PHI2-referenced processor output. A DI/EI/interrupt
            // transition performed by the certified T-state logic becomes
            // externally visible at this edge.
            self.pins.inte = self.inte;
        }

        trace.pins = self.pins;
        trace.pins.phi1 = false;
        trace.pins.phi2 = true;
        observe(ClockEdge::Phi2Rising, trace.pins);

        self.pins.phi2 = false;
        observe(ClockEdge::Phi2Falling, self.pins);

        debug_assert_eq!(trace.t_state, t_state);
        trace
    }

    /// Apply outputs whose documented transition is referenced to PHI1.
    /// Address, data, SYNC and DBIN are deliberately left untouched until PHI2.
    fn clock_phi1_rising(&mut self, inputs: Cpu8080Inputs) {
        self.pins.phi1 = true;
        self.pins.phi2 = false;

        if self.t_state == TState::Thold || self.holding {
            // HLDA is asserted from the PHI1-domain state. Do not release the
            // address/data buses here: Intel specifies high impedance following
            // the PHI2 edge, which the THOLD T-state path performs below.
            self.pins.hlda = true;
            self.pins.wait = false;
            self.pins.wr_n = true;
            return;
        }

        self.pins.hlda = false;

        // Intel's TW flip-flop is evaluated on PHI1. READY low during T2 starts
        // WAIT; while in TW, WAIT remains asserted only while READY stays low.
        // This is intentionally more precise than the legacy complete-T-state
        // projection, where WAIT could only be represented once TW existed.
        self.pins.wait = self.cycle_uses_ready()
            && match self.t_state {
                TState::T2 | TState::Tw => !inputs.ready,
                _ => false,
            };

        let output_cycle = matches!(
            self.machine_cycle,
            MachineCycle::MemoryWrite | MachineCycle::StackWrite | MachineCycle::OutputWrite
        );
        // /WR falls on the first PHI1 following T2: PHI1 of TW when READY has
        // stretched the cycle, otherwise PHI1 of T3. It returns inactive on the
        // PHI1 following T3.
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
        let trace = cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            samples.push((edge, pins));
        });

        assert_eq!(samples.len(), 4);
        assert_eq!(samples.map(|(edge, _)| edge), ClockEdge::ALL);
        assert_eq!((samples[0].1.phi1, samples[0].1.phi2), (true, false));
        assert_eq!((samples[1].1.phi1, samples[1].1.phi2), (false, false));
        assert_eq!((samples[2].1.phi1, samples[2].1.phi2), (false, true));
        assert_eq!((samples[3].1.phi1, samples[3].1.phi2), (false, false));

        // T1 address/status/SYNC are PHI2-derived, not fabricated at PHI1.
        assert!(!samples[0].1.sync);
        assert!(samples[2].1.sync);
        assert_eq!(samples[2].1.data_out, Some(0xa2));
        assert_eq!(trace.t_state, TState::T1);
        assert_eq!((trace.pins.phi1, trace.pins.phi2), (false, true));
        assert_eq!(cpu.t_state(), TState::T2);
        assert_eq!((cpu.pins().phi1, cpu.pins().phi2), (false, false));
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

        let mut samples = Vec::new();
        let trace = cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            samples.push((edge, pins));
        });

        assert!(samples[0].1.phi1);
        assert!(!samples[0].1.phi2);
        assert!(!samples[0].1.wr_n, "/WR must assert on PHI1 before the T3 PHI2 edge");
        assert_eq!(samples[0].1.address, Some(0x3456));
        assert_eq!(samples[0].1.data_out, Some(0xa5));
        assert!(!trace.pins.wr_n);
    }

    #[test]
    fn ready_low_asserts_wait_on_phi1_of_t2_and_tw_holds_or_releases_it() {
        let mut cpu = Cpu8080Cycle::new();
        // T1 -> T2.
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |_, _| {});
        assert_eq!(cpu.t_state(), TState::T2);

        let mut t2 = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs { ready: false, ..Cpu8080Inputs::default() },
            |edge, pins| t2.push((edge, pins)),
        );
        assert!(t2[0].1.wait, "READY low must set the WAIT flip-flop on PHI1 of T2");
        assert!(t2[2].1.wait, "WAIT remains asserted through the T2 PHI2 edge");
        assert_eq!(cpu.t_state(), TState::Tw);

        let mut tw_low = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs { ready: false, ..Cpu8080Inputs::default() },
            |edge, pins| tw_low.push((edge, pins)),
        );
        assert!(tw_low[0].1.wait);
        assert!(tw_low[2].1.wait);
        assert!(tw_low[2].1.dbin, "read DBIN must remain asserted through TW");
        assert_eq!(cpu.t_state(), TState::Tw);

        let mut tw_release = Vec::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            tw_release.push((edge, pins));
        });
        assert!(!tw_release[0].1.wait, "READY high releases WAIT on PHI1 of TW");
        assert!(!tw_release[2].1.wait);
        assert_eq!(cpu.t_state(), TState::T3);
    }

    #[test]
    fn hold_dwell_asserts_hlda_on_phi1_and_floats_bus_at_phi2() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.holding = true;
        cpu.t_state = TState::Thold;
        cpu.pins.address = Some(0x1234);
        cpu.pins.data_out = Some(0x56);

        let mut samples = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs { hold: true, ..Cpu8080Inputs::default() },
            |edge, pins| samples.push((edge, pins)),
        );

        assert!(samples[0].1.hlda);
        assert_eq!(samples[0].1.address, Some(0x1234), "bus must not float before PHI2");
        assert_eq!(samples[0].1.data_out, Some(0x56));
        assert!(samples[2].1.hlda);
        assert_eq!(samples[2].1.address, None, "address bus floats after PHI2 in HOLD");
        assert_eq!(samples[2].1.data_out, None, "data bus floats after PHI2 in HOLD");
        assert!(samples[2].1.wr_n);
    }
}

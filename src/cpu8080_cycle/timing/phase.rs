use super::{ClockEdge, MachineCycle, TState};
use super::super::{decode::Instruction, Cpu8080Cycle, Cpu8080Inputs, Cpu8080Pins, TickTrace};

impl Cpu8080Cycle {
    /// Execute one complete T-state through the Intel 8080's two non-overlapping
    /// clock phases while exposing package-pin truth after each physical edge.
    /// Callers without an external synchronizer may present one stable input
    /// sample for both phases.
    pub(crate) fn tick_with_pin_edges(
        &mut self,
        inputs: Cpu8080Inputs,
        observe: impl FnMut(ClockEdge, Cpu8080Pins),
    ) -> TickTrace {
        self.tick_with_pin_edges_split(inputs, inputs, observe)
    }

    /// Edge path used by a real CPU-board adapter. `phi1_inputs` are the package
    /// inputs already stable when PHI1 rises; `phi2_inputs` are the values made
    /// valid for the processor at the PHI2 sampling edge. Keeping them separate
    /// is required for the Altair CPU board, which synchronizes S-100 PRDY and
    /// PHOLD at PHI2 instead of wiring those backplane lines straight to the die.
    ///
    /// The observer is called exactly four times in `ClockEdge::ALL` order. The
    /// returned `TickTrace` is the PHI2-rising observation for the completed
    /// T-state; live `pins()` after return are in dead time with both clocks LOW.
    pub(crate) fn tick_with_pin_edges_split(
        &mut self,
        phi1_inputs: Cpu8080Inputs,
        phi2_inputs: Cpu8080Inputs,
        mut observe: impl FnMut(ClockEdge, Cpu8080Pins),
    ) -> TickTrace {
        let t_state = self.t_state;

        self.clock_phi1_rising(phi1_inputs);
        observe(ClockEdge::Phi1Rising, self.pins);

        self.pins.phi1 = false;
        observe(ClockEdge::Phi1Falling, self.pins);

        // WAIT, /WR and HLDA are PHI1-owned outputs. `tick()` still drives its
        // complete-T-state compatibility snapshot, so preserve the physical
        // PHI1 results across PHI2 instead of moving those outputs later.
        let wait_after_phi1 = self.pins.wait;
        let wr_n_after_phi1 = self.pins.wr_n;
        let hlda_after_phi1 = self.pins.hlda;

        self.pins.phi2 = true;
        let mut trace = self.tick(phi2_inputs);

        // RESET rebuilds Cpu8080Pins in the semantic core. The board oscillator
        // does not stop on RESET, so restore the active phase for this sample.
        self.pins.phi1 = false;
        self.pins.phi2 = true;

        if !trace.reset {
            self.pins.wait = wait_after_phi1;
            self.pins.wr_n = wr_n_after_phi1;
            self.pins.hlda = hlda_after_phi1;
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
            // The package HOLD value presented here has already passed any
            // external CPU-board synchronization. HLDA therefore follows that
            // stable input in the PHI1 domain; bus drive changes at PHI2.
            self.pins.hlda = inputs.hold;
            self.pins.wait = false;
            self.pins.wr_n = true;
            return;
        }

        self.pins.hlda = false;

        // Intel specifies WAIT assertion on entry to TW, referenced to PHI1.
        // READY is sampled during T2/TW; it does not make WAIT rise during PHI1
        // of T2 itself. Once the semantic PHI2 transition has entered TW, the
        // following PHI1 exposes WAIT. It falls on the PHI1 after READY lets the
        // processor leave TW.
        self.pins.wait = self.cycle_uses_ready() && self.t_state == TState::Tw && !inputs.ready;

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
        let edges: Vec<_> = samples.iter().map(|(edge, _)| *edge).collect();
        assert_eq!(edges.as_slice(), &ClockEdge::ALL);
        assert_eq!((samples[0].1.phi1, samples[0].1.phi2), (true, false));
        assert_eq!((samples[1].1.phi1, samples[1].1.phi2), (false, false));
        assert_eq!((samples[2].1.phi1, samples[2].1.phi2), (false, true));
        assert_eq!((samples[3].1.phi1, samples[3].1.phi2), (false, false));

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
        cpu.instruction = Instruction::MviMemory;
        cpu.machine_cycle = MachineCycle::MemoryWrite;
        cpu.machine_cycle_index = 3;
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
    fn ready_low_enters_tw_at_t2_phi2_then_wait_rises_on_tw_phi1() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |_, _| {});
        assert_eq!(cpu.t_state(), TState::T2);

        let mut t2 = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs { ready: false, ..Cpu8080Inputs::default() },
            |edge, pins| t2.push((edge, pins)),
        );
        assert!(!t2[0].1.wait, "WAIT must not rise until the processor actually enters TW");
        assert!(!t2[2].1.wait);
        assert_eq!(cpu.t_state(), TState::Tw);

        let mut tw_low = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs { ready: false, ..Cpu8080Inputs::default() },
            |edge, pins| tw_low.push((edge, pins)),
        );
        assert!(tw_low[0].1.wait, "WAIT rises on PHI1 of the actual TW state");
        assert!(tw_low[2].1.wait);
        assert!(tw_low[2].1.dbin, "read DBIN must remain asserted through TW");
        assert_eq!(cpu.t_state(), TState::Tw);

        let mut tw_release = Vec::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            tw_release.push((edge, pins));
        });
        assert!(!tw_release[0].1.wait, "stable package READY high releases WAIT on PHI1 of TW");
        assert!(!tw_release[2].1.wait);
        assert_eq!(cpu.t_state(), TState::T3);
    }

    #[test]
    fn split_inputs_can_change_at_phi2_without_retroactively_changing_phi1() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |_, _| {});
        assert_eq!(cpu.t_state(), TState::T2);

        let old = Cpu8080Inputs { ready: true, ..Cpu8080Inputs::default() };
        let sampled = Cpu8080Inputs { ready: false, ..Cpu8080Inputs::default() };
        let mut samples = Vec::new();
        cpu.tick_with_pin_edges_split(old, sampled, |edge, pins| samples.push((edge, pins)));

        assert!(!samples[0].1.wait);
        assert_eq!(cpu.t_state(), TState::Tw, "READY sampled low at PHI2 must select TW");
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

    #[test]
    fn hold_release_drops_hlda_on_phi1_and_restores_cpu_bus_at_phi2() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.holding = true;
        cpu.t_state = TState::Thold;
        cpu.hold_resume_t_state = TState::T2;
        cpu.machine_cycle = MachineCycle::InstructionFetch;
        cpu.cycle_address = 0x2345;
        cpu.pins.hlda = true;
        cpu.pins.address = None;
        cpu.pins.data_out = None;

        let mut samples = Vec::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            samples.push((edge, pins));
        });

        assert!(!samples[0].1.hlda, "package HOLD low must release HLDA at PHI1");
        assert!(!samples[2].1.hlda);
        assert_eq!(samples[2].1.address, Some(0x2345), "CPU regains address bus at PHI2");
        assert!(samples[2].1.dbin, "resumed fetch T2 must restore DBIN");
        assert!(!cpu.is_holding());
        assert_eq!(cpu.t_state(), TState::T3);
    }
}

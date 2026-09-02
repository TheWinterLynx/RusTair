use super::super::{Cpu8080Cycle, Cpu8080Inputs, Cpu8080Pins, TickTrace};
use super::{ClockEdge, MachineCycle, TState};

impl Cpu8080Cycle {
    /// Execute one complete T-state through the Intel 8080's two non-overlapping
    /// clock phases while exposing package-pin truth after each physical edge.
    /// Callers without an external synchronizer may present one stable input
    /// sample for both phases.
    pub(crate) fn tick_with_pin_edges(
        &mut self,
        inputs: Cpu8080Inputs,
        mut observe: impl FnMut(ClockEdge, Cpu8080Pins),
    ) -> TickTrace {
        self.tick_with_live_phi2_inputs(inputs, |edge, pins| {
            observe(edge, pins);
            inputs
        })
    }

    /// Test helper for callers that already know both package-input samples.
    /// `phi1_inputs` are stable when PHI1 rises; `phi2_inputs` are the values
    /// presented at the PHI2 sampling edge. Production uses the live callback
    /// below so board logic may change READY/HOLD between PHI1 and PHI2.
    #[cfg(test)]
    pub(crate) fn tick_with_pin_edges_split(
        &mut self,
        phi1_inputs: Cpu8080Inputs,
        phi2_inputs: Cpu8080Inputs,
        mut observe: impl FnMut(ClockEdge, Cpu8080Pins),
    ) -> TickTrace {
        self.tick_with_live_phi2_inputs(phi1_inputs, |edge, pins| {
            observe(edge, pins);
            phi2_inputs
        })
    }

    /// Production CPU-board path. The callback observes every real clock edge
    /// and returns the external package-input levels that exist after that edge.
    /// The value returned after PHI1 falls is the one sampled at the upcoming
    /// PHI2 edge. This matters on the Altair because SYNC+PHI1 clocks the MITS
    /// 8212 status latch; a selected card may then change PRDY before the same
    /// T-state's PHI2 READY sample (the 88-2SIO input wait is the canonical case).
    ///
    /// The callback is still invoked exactly four times in `ClockEdge::ALL`
    /// order. The returned `TickTrace` is the PHI2-rising observation for the
    /// completed T-state; live `pins()` after return are in dead time with both
    /// clocks LOW.
    pub(crate) fn tick_with_live_phi2_inputs(
        &mut self,
        phi1_inputs: Cpu8080Inputs,
        mut observe_and_sample: impl FnMut(ClockEdge, Cpu8080Pins) -> Cpu8080Inputs,
    ) -> TickTrace {
        let t_state = self.t_state;

        self.clock_phi1_rising(phi1_inputs);
        let _ = observe_and_sample(ClockEdge::Phi1Rising, self.pins);

        self.pins.phi1 = false;
        let phi2_inputs = observe_and_sample(ClockEdge::Phi1Falling, self.pins);

        // WAIT, /WR and HLDA are PHI1-owned outputs. `tick()` still drives its
        // complete-T-state compatibility snapshot, so preserve the physical
        // PHI1 results across PHI2 instead of moving those outputs later.
        let wait_after_phi1 = self.pins.wait;
        let wr_n_after_phi1 = self.pins.wr_n;
        let hlda_after_phi1 = self.pins.hlda;

        self.pins.phi2 = true;
        // READY/HOLD are semantically consumed here, at the processor sampling
        // edge. On the Altair board S-100 PRDY/PHOLD are first synchronized to
        // this same leading PHI2 edge, so a raw request cannot retroactively
        // change a PHI1-owned package output earlier in the T-state.
        let mut trace = self.tick(phi2_inputs);

        // RESET rebuilds Cpu8080Pins in the semantic core. The board oscillator
        // does not stop on RESET, so restore the active phase for this sample.
        self.pins.phi1 = false;
        self.pins.phi2 = true;

        if !trace.reset {
            self.pins.wait = wait_after_phi1;
            self.pins.wr_n = wr_n_after_phi1;
            self.pins.hlda = hlda_after_phi1;
            // HOLD release is a two-edge sequence: PHOLD is sampled LOW at this
            // PHI2, but Intel specifies that HLDA returns LOW on the following
            // leading PHI1. While HLDA is still HIGH the package buses remain
            // released even if the semantic core has restored its suspended
            // machine-cycle state internally.
            if hlda_after_phi1 {
                self.pins.address = None;
                self.pins.data_out = None;
                self.pins.sync = false;
                self.pins.dbin = false;
                self.pins.wr_n = true;
            }
            self.pins.inte = self.inte;
        }

        trace.pins = self.pins;
        trace.pins.phi1 = false;
        trace.pins.phi2 = true;
        let _ = observe_and_sample(ClockEdge::Phi2Rising, trace.pins);

        self.pins.phi2 = false;
        let _ = observe_and_sample(ClockEdge::Phi2Falling, self.pins);

        // HOLD/HALT dwell can legitimately exit at this PHI2 and report the
        // resumed state rather than the dwell state captured before PHI1.
        // RESET is the other intentional exception: it is asynchronous to the
        // current machine cycle and rebases the semantic core to fetch T1 at the
        // PHI2 where RESET is consumed.
        debug_assert!(
            trace.reset
                || trace.t_state == t_state
                || matches!(t_state, TState::Thold | TState::Thalt)
        );
        trace
    }

    /// Apply outputs whose documented transition is referenced to PHI1.
    /// Address, data, SYNC and DBIN are deliberately left untouched until PHI2.
    fn clock_phi1_rising(&mut self, _inputs: Cpu8080Inputs) {
        self.pins.phi1 = true;
        self.pins.phi2 = false;

        if self.t_state == TState::Thold || self.holding {
            // The internal HOLD latch is cleared only by the following PHI2.
            // Therefore an asynchronous/raw PHOLD release cannot lower HLDA on
            // this PHI1; HLDA stays HIGH until the next PHI1 after that clear.
            self.pins.hlda = true;
            self.pins.wait = false;
            self.pins.wr_n = true;
            return;
        }

        self.pins.hlda = false;

        // Intel specifies WAIT assertion on entry to TW, referenced to PHI1.
        // READY is sampled at PHI2 of T2/TW. Once that PHI2 has selected TW, the
        // following PHI1 raises WAIT. If READY releases TW at a later PHI2, WAIT
        // remains HIGH through that edge and falls on the following PHI1.
        self.pins.wait = self.cycle_uses_ready() && self.t_state == TState::Tw;

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
    use super::super::super::decode::Instruction;
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
    fn reset_from_mid_cycle_rebases_to_fetch_t1_at_phi2() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |_, _| {});
        assert_eq!(cpu.t_state(), TState::T2);

        let mut samples = Vec::new();
        let trace = cpu.tick_with_pin_edges(
            Cpu8080Inputs {
                reset: true,
                ..Cpu8080Inputs::default()
            },
            |edge, pins| samples.push((edge, pins)),
        );

        assert_eq!(samples.len(), 4);
        assert!(trace.reset);
        assert_eq!(trace.t_state, TState::T1);
        assert_eq!(cpu.t_state(), TState::T1);
        assert_eq!((trace.pins.phi1, trace.pins.phi2), (false, true));
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
        assert!(
            !samples[0].1.wr_n,
            "/WR must assert on PHI1 before the T3 PHI2 edge"
        );
        assert_eq!(samples[0].1.address, Some(0x3456));
        assert_eq!(samples[0].1.data_out, Some(0xa5));
        assert!(!trace.pins.wr_n);
    }

    #[test]
    fn ready_low_enters_tw_at_t2_phi2_then_wait_tracks_tw_until_next_phi1() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |_, _| {});
        assert_eq!(cpu.t_state(), TState::T2);

        let mut t2 = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs {
                ready: false,
                ..Cpu8080Inputs::default()
            },
            |edge, pins| t2.push((edge, pins)),
        );
        assert!(
            !t2[0].1.wait,
            "WAIT must not rise until the processor actually enters TW"
        );
        assert!(!t2[2].1.wait);
        assert_eq!(cpu.t_state(), TState::Tw);

        let mut tw_low = Vec::new();
        cpu.tick_with_pin_edges(
            Cpu8080Inputs {
                ready: false,
                ..Cpu8080Inputs::default()
            },
            |edge, pins| tw_low.push((edge, pins)),
        );
        assert!(
            tw_low[0].1.wait,
            "WAIT rises on PHI1 of the actual TW state"
        );
        assert!(tw_low[2].1.wait);
        assert!(
            tw_low[2].1.dbin,
            "read DBIN must remain asserted through TW"
        );
        assert_eq!(cpu.t_state(), TState::Tw);

        let mut tw_release = Vec::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            tw_release.push((edge, pins));
        });
        assert!(
            tw_release[0].1.wait,
            "WAIT stays HIGH until READY is sampled at PHI2"
        );
        assert!(
            tw_release[2].1.wait,
            "WAIT stays HIGH through the PHI2 that exits TW"
        );
        assert_eq!(cpu.t_state(), TState::T3);

        let mut t3 = Vec::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            t3.push((edge, pins));
        });
        assert!(
            !t3[0].1.wait,
            "WAIT falls on the leading PHI1 after TW exit"
        );
    }

    #[test]
    fn split_inputs_can_change_at_phi2_without_retroactively_changing_phi1() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |_, _| {});
        assert_eq!(cpu.t_state(), TState::T2);

        let old = Cpu8080Inputs {
            ready: true,
            ..Cpu8080Inputs::default()
        };
        let sampled = Cpu8080Inputs {
            ready: false,
            ..Cpu8080Inputs::default()
        };
        let mut samples = Vec::new();
        cpu.tick_with_pin_edges_split(old, sampled, |edge, pins| samples.push((edge, pins)));

        assert!(!samples[0].1.wait);
        assert_eq!(
            cpu.t_state(),
            TState::Tw,
            "READY sampled low at PHI2 must select TW"
        );
    }

    #[test]
    fn live_phi2_inputs_are_sampled_after_phi1_observation() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |_, _| {});
        assert_eq!(cpu.t_state(), TState::T2);

        let mut callback_count = 0usize;
        cpu.tick_with_live_phi2_inputs(Cpu8080Inputs::default(), |_edge, _pins| {
            callback_count += 1;
            if callback_count == 2 {
                Cpu8080Inputs {
                    ready: false,
                    ..Cpu8080Inputs::default()
                }
            } else {
                Cpu8080Inputs::default()
            }
        });

        assert_eq!(callback_count, 4);
        assert_eq!(
            cpu.t_state(),
            TState::Tw,
            "external PRDY settled after PHI1 must be sampled at PHI2"
        );
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
            Cpu8080Inputs {
                hold: true,
                ..Cpu8080Inputs::default()
            },
            |edge, pins| samples.push((edge, pins)),
        );

        assert!(samples[0].1.hlda);
        assert_eq!(
            samples[0].1.address,
            Some(0x1234),
            "bus must not float before PHI2"
        );
        assert_eq!(samples[0].1.data_out, Some(0x56));
        assert!(samples[2].1.hlda);
        assert_eq!(
            samples[2].1.address, None,
            "address bus floats after PHI2 in HOLD"
        );
        assert_eq!(
            samples[2].1.data_out, None,
            "data bus floats after PHI2 in HOLD"
        );
        assert!(samples[2].1.wr_n);
    }

    #[test]
    fn hold_release_clears_latch_at_phi2_then_drops_hlda_at_next_phi1() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.holding = true;
        cpu.t_state = TState::Thold;
        cpu.hold_resume_t_state = TState::T2;
        cpu.machine_cycle = MachineCycle::InstructionFetch;
        cpu.cycle_address = 0x2345;
        cpu.pins.hlda = true;
        cpu.pins.address = None;
        cpu.pins.data_out = None;

        let mut release = Vec::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            release.push((edge, pins));
        });

        assert!(
            release[0].1.hlda,
            "raw PHOLD release cannot lower HLDA before its PHI2 sample"
        );
        assert!(
            release[2].1.hlda,
            "HLDA stays HIGH through the PHI2 that clears HOLD"
        );
        assert_eq!(
            release[2].1.address, None,
            "bus remains released while HLDA is HIGH"
        );
        assert_eq!(release[2].1.data_out, None);
        assert!(!cpu.is_holding());
        assert_eq!(cpu.t_state(), TState::T3);

        let mut resumed = Vec::new();
        cpu.tick_with_pin_edges(Cpu8080Inputs::default(), |edge, pins| {
            resumed.push((edge, pins));
        });
        assert!(
            !resumed[0].1.hlda,
            "HLDA falls on the leading PHI1 after release PHI2"
        );
        assert_eq!(
            resumed[0].1.address, None,
            "CPU does not regain the bus before the following PHI2"
        );
        assert!(!resumed[2].1.hlda);
        assert_eq!(
            resumed[2].1.address,
            Some(0x2345),
            "CPU regains address bus at the following PHI2"
        );
    }
}

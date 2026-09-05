use crate::cpu8080_cycle::{Cpu8080Pins, TState, TickTrace};

/// Common electrical contract between the exact CPU-board adapter and the S-100 bus.
///
/// The original Altair CPU board turns the 8080's one bidirectional D0-D7 bus
/// into two independent S-100 directions. Keeping all three domains here stops
/// the front panel, debugger and CPU package view from silently treating one
/// byte as three different electrical nets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct S100CpuSample {
    pub address: Option<u16>,
    pub cpu_data: Option<u8>,
    pub data_in: Option<u8>,
    pub data_out: Option<u8>,
    /// Compatibility latch update used only where an exact edge is not available.
    /// Ordinary Partial execution latches status at the real SYNC+PHI1 edge.
    pub status_word: Option<u8>,
    pub inte: bool,
    pub ready: bool,
    pub wait: bool,
    pub hlda: bool,
}

/// S-100/front-panel control lines presented to a CPU board. These are inputs
/// to the processor board and therefore remain distinct from CPU outputs such
/// as WAIT, HLDA, INTE and SINTA/status. `interrupt` is canonical PINT (pin 73).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct S100CpuControlLines {
    pub ready: bool,
    pub interrupt: bool,
    pub hold: bool,
    pub reset: bool,
}

impl super::AltairBus {
    /// Deposit is the one original front-panel operation that really does drive
    /// the write data/pulse itself while the stopped CPU continues to provide the
    /// address. Expose that physical action to the exact backend CPU-board path.
    pub(crate) fn cpu_board_front_panel_deposit(&mut self, address: u16, value: u8) {
        self.panel.set_address_latch(address);
        self.front_panel_deposit(address, value);
    }

    /// Adaptive Cycle mutates the external READY input independently of WAIT.
    /// WAIT is an 8080 output and is updated only by exact CPU-board samples.
    pub(crate) fn cycle_set_ready_input(&mut self, ready: bool) {
        self.s100.set_ready_input(ready);
    }

    /// Display/Control-board PRDY contribution before RAM/device wait sources
    /// are wired into the effective S-100 READY level.
    pub(crate) fn cycle_front_panel_ready_input(&self) -> bool {
        self.s100.signals().front_panel_ready
    }

    /// Change only the external HOLD request seen by the cycle-accurate CPU.
    /// HLDA is an 8080 output and must remain whatever the last exact CPU sample
    /// drove until a later `Cpu8080Cycle` edge changes it.
    pub(crate) fn cycle_set_hold_request(&mut self, hold: bool) {
        let cpu_hlda = self.s100.signals().hlda;
        self.s100.set_hold(hold);
        self.s100.set_hlda(cpu_hlda);
    }

    /// Project one exact Intel 8080 clock edge through the original MITS CPU
    /// board onto the canonical S-100 backplane command/clock nets.
    ///
    /// The board's 8212 status latch is physically clocked when processor SYNC
    /// is still high at a PHI1 rising edge. This deliberately separates the
    /// T1 status byte on CPU D/DO from the later dedicated S-100 status outputs.
    /// The original 88-2SIO also makes its one input wait edge-owned: SINP clocks
    /// its V flip-flop at this T2 PHI1 and pulls PRDY low; the processor's PWAIT
    /// output clears V at TW PHI1 and releases PRDY again. Keep those electrical
    /// transitions here rather than moving them to a host-side T-state boundary.
    pub(crate) fn drive_cycle_cpu_board_edge<E>(&mut self, _edge: E, pins: Cpu8080Pins) {
        self.s100.drive_cpu_board_edge(
            pins.phi1,
            pins.phi2,
            pins.sync,
            pins.dbin,
            pins.wr_n,
        );
        if pins.phi1 && pins.sync {
            if let Some(word) = pins.data_out {
                self.s100.latch_cpu_status(word);
            }
        }
        if !self.cycle_uses_physical_serial() && pins.phi1 && pins.sync {
            let signals = self.s100.signals();
            let port = signals.address as u8;
            if signals.inp && self.io.input_wait_states(port) != 0 {
                self.s100.set_memory_ready_input(false);
            }
        }
        if !self.cycle_uses_physical_serial() && pins.phi1 && pins.wait {
            let signals = self.s100.signals();
            let port = signals.address as u8;
            if signals.inp && self.io.input_wait_states(port) != 0 {
                self.s100.set_memory_ready_input(true);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn raw_s100_phi1(&self) -> Option<bool> { self.s100.signals().phi1 }
    #[cfg(test)]
    pub(crate) fn raw_s100_phi2(&self) -> Option<bool> { self.s100.signals().phi2 }
    #[cfg(test)]
    pub(crate) fn raw_s100_psync(&self) -> bool { self.s100.signals().psync }
    #[cfg(test)]
    pub(crate) fn raw_s100_pdbin(&self) -> bool { self.s100.signals().pdbin }
    #[cfg(test)]
    pub(crate) fn raw_s100_pwr_n(&self) -> bool { self.s100.signals().pwr_n }
}

/// Adapter for the edge/T-state Intel 8080 core.
///
/// Timing and CPU output-control signals come directly from `TickTrace`. Read
/// data originates on RAM/I/O boards and therefore appears on S-100 DI before
/// reaching the package D bus. Front-panel EXAMINE injection is different: the
/// D/C board strobes the processor D bus directly and bypasses S-100 DI.
pub(crate) struct Cycle8080S100Adapter;

impl Cycle8080S100Adapter {
    pub(crate) fn sample_with_front_panel_direct(
        trace: &TickTrace,
        visible_data: Option<u8>,
        front_panel_direct: bool,
        ready: bool,
    ) -> S100CpuSample {
        // Exact normal status is NOT latched here. The processor presents the
        // byte on D/DO while SYNC is active in T1; the MITS board's 8212 captures
        // it only at the following PHI1 edge. HALT dwell is the one compatibility
        // restoration path retained after bus grant/release because no new SYNC
        // occurs while the processor remains halted.
        let status_word = if !trace.pins.hlda && trace.t_state == TState::Thalt {
            trace.machine_cycle.status_word()
        } else {
            None
        };

        let (cpu_data, data_in, data_out) = if trace.pins.hlda {
            (None, None, None)
        } else if let Some(value) = trace.pins.data_out {
            (Some(value), None, Some(value))
        } else if let Some(value) = visible_data {
            if front_panel_direct {
                (Some(value), None, Some(value))
            } else {
                (Some(value), Some(value), None)
            }
        } else {
            (None, None, None)
        };

        S100CpuSample {
            address: trace.pins.address,
            cpu_data,
            data_in,
            data_out,
            status_word,
            inte: trace.pins.inte,
            ready,
            wait: trace.pins.wait,
            hlda: trace.pins.hlda,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu8080_cycle::{Cpu8080Pins, MachineCycle};

    #[test]
    fn cycle_adapter_keeps_t1_status_on_cpu_d_do_until_real_phi1_latch() {
        let trace = TickTrace {
            machine_cycle: MachineCycle::MemoryRead,
            machine_cycle_index: 2,
            t_state: TState::T1,
            pins: Cpu8080Pins {
                phi2: true,
                address: Some(0x2000),
                data_out: Some(0x82),
                sync: true,
                inte: true,
                ..Cpu8080Pins::default()
            },
            opcode: Some(0x3A),
            instruction_complete: false,
            reset: false,
            fault: None,
            total_t_states: 5,
            instruction_t_states: 5,
        };

        let sample = Cycle8080S100Adapter::sample_with_front_panel_direct(&trace, None, false, true);
        assert_eq!(sample.address, Some(0x2000));
        assert_eq!(sample.cpu_data, Some(0x82));
        assert_eq!(sample.data_in, None);
        assert_eq!(sample.data_out, Some(0x82));
        assert_eq!(sample.status_word, None, "exact status latch belongs to SYNC+PHI1, not T1 projection");
        assert!(sample.inte);
        assert!(sample.ready);
    }

    #[test]
    fn mits_8212_status_latches_only_on_sync_plus_phi1_edge() {
        let mut bus = super::super::AltairBus::default();
        let t1_phi2 = Cpu8080Pins {
            phi1: false,
            phi2: true,
            data_out: Some(0xa2),
            sync: true,
            ..Cpu8080Pins::default()
        };
        bus.drive_cycle_cpu_board_edge((), t1_phi2);
        assert!(!bus.s100.signals().m1);
        assert!(bus.raw_s100_psync());
        assert_eq!(bus.raw_s100_phi2(), Some(true));

        let t2_phi1 = Cpu8080Pins {
            phi1: true,
            phi2: false,
            data_out: Some(0xa2),
            sync: true,
            ..Cpu8080Pins::default()
        };
        bus.drive_cycle_cpu_board_edge((), t2_phi1);
        let latched = bus.s100.signals();
        assert!(latched.memr && latched.m1 && latched.wo);
        assert_eq!(bus.raw_s100_phi1(), Some(true));
    }

    #[test]
    fn cycle_cpu_board_exports_historical_command_nets_directly_from_package_pins() {
        let mut bus = super::super::AltairBus::default();
        let pins = Cpu8080Pins {
            phi1: false,
            phi2: true,
            sync: false,
            dbin: true,
            wr_n: true,
            ..Cpu8080Pins::default()
        };
        bus.drive_cycle_cpu_board_edge((), pins);
        assert_eq!(bus.raw_s100_phi1(), Some(false));
        assert_eq!(bus.raw_s100_phi2(), Some(true));
        assert!(!bus.raw_s100_psync());
        assert!(bus.raw_s100_pdbin());
        assert!(bus.raw_s100_pwr_n());
    }

    #[test]
    fn cycle_adapter_distinguishes_normal_di_from_front_panel_direct_injection() {
        let trace = TickTrace {
            machine_cycle: MachineCycle::MemoryRead,
            machine_cycle_index: 2,
            t_state: TState::T3,
            pins: Cpu8080Pins {
                address: Some(0x2000),
                dbin: true,
                ..Cpu8080Pins::default()
            },
            opcode: Some(0x3A),
            instruction_complete: false,
            reset: false,
            fault: None,
            total_t_states: 7,
            instruction_t_states: 7,
        };
        let memory = Cycle8080S100Adapter::sample_with_front_panel_direct(&trace, Some(0x5a), false, true);
        assert_eq!(memory.cpu_data, Some(0x5a));
        assert_eq!(memory.data_in, Some(0x5a));
        assert_eq!(memory.data_out, None);

        let jam = Cycle8080S100Adapter::sample_with_front_panel_direct(
            &trace,
            Some(0xc3),
            true,
            true,
        );
        assert_eq!(jam.cpu_data, Some(0xc3));
        assert_eq!(jam.data_in, None);
        assert_eq!(jam.data_out, Some(0xc3));
    }

    #[test]
    fn cycle_hold_request_change_does_not_fabricate_hlda_output() {
        let mut bus = super::super::AltairBus::default();
        bus.s100.set_hlda(true);
        bus.cycle_set_hold_request(false);
        assert!(!bus.s100.signals().hold);
        assert!(bus.s100.signals().hlda, "HLDA must remain CPU-owned until the next exact sample");
    }

    #[test]
    fn cycle_run_ready_change_does_not_fabricate_wait_output() {
        let mut chassis = super::super::AltairChassis::default();
        chassis.cycle_power_chassis(true, true, 0, false);
        chassis.bus.s100.drive_cpu_t_state(
            Some(0), Some(0xa2), None, Some(0xa2), Some(0xa2), false, false,
            true, false, false,
        );
        chassis.cycle_set_running(false);
        let stopped_request = chassis.bus.s100.signals();
        assert!(!stopped_request.run);
        assert!(!stopped_request.ready);
        assert!(!stopped_request.wait, "lowering READY is not itself a WAIT acknowledgement");
    }
}

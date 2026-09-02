use crate::cpu8080::{Bus, Cpu8080};
use crate::cpu8080_cycle::{Cpu8080Pins, TState, TickTrace};

/// Machine-cycle classes emitted by the instruction-level 8080 core.
///
/// The fast core cannot expose every physical T-state, so its CPU-board adapter
/// expands one semantic machine cycle into synthetic S-100 samples. The cycle
/// core does not use this enum for timing; it supplies real pin-level samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum S100Cycle {
    InstructionFetch,
    MemoryRead,
    MemoryWrite,
    StackRead,
    StackWrite,
    InputRead,
    OutputWrite,
    InterruptAcknowledge,
    HaltAcknowledge,
    InterruptAcknowledgeWhileHalted,
}

impl S100Cycle {
    pub(super) const fn status_word(self) -> u8 {
        match self {
            Self::InstructionFetch => 0xA2,
            Self::MemoryRead => 0x82,
            Self::MemoryWrite => 0x00,
            Self::StackRead => 0x86,
            Self::StackWrite => 0x04,
            Self::InputRead => 0x42,
            Self::OutputWrite => 0x10,
            Self::InterruptAcknowledge => 0x23,
            Self::HaltAcknowledge => 0x8A,
            Self::InterruptAcknowledgeWhileHalted => 0x2B,
        }
    }

    const fn t_states(self) -> u32 {
        match self {
            Self::InstructionFetch | Self::HaltAcknowledge => 4,
            _ => 3,
        }
    }

    const fn reads_data_from_s100(self) -> bool {
        matches!(
            self,
            Self::InstructionFetch
                | Self::MemoryRead
                | Self::StackRead
                | Self::InputRead
                | Self::InterruptAcknowledge
                | Self::InterruptAcknowledgeWhileHalted
        )
    }

    const fn writes_data_to_s100(self) -> bool {
        matches!(self, Self::MemoryWrite | Self::StackWrite | Self::OutputWrite)
    }
}

/// Common electrical contract between a CPU-board adapter and the S-100 bus.
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
    /// Fast/reconstructed status-latch update. Exact Cycle latches ordinary
    /// status through the physical SYNC+PHI1 edge instead.
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

/// Bytes driven by the original Display/Control board onto the 8080 data bus.
///
/// EXAMINE injects `JMP address` (C3, low, high). EXAMINE NEXT injects NOP.
/// This is how the real Altair makes the CPU itself drive the requested address;
/// the front panel does not directly load the program counter or address bus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrontPanelJamSequence {
    bytes: [u8; 3],
    len: usize,
}

impl FrontPanelJamSequence {
    const fn examine(address: u16) -> Self {
        Self {
            bytes: [0xc3, address as u8, (address >> 8) as u8],
            len: 3,
        }
    }

    const fn examine_next() -> Self {
        Self { bytes: [0x00, 0, 0], len: 1 }
    }

    fn byte(self, index: usize) -> Option<u8> {
        (index < self.len).then_some(self.bytes[index])
    }
}

/// Adapter for the validated instruction-level 8080 core.
///
/// It cannot reconstruct true sub-instruction bus activity, so it synthesizes a
/// conservative T-state-shaped stream from the machine-cycle callback. This is
/// why the fast backend continues to advertise `exact_bus_activity = false`.
pub(super) struct Fast8080S100Adapter;

impl Fast8080S100Adapter {
    fn for_each_sample_impl(
        address: u16,
        data: u8,
        cycle: S100Cycle,
        inte: bool,
        ready: bool,
        wait: bool,
        front_panel_direct: bool,
        mut emit: impl FnMut(S100CpuSample),
    ) {
        let status = cycle.status_word();
        // At the beginning of every machine cycle the 8080 places its status
        // byte on D0-D7. Fast cannot expose the later physical SYNC+PHI1 latch
        // edge, so it deliberately applies the equivalent status immediately.
        emit(S100CpuSample {
            address: Some(address),
            cpu_data: Some(status),
            data_in: None,
            data_out: Some(status),
            status_word: Some(status),
            inte,
            ready,
            wait,
            hlda: false,
        });

        for _ in 1..cycle.t_states() {
            let (cpu_data, data_in, data_out) = if front_panel_direct {
                // EXAMINE injects directly onto the processor D bus; it is not
                // an S-100 DI source. The CPU-board DO buffers see that same D
                // level while the Display/Control board owns the injection.
                (Some(data), None, Some(data))
            } else if cycle.reads_data_from_s100() {
                (Some(data), Some(data), None)
            } else if cycle.writes_data_to_s100() {
                (Some(data), None, Some(data))
            } else {
                (None, None, None)
            };
            emit(S100CpuSample {
                address: Some(address),
                cpu_data,
                data_in,
                data_out,
                status_word: None,
                inte,
                ready,
                wait,
                hlda: false,
            });
        }
    }

    pub(super) fn for_each_sample(
        address: u16,
        data: u8,
        cycle: S100Cycle,
        inte: bool,
        ready: bool,
        wait: bool,
        emit: impl FnMut(S100CpuSample),
    ) {
        Self::for_each_sample_impl(address, data, cycle, inte, ready, wait, false, emit);
    }

    pub(super) fn for_each_front_panel_jam_sample(
        address: u16,
        data: u8,
        cycle: S100Cycle,
        inte: bool,
        ready: bool,
        wait: bool,
        emit: impl FnMut(S100CpuSample),
    ) {
        Self::for_each_sample_impl(address, data, cycle, inte, ready, wait, true, emit);
    }
}

/// Temporary bus overlay used while the physical front panel jams an instruction
/// into the instruction-level 8080. RAM remains untouched; only the bytes seen
/// by the CPU on the selected read cycles are replaced by the panel sequence.
struct FrontPanelJamBus<'a> {
    bus: &'a mut super::AltairBus,
    sequence: FrontPanelJamSequence,
    index: usize,
}

impl<'a> FrontPanelJamBus<'a> {
    fn new(bus: &'a mut super::AltairBus, sequence: FrontPanelJamSequence) -> Self {
        Self { bus, sequence, index: 0 }
    }

    fn jam_or_memory(&mut self, address: u16, cycle: S100Cycle) -> u8 {
        if let Some(value) = self.sequence.byte(self.index) {
            self.index += 1;
            self.bus.drive_front_panel_jam_cycle(address, value, cycle);
            value
        } else {
            let value = self.bus.memory.read(address);
            self.bus.drive_cpu_cycle(address, value, cycle);
            value
        }
    }
}

impl Bus for FrontPanelJamBus<'_> {
    fn read(&mut self, address: u16) -> u8 {
        self.jam_or_memory(address, S100Cycle::MemoryRead)
    }

    fn write(&mut self, address: u16, value: u8) {
        self.bus.write(address, value);
    }

    fn input(&mut self, port: u8) -> u8 { self.bus.input(port) }
    fn output(&mut self, port: u8, value: u8) { self.bus.output(port, value); }
    fn set_inte(&mut self, enabled: bool) { self.bus.set_inte(enabled); }

    fn opcode_fetch(&mut self, address: u16) -> u8 {
        self.jam_or_memory(address, S100Cycle::InstructionFetch)
    }

    fn stack_read(&mut self, address: u16) -> u8 { self.bus.stack_read(address) }
    fn stack_write(&mut self, address: u16, value: u8) { self.bus.stack_write(address, value); }
    fn halt_ack(&mut self, address: u16, opcode: u8) { self.bus.halt_ack(address, opcode); }
    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        self.bus.interrupt_ack(address, opcode, while_halted);
    }

    // Front-panel-injected JMP/NOP activity is hardware control sequencing, not
    // guest code, and must never perturb diagnostic instruction accounting.
    fn instruction_complete(&mut self, _address: u16, _opcode: u8, _t_states: u32) {}
}

impl super::AltairBus {
    /// Deposit is the one original front-panel operation that really does drive
    /// the write data/pulse itself while the stopped CPU continues to provide the
    /// address. Expose that physical action to backend CPU-board adapters.
    pub(crate) fn cpu_board_front_panel_deposit(&mut self, address: u16, value: u8) {
        self.panel.set_address_latch(address);
        self.front_panel_deposit(address, value);
    }

    pub(super) fn drive_front_panel_jam_cycle(
        &mut self,
        address: u16,
        data: u8,
        cycle: S100Cycle,
    ) {
        let signals = self.s100.signals();
        let inte = signals.inte;
        Fast8080S100Adapter::for_each_front_panel_jam_sample(
            address,
            data,
            cycle,
            inte,
            signals.ready,
            signals.wait,
            |sample| self.drive_cpu_board_sample(sample),
        );
    }

    /// Cycle Accurate mutates the external READY input independently of WAIT.
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
    /// drove until a later `Cpu8080Cycle` edge changes it. The generic chassis
    /// helper also drops HLDA on HOLD release for the instruction-level backend;
    /// Cycle must not use that approximation.
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
            let signals = self.s100.signals();
            let port = signals.address as u8;
            if signals.inp && self.io.input_wait_states(port) != 0 {
                self.s100.set_memory_ready_input(false);
            }
        }
        if pins.phi1 && pins.wait {
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

impl super::AltairMachine {
    /// Instruction-level approximation of the original EXAMINE/EXAMINE NEXT
    /// sequencer. The fast CPU executes the real injected JMP/NOP rather than
    /// having its PC assigned by the GUI or backend.
    pub(crate) fn fast_front_panel_examine_via_cpu_board(&mut self, next: bool) {
        if !self.powered
            || self.running
            || self.bus.reset_asserted()
            || self.bus.hold_requested()
            || self.cpu.halted
        {
            return;
        }

        let sequence = if next {
            FrontPanelJamSequence::examine_next()
        } else {
            FrontPanelJamSequence::examine(self.bus.panel_switches())
        };

        self.bus.set_hlda(false);
        self.bus.set_ready(true);
        {
            let cpu: &mut Cpu8080 = &mut self.cpu;
            let bus = &mut self.bus;
            let mut jam_bus = FrontPanelJamBus::new(bus, sequence);
            cpu.step(&mut jam_bus);
        }
        self.bus.sync_cpu_inte(self.cpu.inte);

        // The next fetch is where the physical machine is stopped. Synthesise
        // that waiting-fetch bus state for the instruction-level core.
        self.bus.panel.set_address_latch(self.cpu.pc);
        self.bus.set_ready(false);
        self.bus.release_front_panel_reset_bus(self.cpu.pc, false);
    }

    pub(crate) fn fast_front_panel_deposit_via_cpu_board(&mut self, next: bool) {
        if !self.powered
            || self.running
            || self.bus.reset_asserted()
            || self.bus.hold_requested()
            || self.cpu.halted
        {
            return;
        }

        if next {
            self.fast_front_panel_examine_via_cpu_board(true);
        }
        let address = self.bus.panel_address();
        let value = self.bus.panel_switches() as u8;
        self.bus.cpu_board_front_panel_deposit(address, value);
    }

    /// PROTECT/UNPROTECT belongs to the front panel and the selected memory
    /// board, not to a particular CPU implementation. The addressed board is
    /// therefore derived from the address currently visible on the S-100 bus.
    /// Changing the switch register after EXAMINE must not silently retarget the
    /// protection operation.
    pub(crate) fn front_panel_set_memory_protection_via_s100(&mut self, protected: bool) {
        if !self.powered
            || self.running
            || self.bus.reset_asserted()
            || self.bus.hold_requested()
            || self.cpu.halted
        {
            return;
        }

        let address = self.bus.panel_address();
        self.bus.set_protected(address, protected);
        self.bus.freeze_panel_bus();
    }
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
    fn fast_adapter_routes_fetch_status_to_do_and_memory_data_to_di() {
        let mut samples = Vec::new();
        Fast8080S100Adapter::for_each_sample(
            0x1234,
            0x56,
            S100Cycle::InstructionFetch,
            true,
            true,
            false,
            |sample| samples.push(sample),
        );

        assert_eq!(samples.len(), 4);
        assert_eq!(samples[0].status_word, Some(0xA2));
        assert_eq!(samples[0].cpu_data, Some(0xA2));
        assert_eq!(samples[0].data_in, None);
        assert_eq!(samples[0].data_out, Some(0xA2));
        assert_eq!(samples[1].status_word, None);
        assert_eq!(samples[1].cpu_data, Some(0x56));
        assert_eq!(samples[1].data_in, Some(0x56));
        assert_eq!(samples[1].data_out, None);
        assert!(samples.iter().all(|sample| sample.address == Some(0x1234)));
    }

    #[test]
    fn fast_adapter_routes_memory_write_to_do_not_di() {
        let mut samples = Vec::new();
        Fast8080S100Adapter::for_each_sample(
            0x2000,
            0xa5,
            S100Cycle::MemoryWrite,
            false,
            true,
            false,
            |sample| samples.push(sample),
        );
        assert_eq!(samples[1].cpu_data, Some(0xa5));
        assert_eq!(samples[1].data_in, None);
        assert_eq!(samples[1].data_out, Some(0xa5));
    }

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

    #[test]
    fn fast_front_panel_examine_executes_jammed_jump_without_touching_ram() {
        let mut machine = super::super::AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.bus.load(0, &[0x76, 0xaa, 0xbb]);
        machine.bus.load(0x0123, &[0x5a]);
        for bit in 0..16 {
            if 0x0123 & (1u16 << bit) != 0 {
                machine.toggle_sense_switch(bit);
            }
        }

        let before = machine.cpu.cycles;
        machine.fast_front_panel_examine_via_cpu_board(false);
        assert_eq!(machine.cpu.pc, 0x0123);
        assert_eq!(machine.cpu.cycles - before, 10);
        assert_eq!(machine.bus.peek_memory(0), Some(0x76));
        assert_eq!(machine.bus.peek_memory(1), Some(0xaa));
        assert_eq!(machine.bus.peek_memory(2), Some(0xbb));
        assert_eq!(machine.address_leds(), 0x0123);
        assert_eq!(machine.data_leds(), 0x5a);
        assert!(machine.wait_led());
    }

    #[test]
    fn fast_front_panel_examine_next_jams_nop_and_deposit_next_uses_new_address() {
        let mut machine = super::super::AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.bus.load(0, &[0x11, 0x22]);
        machine.fast_front_panel_examine_via_cpu_board(true);
        assert_eq!(machine.cpu.pc, 1);
        assert_eq!(machine.address_leds(), 1);

        // Low eight switches = A5h.
        for bit in [0, 2, 5, 7] { machine.toggle_sense_switch(bit); }
        machine.fast_front_panel_deposit_via_cpu_board(false);
        assert_eq!(machine.bus.peek_memory(1), Some(0xa5));

        machine.fast_front_panel_deposit_via_cpu_board(true);
        assert_eq!(machine.cpu.pc, 2);
        assert_eq!(machine.bus.peek_memory(2), Some(0xa5));
    }

    #[test]
    fn front_panel_protection_uses_live_s100_address_and_blocks_deposit() {
        let mut machine = super::super::AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.bus.load(0x0456, &[0x11]);

        // Select 0456h and let the CPU itself drive it through a jammed JMP.
        for bit in 0..16 {
            if 0x0456 & (1u16 << bit) != 0 {
                machine.toggle_sense_switch(bit);
            }
        }
        machine.fast_front_panel_examine_via_cpu_board(false);
        assert_eq!(machine.address_leds(), 0x0456);

        // Change the switch register to a different 1 KiB board without doing
        // another EXAMINE. PROTECT must still target the live S-100 address.
        machine.toggle_sense_switch(11); // 0456h -> 0C56h, low data stays 56h.
        machine.front_panel_set_memory_protection_via_s100(true);
        assert!(machine.bus.is_protected(0x0400));
        assert!(machine.bus.is_protected(0x07ff));
        assert!(!machine.bus.is_protected(0x0c00));
        assert!(machine.current_board_protected());

        machine.fast_front_panel_deposit_via_cpu_board(false);
        assert_eq!(machine.bus.peek_memory(0x0456), Some(0x11));

        machine.front_panel_set_memory_protection_via_s100(false);
        assert!(!machine.current_board_protected());
        machine.fast_front_panel_deposit_via_cpu_board(false);
        assert_eq!(machine.bus.peek_memory(0x0456), Some(0x56));
    }
}

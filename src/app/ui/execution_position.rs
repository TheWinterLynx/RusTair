use super::super::RusTairApp;

/// Return a UI-stable address for the guest instruction currently being
/// executed (or the next instruction when stopped at a boundary).
///
/// The fast core only exposes complete-instruction states, so its register PC
/// already has this meaning. Cycle Accurate can be rendered between machine
/// cycles, where the architectural PC may already point at an operand/next byte.
/// While a debugger view is open the shared instruction trace is active; the
/// latest completed entry's `after.pc` is therefore the start address of the
/// instruction currently in flight. HLT is special because its architectural
/// PC advances before the processor enters halt dwell, so retain the HLT entry's
/// own address for didactic/current-instruction displays.
pub(super) fn current_instruction_address(app: &mut RusTairApp) -> u16 {
    let cpu = app.machine.intel8080_state();
    if !app.machine.instruction_trace_enabled() {
        return cpu.pc;
    }

    let history = app.machine.instruction_trace_snapshot();
    let Some(last) = history.last() else {
        return cpu.pc;
    };

    if cpu.halted.unwrap_or(false) && last.after.halted {
        last.address
    } else {
        last.after.pc
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::Intel8080State;
    use crate::trace8080::{CpuSnapshot8080, InstructionTraceEntry};

    fn stable_address(
        cpu: Intel8080State,
        trace_enabled: bool,
        last: Option<&InstructionTraceEntry>,
    ) -> u16 {
        if !trace_enabled {
            return cpu.pc;
        }
        let Some(last) = last else { return cpu.pc; };
        if cpu.halted.unwrap_or(false) && last.after.halted {
            last.address
        } else {
            last.after.pc
        }
    }

    #[test]
    fn cycle_mid_instruction_uses_previous_after_pc_not_live_operand_pc() {
        let cpu = Intel8080State { pc: 0x0387, ..Intel8080State::default() };
        let entry = InstructionTraceEntry {
            sequence: 1,
            address: 0x0382,
            bytes: [0x00, 0x00, 0x00],
            length: 1,
            t_states: 4,
            before: CpuSnapshot8080 { pc: 0x0382, ..CpuSnapshot8080::default() },
            after: CpuSnapshot8080 { pc: 0x0385, ..CpuSnapshot8080::default() },
            effects: Vec::new(),
        };
        assert_eq!(stable_address(cpu, true, Some(&entry)), 0x0385);
    }

    #[test]
    fn halted_display_keeps_hlt_opcode_address() {
        let cpu = Intel8080State { pc: 0x0101, halted: Some(true), ..Intel8080State::default() };
        let entry = InstructionTraceEntry {
            sequence: 1,
            address: 0x0100,
            bytes: [0x76, 0x00, 0x00],
            length: 1,
            t_states: 7,
            before: CpuSnapshot8080 { pc: 0x0100, ..CpuSnapshot8080::default() },
            after: CpuSnapshot8080 { pc: 0x0101, halted: true, ..CpuSnapshot8080::default() },
            effects: Vec::new(),
        };
        assert_eq!(stable_address(cpu, true, Some(&entry)), 0x0100);
    }
}

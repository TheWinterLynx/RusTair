use std::collections::VecDeque;

use crate::decoder8080::{decode_8080, ControlFlow, DecodedInstruction};

pub const DEFAULT_INSTRUCTION_HISTORY_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuSnapshot8080 {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub flags: u8,
    pub pc: u16,
    pub sp: u16,
    pub inte: bool,
    pub halted: bool,
}

impl CpuSnapshot8080 {
    pub const fn af(self) -> u16 { ((self.a as u16) << 8) | self.flags as u16 }
    pub const fn bc(self) -> u16 { ((self.b as u16) << 8) | self.c as u16 }
    pub const fn de(self) -> u16 { ((self.d as u16) << 8) | self.e as u16 }
    pub const fn hl(self) -> u16 { ((self.h as u16) << 8) | self.l as u16 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionEffect8080 {
    MemoryRead { address: u16, value: u8 },
    MemoryWrite { address: u16, value: u8 },
    StackRead { address: u16, value: u8 },
    StackWrite { address: u16, value: u8 },
    IoRead { port: u8, value: u8 },
    IoWrite { port: u8, value: u8 },
}

impl InstructionEffect8080 {
    pub fn label(self) -> String {
        match self {
            Self::MemoryRead { address, value } => format!("READ  [${address:04X}] -> ${value:02X}"),
            Self::MemoryWrite { address, value } => format!("WRITE [${address:04X}] <- ${value:02X}"),
            Self::StackRead { address, value } => format!("STACK READ  [${address:04X}] -> ${value:02X}"),
            Self::StackWrite { address, value } => format!("STACK WRITE [${address:04X}] <- ${value:02X}"),
            Self::IoRead { port, value } => format!("IN  ${port:02X} -> ${value:02X}"),
            Self::IoWrite { port, value } => format!("OUT ${port:02X} <- ${value:02X}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionTraceEntry {
    pub sequence: u64,
    pub address: u16,
    pub bytes: [u8; 3],
    pub length: u8,
    pub t_states: u32,
    pub before: CpuSnapshot8080,
    pub after: CpuSnapshot8080,
    pub effects: Vec<InstructionEffect8080>,
}

impl InstructionTraceEntry {
    pub fn opcode(&self) -> u8 { self.bytes[0] }

    pub fn bytes_text(&self) -> String {
        match self.length {
            1 => format!("{:02X}", self.bytes[0]),
            2 => format!("{:02X} {:02X}", self.bytes[0], self.bytes[1]),
            _ => format!("{:02X} {:02X} {:02X}", self.bytes[0], self.bytes[1], self.bytes[2]),
        }
    }
}

fn read_effect<F>(effects: &mut Vec<InstructionEffect8080>, address: u16, mut peek: F)
where
    F: FnMut(u16) -> Option<u8>,
{
    if let Some(value) = peek(address) {
        effects.push(InstructionEffect8080::MemoryRead { address, value });
    }
}

fn stack_read_effect<F>(effects: &mut Vec<InstructionEffect8080>, address: u16, mut peek: F)
where
    F: FnMut(u16) -> Option<u8>,
{
    if let Some(value) = peek(address) {
        effects.push(InstructionEffect8080::StackRead { address, value });
    }
}

fn write_effect<F>(effects: &mut Vec<InstructionEffect8080>, address: u16, mut peek: F)
where
    F: FnMut(u16) -> Option<u8>,
{
    if let Some(value) = peek(address) {
        effects.push(InstructionEffect8080::MemoryWrite { address, value });
    }
}

fn stack_write_effect<F>(effects: &mut Vec<InstructionEffect8080>, address: u16, mut peek: F)
where
    F: FnMut(u16) -> Option<u8>,
{
    if let Some(value) = peek(address) {
        effects.push(InstructionEffect8080::StackWrite { address, value });
    }
}

fn branch_taken(decoded: &DecodedInstruction, before: CpuSnapshot8080) -> bool {
    decoded
        .control_flow
        .condition()
        .map(|condition| condition.evaluate(before.flags))
        .unwrap_or(true)
}

/// Collect guest-visible data reads before execution mutates memory. Instruction
/// fetch/operand fetches are intentionally excluded: these events describe the
/// data/stack/I/O side effects a programmer reasons about for the instruction.
pub fn collect_pre_instruction_effects<F>(
    bytes: [u8; 3],
    before: CpuSnapshot8080,
    mut peek: F,
) -> Vec<InstructionEffect8080>
where
    F: FnMut(u16) -> Option<u8>,
{
    let decoded = decode_8080(bytes[0], bytes[1], bytes[2]);
    let mut effects = Vec::new();
    let op0 = decoded.operands.first().map(String::as_str);
    let op1 = decoded.operands.get(1).map(String::as_str);

    match decoded.mnemonic {
        "MOV" if op1 == Some("M") => read_effect(&mut effects, before.hl(), &mut peek),
        "ADD" | "ADC" | "SUB" | "SBB" | "ANA" | "XRA" | "ORA" | "CMP"
            if op0 == Some("M") => read_effect(&mut effects, before.hl(), &mut peek),
        "INR" | "DCR" if op0 == Some("M") => read_effect(&mut effects, before.hl(), &mut peek),
        "LDAX" => {
            let address = if op0 == Some("B") { before.bc() } else { before.de() };
            read_effect(&mut effects, address, &mut peek);
        }
        "LDA" => {
            if let Some(address) = decoded.immediate16 { read_effect(&mut effects, address, &mut peek); }
        }
        "LHLD" => {
            if let Some(address) = decoded.immediate16 {
                read_effect(&mut effects, address, &mut peek);
                read_effect(&mut effects, address.wrapping_add(1), &mut peek);
            }
        }
        "POP" | "XTHL" => {
            stack_read_effect(&mut effects, before.sp, &mut peek);
            stack_read_effect(&mut effects, before.sp.wrapping_add(1), &mut peek);
        }
        "RET" => {
            stack_read_effect(&mut effects, before.sp, &mut peek);
            stack_read_effect(&mut effects, before.sp.wrapping_add(1), &mut peek);
        }
        _ if matches!(decoded.control_flow, ControlFlow::Return { condition: Some(_) })
            && branch_taken(&decoded, before) => {
            stack_read_effect(&mut effects, before.sp, &mut peek);
            stack_read_effect(&mut effects, before.sp.wrapping_add(1), &mut peek);
        }
        _ => {}
    }

    effects
}

/// Collect writes and I/O after execution so the recorded value is the value the
/// guest actually left in RAM or transferred through the selected I/O port.
pub fn collect_post_instruction_effects<F>(
    bytes: [u8; 3],
    before: CpuSnapshot8080,
    after: CpuSnapshot8080,
    mut peek: F,
) -> Vec<InstructionEffect8080>
where
    F: FnMut(u16) -> Option<u8>,
{
    let decoded = decode_8080(bytes[0], bytes[1], bytes[2]);
    let mut effects = Vec::new();
    let op0 = decoded.operands.first().map(String::as_str);

    match decoded.mnemonic {
        "MOV" if op0 == Some("M") => write_effect(&mut effects, before.hl(), &mut peek),
        "MVI" if op0 == Some("M") => write_effect(&mut effects, before.hl(), &mut peek),
        "INR" | "DCR" if op0 == Some("M") => write_effect(&mut effects, before.hl(), &mut peek),
        "STAX" => {
            let address = if op0 == Some("B") { before.bc() } else { before.de() };
            write_effect(&mut effects, address, &mut peek);
        }
        "STA" => {
            if let Some(address) = decoded.immediate16 { write_effect(&mut effects, address, &mut peek); }
        }
        "SHLD" => {
            if let Some(address) = decoded.immediate16 {
                write_effect(&mut effects, address, &mut peek);
                write_effect(&mut effects, address.wrapping_add(1), &mut peek);
            }
        }
        "XTHL" => {
            stack_write_effect(&mut effects, before.sp, &mut peek);
            stack_write_effect(&mut effects, before.sp.wrapping_add(1), &mut peek);
        }
        "PUSH" | "RST" => {
            stack_write_effect(&mut effects, after.sp, &mut peek);
            stack_write_effect(&mut effects, after.sp.wrapping_add(1), &mut peek);
        }
        "CALL" if branch_taken(&decoded, before) => {
            stack_write_effect(&mut effects, after.sp, &mut peek);
            stack_write_effect(&mut effects, after.sp.wrapping_add(1), &mut peek);
        }
        _ if matches!(decoded.control_flow, ControlFlow::Call { condition: Some(_), .. })
            && branch_taken(&decoded, before) => {
            stack_write_effect(&mut effects, after.sp, &mut peek);
            stack_write_effect(&mut effects, after.sp.wrapping_add(1), &mut peek);
        }
        _ => {}
    }

    match decoded.mnemonic {
        "IN" => {
            if let Some(port) = decoded.immediate8 {
                effects.push(InstructionEffect8080::IoRead { port, value: after.a });
            }
        }
        "OUT" => {
            if let Some(port) = decoded.immediate8 {
                effects.push(InstructionEffect8080::IoWrite { port, value: before.a });
            }
        }
        _ => {}
    }

    effects
}

#[derive(Debug)]
pub struct InstructionTraceBuffer {
    enabled: bool,
    limit: usize,
    next_sequence: u64,
    entries: VecDeque<InstructionTraceEntry>,
}

impl Default for InstructionTraceBuffer {
    fn default() -> Self { Self::new(DEFAULT_INSTRUCTION_HISTORY_LIMIT) }
}

impl InstructionTraceBuffer {
    pub fn new(limit: usize) -> Self {
        Self {
            enabled: false,
            limit: limit.max(1),
            next_sequence: 1,
            entries: VecDeque::with_capacity(limit.max(1)),
        }
    }

    pub fn enabled(&self) -> bool { self.enabled }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn snapshot(&self) -> Vec<InstructionTraceEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn push(
        &mut self,
        address: u16,
        bytes: [u8; 3],
        before: CpuSnapshot8080,
        after: CpuSnapshot8080,
        t_states: u32,
    ) {
        self.push_with_effects(address, bytes, before, after, t_states, Vec::new());
    }

    pub fn push_with_effects(
        &mut self,
        address: u16,
        bytes: [u8; 3],
        before: CpuSnapshot8080,
        after: CpuSnapshot8080,
        t_states: u32,
        effects: Vec<InstructionEffect8080>,
    ) {
        if !self.enabled {
            return;
        }
        let decoded = decode_8080(bytes[0], bytes[1], bytes[2]);
        let entry = InstructionTraceEntry {
            sequence: self.next_sequence,
            address,
            bytes,
            length: decoded.length,
            t_states,
            before,
            after,
            effects,
        };
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        if self.entries.len() == self.limit {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded_and_keeps_sequence_order() {
        let mut history = InstructionTraceBuffer::new(2);
        history.set_enabled(true);
        for address in 0..3u16 {
            history.push(
                address,
                [0x00, 0, 0],
                CpuSnapshot8080 { pc: address, ..CpuSnapshot8080::default() },
                CpuSnapshot8080 { pc: address + 1, ..CpuSnapshot8080::default() },
                4,
            );
        }
        let snapshot = history.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].sequence, 2);
        assert_eq!(snapshot[1].sequence, 3);
        assert_eq!(snapshot[1].address, 2);
    }

    #[test]
    fn disabled_history_has_zero_runtime_entries() {
        let mut history = InstructionTraceBuffer::default();
        history.push(0, [0, 0, 0], CpuSnapshot8080::default(), CpuSnapshot8080::default(), 4);
        assert!(history.snapshot().is_empty());
    }

    #[test]
    fn derives_m_read_write_and_io_effects() {
        let before = CpuSnapshot8080 { h: 0x12, l: 0x34, a: 0x55, sp: 0x0200, ..CpuSnapshot8080::default() };
        let after = CpuSnapshot8080 { h: 0x12, l: 0x34, a: 0x66, sp: 0x0200, ..before };

        let pre = collect_pre_instruction_effects([0x7e, 0, 0], before, |address| (address == 0x1234).then_some(0x66));
        assert_eq!(pre, vec![InstructionEffect8080::MemoryRead { address: 0x1234, value: 0x66 }]);

        let post = collect_post_instruction_effects([0x77, 0, 0], before, after, |address| (address == 0x1234).then_some(0x55));
        assert_eq!(post, vec![InstructionEffect8080::MemoryWrite { address: 0x1234, value: 0x55 }]);

        let input = collect_post_instruction_effects([0xdb, 0x10, 0], before, after, |_| None);
        assert_eq!(input, vec![InstructionEffect8080::IoRead { port: 0x10, value: 0x66 }]);
        let output = collect_post_instruction_effects([0xd3, 0x11, 0], before, after, |_| None);
        assert_eq!(output, vec![InstructionEffect8080::IoWrite { port: 0x11, value: 0x55 }]);
    }
}

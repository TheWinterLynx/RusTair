use std::collections::VecDeque;

use crate::cpu8080::FLAG_1;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InstructionTraceMetadata {
    pub generation: u64,
    pub dropped_entries: u64,
    pub capacity: usize,
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

fn read_effect<F>(effects: &mut Vec<InstructionEffect8080>, address: u16, guest_read: &mut F)
where
    F: FnMut(u16) -> u8,
{
    effects.push(InstructionEffect8080::MemoryRead {
        address,
        value: guest_read(address),
    });
}

fn stack_read_effect<F>(effects: &mut Vec<InstructionEffect8080>, address: u16, guest_read: &mut F)
where
    F: FnMut(u16) -> u8,
{
    effects.push(InstructionEffect8080::StackRead {
        address,
        value: guest_read(address),
    });
}

fn memory_write(effects: &mut Vec<InstructionEffect8080>, address: u16, value: u8) {
    effects.push(InstructionEffect8080::MemoryWrite { address, value });
}

fn stack_write(effects: &mut Vec<InstructionEffect8080>, address: u16, value: u8) {
    effects.push(InstructionEffect8080::StackWrite { address, value });
}

/// 8080 PUSH/CALL/RST write the high byte first at SP-1 and the low byte
/// second at SP-2. Keep trace effects in electrical/temporal order so a pair of
/// stack watchpoints reports the same first transfer the CPU actually performs.
fn stack_push_word(effects: &mut Vec<InstructionEffect8080>, before_sp: u16, value: u16) {
    stack_write(
        effects,
        before_sp.wrapping_sub(1),
        (value >> 8) as u8,
    );
    stack_write(effects, before_sp.wrapping_sub(2), value as u8);
}

fn branch_taken(decoded: &DecodedInstruction, before: CpuSnapshot8080) -> bool {
    decoded
        .control_flow
        .condition()
        .map(|condition| condition.evaluate(before.flags))
        .unwrap_or(true)
}

fn register8(snapshot: CpuSnapshot8080, operand: &str) -> Option<u8> {
    match operand {
        "A" => Some(snapshot.a),
        "B" => Some(snapshot.b),
        "C" => Some(snapshot.c),
        "D" => Some(snapshot.d),
        "E" => Some(snapshot.e),
        "H" => Some(snapshot.h),
        "L" => Some(snapshot.l),
        _ => None,
    }
}

fn pushed_pair(snapshot: CpuSnapshot8080, operand: &str) -> Option<u16> {
    match operand {
        "B" => Some(snapshot.bc()),
        "D" => Some(snapshot.de()),
        "H" => Some(snapshot.hl()),
        "PSW" => {
            let flags = (snapshot.flags & 0xd5) | FLAG_1;
            Some(((snapshot.a as u16) << 8) | u16::from(flags))
        }
        _ => None,
    }
}

fn pre_memory_read(effects: &[InstructionEffect8080], address: u16) -> Option<u8> {
    effects.iter().find_map(|effect| match *effect {
        InstructionEffect8080::MemoryRead { address: candidate, value } if candidate == address => {
            Some(value)
        }
        _ => None,
    })
}

/// Collect guest-visible data reads before execution mutates state/memory.
/// Instruction/operand fetches are intentionally excluded. `guest_read` must be
/// a non-destructive preview of the byte that the emulated CPU would receive.
pub fn collect_pre_instruction_effects<F>(
    bytes: [u8; 3],
    before: CpuSnapshot8080,
    mut guest_read: F,
) -> Vec<InstructionEffect8080>
where
    F: FnMut(u16) -> u8,
{
    let decoded = decode_8080(bytes[0], bytes[1], bytes[2]);
    let mut effects = Vec::new();
    let op0 = decoded.operands.first().map(String::as_str);
    let op1 = decoded.operands.get(1).map(String::as_str);

    match decoded.mnemonic {
        "MOV" if op1 == Some("M") => read_effect(&mut effects, before.hl(), &mut guest_read),
        "ADD" | "ADC" | "SUB" | "SBB" | "ANA" | "XRA" | "ORA" | "CMP"
            if op0 == Some("M") => read_effect(&mut effects, before.hl(), &mut guest_read),
        "INR" | "DCR" if op0 == Some("M") => read_effect(&mut effects, before.hl(), &mut guest_read),
        "LDAX" => {
            let address = if op0 == Some("B") { before.bc() } else { before.de() };
            read_effect(&mut effects, address, &mut guest_read);
        }
        "LDA" => {
            if let Some(address) = decoded.immediate16 {
                read_effect(&mut effects, address, &mut guest_read);
            }
        }
        "LHLD" => {
            if let Some(address) = decoded.immediate16 {
                read_effect(&mut effects, address, &mut guest_read);
                read_effect(&mut effects, address.wrapping_add(1), &mut guest_read);
            }
        }
        "POP" | "XTHL" => {
            stack_read_effect(&mut effects, before.sp, &mut guest_read);
            stack_read_effect(&mut effects, before.sp.wrapping_add(1), &mut guest_read);
        }
        "RET" => {
            stack_read_effect(&mut effects, before.sp, &mut guest_read);
            stack_read_effect(&mut effects, before.sp.wrapping_add(1), &mut guest_read);
        }
        _ if matches!(decoded.control_flow, ControlFlow::Return { condition: Some(_) })
            && branch_taken(&decoded, before) =>
        {
            stack_read_effect(&mut effects, before.sp, &mut guest_read);
            stack_read_effect(&mut effects, before.sp.wrapping_add(1), &mut guest_read);
        }
        _ => {}
    }

    effects
}

/// Collect guest-visible writes and I/O after the instruction completes. Write
/// values are derived from 8080 semantics rather than re-reading RAM afterwards,
/// so an attempted write remains visible when protection/uninstalled memory
/// prevents the physical cell from changing.
pub fn collect_post_instruction_effects(
    bytes: [u8; 3],
    before: CpuSnapshot8080,
    after: CpuSnapshot8080,
    pre_effects: &[InstructionEffect8080],
) -> Vec<InstructionEffect8080> {
    let decoded = decode_8080(bytes[0], bytes[1], bytes[2]);
    let mut effects = Vec::new();
    let op0 = decoded.operands.first().map(String::as_str);
    let op1 = decoded.operands.get(1).map(String::as_str);

    match decoded.mnemonic {
        "MOV" if op0 == Some("M") => {
            if let Some(value) = op1.and_then(|operand| register8(before, operand)) {
                memory_write(&mut effects, before.hl(), value);
            }
        }
        "MVI" if op0 == Some("M") => {
            if let Some(value) = decoded.immediate8 {
                memory_write(&mut effects, before.hl(), value);
            }
        }
        "INR" | "DCR" if op0 == Some("M") => {
            if let Some(value) = pre_memory_read(pre_effects, before.hl()) {
                let written = if decoded.mnemonic == "INR" {
                    value.wrapping_add(1)
                } else {
                    value.wrapping_sub(1)
                };
                memory_write(&mut effects, before.hl(), written);
            }
        }
        "STAX" => {
            let address = if op0 == Some("B") { before.bc() } else { before.de() };
            memory_write(&mut effects, address, before.a);
        }
        "STA" => {
            if let Some(address) = decoded.immediate16 {
                memory_write(&mut effects, address, before.a);
            }
        }
        "SHLD" => {
            if let Some(address) = decoded.immediate16 {
                memory_write(&mut effects, address, before.l);
                memory_write(&mut effects, address.wrapping_add(1), before.h);
            }
        }
        "XTHL" => {
            stack_write(&mut effects, before.sp, before.l);
            stack_write(&mut effects, before.sp.wrapping_add(1), before.h);
        }
        "PUSH" => {
            if let Some(value) = op0.and_then(|operand| pushed_pair(before, operand)) {
                stack_push_word(&mut effects, before.sp, value);
            }
        }
        "RST" => {
            let return_address = before.pc.wrapping_add(u16::from(decoded.length));
            stack_push_word(&mut effects, before.sp, return_address);
        }
        "CALL" if branch_taken(&decoded, before) => {
            let return_address = before.pc.wrapping_add(u16::from(decoded.length));
            stack_push_word(&mut effects, before.sp, return_address);
        }
        _ if matches!(decoded.control_flow, ControlFlow::Call { condition: Some(_), .. })
            && branch_taken(&decoded, before) =>
        {
            let return_address = before.pc.wrapping_add(u16::from(decoded.length));
            stack_push_word(&mut effects, before.sp, return_address);
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
    generation: u64,
    dropped_since_clear: u64,
    entries: VecDeque<InstructionTraceEntry>,
}

impl Default for InstructionTraceBuffer {
    fn default() -> Self { Self::new(DEFAULT_INSTRUCTION_HISTORY_LIMIT) }
}

impl InstructionTraceBuffer {
    pub fn new(limit: usize) -> Self {
        let limit = limit.max(1);
        Self {
            enabled: false,
            limit,
            next_sequence: 1,
            generation: 1,
            dropped_since_clear: 0,
            entries: VecDeque::with_capacity(limit),
        }
    }

    pub fn enabled(&self) -> bool { self.enabled }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.dropped_since_clear = 0;
        self.generation = self.generation.wrapping_add(1).max(1);
    }

    pub fn snapshot(&self) -> Vec<InstructionTraceEntry> {
        self.entries.iter().cloned().collect()
    }

    pub const fn metadata(&self) -> InstructionTraceMetadata {
        InstructionTraceMetadata {
            generation: self.generation,
            dropped_entries: self.dropped_since_clear,
            capacity: self.limit,
        }
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
            self.dropped_since_clear = self.dropped_since_clear.saturating_add(1);
        }
        self.entries.push_back(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded_and_reports_actual_eviction() {
        let mut history = InstructionTraceBuffer::new(2);
        history.set_enabled(true);
        let generation = history.metadata().generation;
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
        assert_eq!(history.metadata().dropped_entries, 1);
        assert_eq!(history.metadata().generation, generation);

        history.clear();
        assert_eq!(history.metadata().dropped_entries, 0);
        assert_ne!(history.metadata().generation, generation);
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

        let pre = collect_pre_instruction_effects([0x7e, 0, 0], before, |address| {
            if address == 0x1234 { 0x66 } else { 0 }
        });
        assert_eq!(pre, vec![InstructionEffect8080::MemoryRead { address: 0x1234, value: 0x66 }]);

        let post = collect_post_instruction_effects([0x77, 0, 0], before, after, &[]);
        assert_eq!(post, vec![InstructionEffect8080::MemoryWrite { address: 0x1234, value: 0x55 }]);

        let input = collect_post_instruction_effects([0xdb, 0x10, 0], before, after, &[]);
        assert_eq!(input, vec![InstructionEffect8080::IoRead { port: 0x10, value: 0x66 }]);
        let output = collect_post_instruction_effects([0xd3, 0x11, 0], before, after, &[]);
        assert_eq!(output, vec![InstructionEffect8080::IoWrite { port: 0x11, value: 0x55 }]);
    }

    #[test]
    fn protected_or_uninstalled_write_value_does_not_depend_on_post_ram() {
        let before = CpuSnapshot8080 { h: 0x20, l: 0x00, a: 0xa5, ..CpuSnapshot8080::default() };
        let post = collect_post_instruction_effects([0x77, 0, 0], before, before, &[]);
        assert_eq!(post, vec![InstructionEffect8080::MemoryWrite { address: 0x2000, value: 0xa5 }]);
    }

    #[test]
    fn inr_m_reports_read_and_attempted_write() {
        let before = CpuSnapshot8080 { h: 0x01, l: 0x00, ..CpuSnapshot8080::default() };
        let pre = collect_pre_instruction_effects([0x34, 0, 0], before, |_| 0xff);
        let mut effects = pre.clone();
        effects.extend(collect_post_instruction_effects([0x34, 0, 0], before, before, &pre));
        assert_eq!(effects, vec![
            InstructionEffect8080::MemoryRead { address: 0x0100, value: 0xff },
            InstructionEffect8080::MemoryWrite { address: 0x0100, value: 0x00 },
        ]);
    }

    #[test]
    fn push_and_call_stack_writes_follow_real_bus_order_and_wrap() {
        let before = CpuSnapshot8080 {
            b: 0x12,
            c: 0x34,
            pc: 0xffff,
            sp: 0x0000,
            ..CpuSnapshot8080::default()
        };
        let after_push = CpuSnapshot8080 {
            sp: 0xfffe,
            ..before
        };
        assert_eq!(
            collect_post_instruction_effects([0xc5, 0, 0], before, after_push, &[]),
            vec![
                InstructionEffect8080::StackWrite { address: 0xffff, value: 0x12 },
                InstructionEffect8080::StackWrite { address: 0xfffe, value: 0x34 },
            ]
        );

        let after_call = CpuSnapshot8080 {
            pc: 0x1234,
            sp: 0xfffe,
            ..before
        };
        // CALL at FFFFh has a wrapping return address of 0002h.
        assert_eq!(
            collect_post_instruction_effects([0xcd, 0x34, 0x12], before, after_call, &[]),
            vec![
                InstructionEffect8080::StackWrite { address: 0xffff, value: 0x00 },
                InstructionEffect8080::StackWrite { address: 0xfffe, value: 0x02 },
            ]
        );
    }
}

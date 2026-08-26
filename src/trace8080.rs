use std::collections::VecDeque;

use crate::decoder8080::decode_8080;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionTraceEntry {
    pub sequence: u64,
    pub address: u16,
    pub bytes: [u8; 3],
    pub length: u8,
    pub t_states: u32,
    pub before: CpuSnapshot8080,
    pub after: CpuSnapshot8080,
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
}

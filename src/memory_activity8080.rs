use std::collections::BTreeMap;

use crate::trace8080::{
    InstructionEffect8080, InstructionTraceEntry, DEFAULT_INSTRUCTION_HISTORY_LIMIT,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemoryActivity8080 {
    pub execute_count: u64,
    pub read_count: u64,
    pub write_count: u64,
    pub last_execute_sequence: Option<u64>,
    pub last_read_sequence: Option<u64>,
    pub last_write_sequence: Option<u64>,
}

impl MemoryActivity8080 {
    pub const fn any(self) -> bool {
        self.execute_count != 0 || self.read_count != 0 || self.write_count != 0
    }

    pub const fn total(self) -> u64 {
        self.execute_count
            .saturating_add(self.read_count)
            .saturating_add(self.write_count)
    }

    pub const fn last_sequence(self) -> Option<u64> {
        let mut latest = self.last_execute_sequence;
        if let Some(sequence) = self.last_read_sequence {
            latest = Some(match latest { Some(current) => current.max(sequence), None => sequence });
        }
        if let Some(sequence) = self.last_write_sequence {
            latest = Some(match latest { Some(current) => current.max(sequence), None => sequence });
        }
        latest
    }
}

#[derive(Clone, Debug, Default)]
pub struct MemoryActivityMap8080 {
    by_address: BTreeMap<u16, MemoryActivity8080>,
    pub first_sequence: Option<u64>,
    pub last_sequence: Option<u64>,
    pub history_gap: bool,
}

impl MemoryActivityMap8080 {
    pub fn get(&self, address: u16) -> MemoryActivity8080 {
        self.by_address.get(&address).copied().unwrap_or_default()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, MemoryActivity8080)> + '_ {
        self.by_address.iter().map(|(&address, &activity)| (address, activity))
    }

    pub fn active_addresses(&self) -> usize {
        self.by_address.len()
    }
}

pub fn summarize_memory_activity_8080(history: &[InstructionTraceEntry]) -> MemoryActivityMap8080 {
    let mut result = MemoryActivityMap8080::default();
    let Some(first) = history.first() else { return result; };
    result.first_sequence = Some(first.sequence);
    result.last_sequence = history.last().map(|entry| entry.sequence);
    result.history_gap = history.len() >= DEFAULT_INSTRUCTION_HISTORY_LIMIT && first.sequence > 1;

    let mut expected = first.sequence;
    for entry in history {
        if entry.sequence != expected {
            result.history_gap = true;
        }
        expected = entry.sequence.wrapping_add(1);

        let execute = result.by_address.entry(entry.address).or_default();
        execute.execute_count = execute.execute_count.saturating_add(1);
        execute.last_execute_sequence = Some(entry.sequence);

        for effect in &entry.effects {
            match *effect {
                InstructionEffect8080::MemoryRead { address, .. }
                | InstructionEffect8080::StackRead { address, .. } => {
                    let activity = result.by_address.entry(address).or_default();
                    activity.read_count = activity.read_count.saturating_add(1);
                    activity.last_read_sequence = Some(entry.sequence);
                }
                InstructionEffect8080::MemoryWrite { address, .. }
                | InstructionEffect8080::StackWrite { address, .. } => {
                    let activity = result.by_address.entry(address).or_default();
                    activity.write_count = activity.write_count.saturating_add(1);
                    activity.last_write_sequence = Some(entry.sequence);
                }
                InstructionEffect8080::IoRead { .. }
                | InstructionEffect8080::IoWrite { .. } => {}
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace8080::CpuSnapshot8080;

    fn trace(sequence: u64, address: u16, effects: Vec<InstructionEffect8080>) -> InstructionTraceEntry {
        InstructionTraceEntry {
            sequence,
            address,
            bytes: [0, 0, 0],
            length: 1,
            t_states: 4,
            before: CpuSnapshot8080 { pc: address, ..CpuSnapshot8080::default() },
            after: CpuSnapshot8080 { pc: address.wrapping_add(1), ..CpuSnapshot8080::default() },
            effects,
        }
    }

    #[test]
    fn separates_execute_read_and_write_activity() {
        let history = vec![
            trace(1, 0x0100, vec![InstructionEffect8080::MemoryRead { address: 0x0200, value: 0x11 }]),
            trace(2, 0x0101, vec![InstructionEffect8080::MemoryWrite { address: 0x0200, value: 0x22 }]),
            trace(3, 0x0100, vec![InstructionEffect8080::StackWrite { address: 0x0ffe, value: 0x01 }]),
        ];
        let map = summarize_memory_activity_8080(&history);
        let code = map.get(0x0100);
        assert_eq!(code.execute_count, 2);
        assert_eq!(code.last_execute_sequence, Some(3));
        let data = map.get(0x0200);
        assert_eq!(data.read_count, 1);
        assert_eq!(data.write_count, 1);
        assert_eq!(data.last_sequence(), Some(2));
        assert_eq!(map.get(0x0ffe).write_count, 1);
        assert!(!map.history_gap);
    }

    #[test]
    fn clear_baseline_with_high_sequence_is_not_a_gap() {
        let fresh_after_clear = summarize_memory_activity_8080(&[trace(8, 0, Vec::new())]);
        assert!(!fresh_after_clear.history_gap);
    }

    #[test]
    fn sequence_holes_are_reported() {
        let hole = summarize_memory_activity_8080(&[
            trace(1, 0, Vec::new()),
            trace(3, 1, Vec::new()),
        ]);
        assert!(hole.history_gap);
    }

    #[test]
    fn full_shifted_buffer_is_reported_as_truncated() {
        let history: Vec<_> = (0..DEFAULT_INSTRUCTION_HISTORY_LIMIT)
            .map(|index| trace(index as u64 + 8, index as u16, Vec::new()))
            .collect();
        assert!(summarize_memory_activity_8080(&history).history_gap);
    }
}

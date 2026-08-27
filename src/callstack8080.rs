use crate::decoder8080::{decode_8080, ControlFlow};
use crate::trace8080::{InstructionTraceEntry, DEFAULT_INSTRUCTION_HISTORY_LIMIT};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallKind8080 {
    Call,
    Restart,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InferredCallFrame8080 {
    pub kind: CallKind8080,
    pub call_site: u16,
    pub target: u16,
    pub return_address: u16,
    pub stack_pointer_after_push: u16,
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InferredCallStack8080 {
    pub frames: Vec<InferredCallFrame8080>,
    /// True when execution before the retained trace may contain older frames,
    /// or when a sequence gap/mismatched return makes the lower stack uncertain.
    pub incomplete: bool,
    pub last_sequence: Option<u64>,
    pub diagnostic: Option<String>,
}

pub fn infer_call_stack_8080(history: &[InstructionTraceEntry]) -> InferredCallStack8080 {
    let mut result = InferredCallStack8080::default();
    let Some(first) = history.first() else { return result; };

    // Sequence numbers intentionally survive Clear. Therefore first.sequence > 1
    // alone does not prove truncation. A full bounded buffer whose first entry is
    // no longer #1 does prove that older execution was evicted.
    if history.len() >= DEFAULT_INSTRUCTION_HISTORY_LIMIT && first.sequence > 1 {
        result.incomplete = true;
        result.diagnostic = Some("older execution was evicted from the bounded history".into());
    }

    let mut expected_sequence = first.sequence;
    for entry in history {
        if entry.sequence != expected_sequence {
            result.frames.clear();
            result.incomplete = true;
            result.diagnostic = Some(format!(
                "instruction-history gap before sequence {}",
                entry.sequence
            ));
        }
        expected_sequence = entry.sequence.wrapping_add(1);
        result.last_sequence = Some(entry.sequence);

        let decoded = decode_8080(entry.bytes[0], entry.bytes[1], entry.bytes[2]);
        let sequential = entry.address.wrapping_add(u16::from(decoded.length));
        match decoded.control_flow {
            ControlFlow::Call { target, .. } => {
                if entry.after.pc == target {
                    result.frames.push(InferredCallFrame8080 {
                        kind: CallKind8080::Call,
                        call_site: entry.address,
                        target,
                        return_address: sequential,
                        stack_pointer_after_push: entry.after.sp,
                        sequence: entry.sequence,
                    });
                }
            }
            ControlFlow::Restart { vector } => {
                if entry.after.pc == vector {
                    result.frames.push(InferredCallFrame8080 {
                        kind: CallKind8080::Restart,
                        call_site: entry.address,
                        target: vector,
                        return_address: sequential,
                        stack_pointer_after_push: entry.after.sp,
                        sequence: entry.sequence,
                    });
                }
            }
            ControlFlow::Return { condition } => {
                let taken = condition.is_none() || entry.after.pc != sequential;
                if !taken {
                    continue;
                }

                if result.frames.last().map(|frame| frame.return_address) == Some(entry.after.pc) {
                    result.frames.pop();
                } else {
                    result.frames.clear();
                    result.incomplete = true;
                    result.diagnostic = Some(format!(
                        "RET at ${:04X} returned to ${:04X} without a matching retained CALL",
                        entry.address, entry.after.pc
                    ));
                }
            }
            _ => {}
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace8080::CpuSnapshot8080;

    fn entry(
        sequence: u64,
        address: u16,
        bytes: [u8; 3],
        before_sp: u16,
        after_pc: u16,
        after_sp: u16,
    ) -> InstructionTraceEntry {
        let decoded = decode_8080(bytes[0], bytes[1], bytes[2]);
        InstructionTraceEntry {
            sequence,
            address,
            bytes,
            length: decoded.length,
            t_states: decoded.timing.base_t_states.into(),
            before: CpuSnapshot8080 { pc: address, sp: before_sp, ..CpuSnapshot8080::default() },
            after: CpuSnapshot8080 { pc: after_pc, sp: after_sp, ..CpuSnapshot8080::default() },
            effects: Vec::new(),
        }
    }

    #[test]
    fn taken_call_and_matching_ret_leave_empty_stack() {
        let history = vec![
            entry(1, 0x0100, [0xcd, 0x00, 0x02], 0x1000, 0x0200, 0x0ffe),
            entry(2, 0x0200, [0xc9, 0, 0], 0x0ffe, 0x0103, 0x1000),
        ];
        let stack = infer_call_stack_8080(&history);
        assert!(stack.frames.is_empty());
        assert!(!stack.incomplete);
    }

    #[test]
    fn nested_calls_preserve_live_frames_in_order() {
        let history = vec![
            entry(1, 0x0100, [0xcd, 0x00, 0x02], 0x1000, 0x0200, 0x0ffe),
            entry(2, 0x0200, [0xcd, 0x00, 0x03], 0x0ffe, 0x0300, 0x0ffc),
        ];
        let stack = infer_call_stack_8080(&history);
        assert_eq!(stack.frames.len(), 2);
        assert_eq!(stack.frames[0].return_address, 0x0103);
        assert_eq!(stack.frames[1].return_address, 0x0203);
        assert_eq!(stack.frames[1].stack_pointer_after_push, 0x0ffc);
    }

    #[test]
    fn conditional_call_not_taken_does_not_create_frame() {
        let history = vec![entry(1, 0x0100, [0xcc, 0x00, 0x02], 0x1000, 0x0103, 0x1000)];
        assert!(infer_call_stack_8080(&history).frames.is_empty());
    }

    #[test]
    fn clear_baseline_with_high_sequence_is_not_mistaken_for_truncation() {
        let history = vec![entry(42, 0x0200, [0x00, 0, 0], 0x1000, 0x0201, 0x1000)];
        assert!(!infer_call_stack_8080(&history).incomplete);
    }

    #[test]
    fn full_shifted_history_is_marked_truncated() {
        let history: Vec<_> = (0..DEFAULT_INSTRUCTION_HISTORY_LIMIT)
            .map(|index| {
                let sequence = index as u64 + 42;
                entry(sequence, index as u16, [0x00, 0, 0], 0x1000, index as u16 + 1, 0x1000)
            })
            .collect();
        assert!(infer_call_stack_8080(&history).incomplete);
    }
}

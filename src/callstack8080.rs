use crate::decoder8080::{decode_8080, ControlFlow};
use crate::trace8080::{InstructionTraceEntry, InstructionTraceMetadata};

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
    /// A known loss/mismatch occurred inside the retained observation window.
    /// Even when false, callers predating capture are deliberately not claimed.
    pub incomplete: bool,
    pub last_sequence: Option<u64>,
    pub diagnostic: Option<String>,
}

pub fn infer_call_stack_8080(
    history: &[InstructionTraceEntry],
    metadata: InstructionTraceMetadata,
) -> InferredCallStack8080 {
    let mut result = InferredCallStack8080::default();
    let Some(first) = history.first() else { return result; };

    if metadata.dropped_entries != 0 {
        result.incomplete = true;
        result.diagnostic = Some(format!(
            "{} older trace entr{} evicted from the bounded history",
            metadata.dropped_entries,
            if metadata.dropped_entries == 1 { "y was" } else { "ies were" },
        ));
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
            ControlFlow::Call { target, condition } => {
                let taken = condition
                    .map(|condition| condition.evaluate(entry.before.flags))
                    .unwrap_or(true);
                if taken {
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
                result.frames.push(InferredCallFrame8080 {
                    kind: CallKind8080::Restart,
                    call_site: entry.address,
                    target: vector,
                    return_address: sequential,
                    stack_pointer_after_push: entry.after.sp,
                    sequence: entry.sequence,
                });
            }
            ControlFlow::Return { condition } => {
                let taken = condition
                    .map(|condition| condition.evaluate(entry.before.flags))
                    .unwrap_or(true);
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
    use crate::cpu8080::FLAG_Z;
    use crate::trace8080::CpuSnapshot8080;

    fn entry_with_flags(
        sequence: u64,
        address: u16,
        bytes: [u8; 3],
        flags: u8,
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
            before: CpuSnapshot8080 {
                pc: address,
                sp: before_sp,
                flags,
                ..CpuSnapshot8080::default()
            },
            after: CpuSnapshot8080 {
                pc: after_pc,
                sp: after_sp,
                flags,
                ..CpuSnapshot8080::default()
            },
            effects: Vec::new(),
        }
    }

    fn entry(
        sequence: u64,
        address: u16,
        bytes: [u8; 3],
        before_sp: u16,
        after_pc: u16,
        after_sp: u16,
    ) -> InstructionTraceEntry {
        entry_with_flags(sequence, address, bytes, 0, before_sp, after_pc, after_sp)
    }

    fn metadata() -> InstructionTraceMetadata {
        InstructionTraceMetadata {
            generation: 1,
            dropped_entries: 0,
            capacity: 4096,
        }
    }

    #[test]
    fn taken_call_and_matching_ret_leave_empty_stack() {
        let history = vec![
            entry(1, 0x0100, [0xcd, 0x00, 0x02], 0x1000, 0x0200, 0x0ffe),
            entry(2, 0x0200, [0xc9, 0, 0], 0x0ffe, 0x0103, 0x1000),
        ];
        let stack = infer_call_stack_8080(&history, metadata());
        assert!(stack.frames.is_empty());
        assert!(!stack.incomplete);
    }

    #[test]
    fn nested_calls_preserve_live_frames_in_order() {
        let history = vec![
            entry(1, 0x0100, [0xcd, 0x00, 0x02], 0x1000, 0x0200, 0x0ffe),
            entry(2, 0x0200, [0xcd, 0x00, 0x03], 0x0ffe, 0x0300, 0x0ffc),
        ];
        let stack = infer_call_stack_8080(&history, metadata());
        assert_eq!(stack.frames.len(), 2);
        assert_eq!(stack.frames[0].return_address, 0x0103);
        assert_eq!(stack.frames[1].return_address, 0x0203);
        assert_eq!(stack.frames[1].stack_pointer_after_push, 0x0ffc);
    }

    #[test]
    fn conditional_call_uses_before_flags_even_when_target_equals_sequential_pc() {
        // CNZ $0103 at $0100. Z=1 means NOT TAKEN, even though target equals
        // the sequential PC and therefore PC comparison alone is ambiguous.
        let history = vec![entry_with_flags(
            1,
            0x0100,
            [0xc4, 0x03, 0x01],
            FLAG_Z,
            0x1000,
            0x0103,
            0x1000,
        )];
        assert!(infer_call_stack_8080(&history, metadata()).frames.is_empty());
    }

    #[test]
    fn conditional_return_uses_flags_even_when_return_equals_sequential_pc() {
        // CALL creates return $0103. RZ at $0200 is taken with Z=1, and the
        // stacked return is deliberately $0201 (the sequential PC of RZ).
        let history = vec![
            entry(1, 0x0100, [0xcd, 0x00, 0x02], 0x1000, 0x0200, 0x0ffe),
            entry_with_flags(2, 0x0200, [0xc8, 0, 0], FLAG_Z, 0x0ffe, 0x0103, 0x1000),
        ];
        let stack = infer_call_stack_8080(&history, metadata());
        assert!(stack.frames.is_empty());
        assert!(!stack.incomplete);
    }

    #[test]
    fn explicit_eviction_marks_retained_stack_incomplete() {
        let mut meta = metadata();
        meta.dropped_entries = 7;
        let stack = infer_call_stack_8080(&[entry(42, 0x0200, [0x00, 0, 0], 0x1000, 0x0201, 0x1000)], meta);
        assert!(stack.incomplete);
    }
}

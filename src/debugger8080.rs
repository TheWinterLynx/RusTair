use crate::decoder8080::{decode_8080, Condition, ControlFlow, DecodedInstruction};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionAt {
    pub address: u16,
    pub bytes: [u8; 3],
    pub decoded: DecodedInstruction,
}

impl InstructionAt {
    pub fn next_address(&self) -> Option<u16> {
        self.address.checked_add(u16::from(self.decoded.length))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleLoop {
    pub start: u16,
    pub back_edge: u16,
    pub condition: Option<Condition>,
    pub branch_taken_now: bool,
    pub instructions: Vec<InstructionAt>,
}

impl SimpleLoop {
    pub fn exit_description(&self) -> String {
        match self.condition {
            Some(condition) => format!(
                "repeat while {} is true; exit when it becomes false",
                condition.description()
            ),
            None => "unconditional backward jump; no structural exit in this loop".into(),
        }
    }
}

pub fn decode_at<F>(mut read: F, address: u16) -> Option<InstructionAt>
where
    F: FnMut(u16) -> Option<u8>,
{
    let opcode = read(address)?;
    let b1 = read(address.wrapping_add(1)).unwrap_or(0);
    let b2 = read(address.wrapping_add(2)).unwrap_or(0);
    let decoded = decode_8080(opcode, b1, b2);

    if decoded.length >= 2 && read(address.checked_add(1)?).is_none() {
        return None;
    }
    if decoded.length >= 3 && read(address.checked_add(2)?).is_none() {
        return None;
    }

    Some(InstructionAt {
        address,
        bytes: [opcode, b1, b2],
        decoded,
    })
}

fn verify_loop_from<F>(
    read: &mut F,
    start: u16,
    back_edge: u16,
    pc: u16,
    flags: u8,
) -> Option<SimpleLoop>
where
    F: FnMut(u16) -> Option<u8>,
{
    let mut cursor = start;
    let mut pc_is_instruction_boundary = false;
    let mut instructions = Vec::new();

    for _ in 0..256 {
        let instruction = decode_at(&mut *read, cursor)?;
        if cursor == pc {
            pc_is_instruction_boundary = true;
        }

        if cursor == back_edge {
            let (target, condition) = match instruction.decoded.control_flow {
                ControlFlow::Jump { target, condition } if target == start => (target, condition),
                _ => return None,
            };
            debug_assert_eq!(target, start);
            let branch_taken_now = condition.map_or(true, |condition| condition.evaluate(flags));
            instructions.push(instruction);
            return pc_is_instruction_boundary.then_some(SimpleLoop {
                start,
                back_edge,
                condition,
                branch_taken_now,
                instructions,
            });
        }

        // Conservative first version: a loop body is a single straight-line
        // basic block ending in one direct backward JMP/Jcc. This deliberately
        // rejects nested branches, calls, returns and indirect transfers rather
        // than inventing boundaries that are not certain.
        if !matches!(instruction.decoded.control_flow, ControlFlow::Linear) {
            return None;
        }

        let next = instruction.next_address()?;
        if next <= cursor || next > back_edge {
            return None;
        }
        instructions.push(instruction);
        cursor = next;
    }

    None
}

/// Detect a high-confidence simple loop containing the current PC.
///
/// The detector searches forward for a direct backward JMP/Jcc whose target is
/// at or before PC, then proves that decoding linearly from that target lands
/// exactly on the branch and that PC is an instruction boundary in that block.
/// This avoids treating arbitrary operand/data bytes as loop branches.
pub fn detect_simple_backward_loop<F>(
    mut read: F,
    pc: u16,
    flags: u8,
) -> Option<SimpleLoop>
where
    F: FnMut(u16) -> Option<u8>,
{
    const FORWARD_SEARCH_BYTES: u16 = 0x0100;
    let search_end = pc.saturating_add(FORWARD_SEARCH_BYTES);
    let mut best: Option<SimpleLoop> = None;

    let mut candidate = pc;
    loop {
        if let Some(instruction) = decode_at(&mut read, candidate) {
            if let ControlFlow::Jump { target, .. } = instruction.decoded.control_flow {
                if target < candidate && target <= pc {
                    if let Some(found) = verify_loop_from(&mut read, target, candidate, pc, flags) {
                        let replace = best.as_ref().is_none_or(|current| {
                            found.back_edge - found.start < current.back_edge - current.start
                        });
                        if replace {
                            best = Some(found);
                        }
                    }
                }
            }
        }

        if candidate == search_end || candidate == u16::MAX {
            break;
        }
        candidate = candidate.wrapping_add(1);
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu8080::FLAG_Z;

    fn reader(memory: &[u8]) -> impl FnMut(u16) -> Option<u8> + '_ {
        move |address| memory.get(usize::from(address)).copied()
    }

    #[test]
    fn detects_simple_conditional_backward_loop_and_live_branch_state() {
        // 0000: MVI B,03
        // 0002: DCR B
        // 0003: JNZ 0002
        // 0006: HLT
        let memory = [0x06, 0x03, 0x05, 0xc2, 0x02, 0x00, 0x76];
        let found = detect_simple_backward_loop(reader(&memory), 0x0002, 0).unwrap();
        assert_eq!(found.start, 0x0002);
        assert_eq!(found.back_edge, 0x0003);
        assert_eq!(found.condition, Some(Condition::NotZero));
        assert!(found.branch_taken_now);
        assert_eq!(found.instructions.len(), 2);
        assert_eq!(found.instructions[0].decoded.text(), "DCR B");
        assert_eq!(found.instructions[1].decoded.text(), "JNZ $0002");

        let exiting = detect_simple_backward_loop(reader(&memory), 0x0003, FLAG_Z).unwrap();
        assert!(!exiting.branch_taken_now);
    }

    #[test]
    fn detects_unconditional_loop() {
        let memory = [0x00, 0xc3, 0x00, 0x00];
        let found = detect_simple_backward_loop(reader(&memory), 0x0000, 0).unwrap();
        assert_eq!(found.start, 0x0000);
        assert_eq!(found.back_edge, 0x0001);
        assert_eq!(found.condition, None);
        assert!(found.branch_taken_now);
    }

    #[test]
    fn refuses_false_branch_found_inside_an_operand() {
        // At byte 0001 there is C2, but it is the low immediate byte of LXI H.
        // Linear decoding from its claimed target cannot prove a loop ending at
        // that byte, so the detector must not present it as known code.
        let memory = [0x21, 0xc2, 0x00, 0x76];
        assert!(detect_simple_backward_loop(reader(&memory), 0x0000, 0).is_none());
    }

    #[test]
    fn rejects_nested_or_non_linear_body_in_first_conservative_version() {
        // 0000 JNZ 0006 (forward branch inside would-be body)
        // 0003 NOP
        // 0004 JMP 0000
        let memory = [0xc2, 0x06, 0x00, 0x00, 0xc3, 0x00, 0x00];
        assert!(detect_simple_backward_loop(reader(&memory), 0x0003, 0).is_none());
    }
}

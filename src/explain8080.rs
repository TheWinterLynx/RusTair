use crate::backend::Intel8080State;
use crate::decoder8080::{ControlFlow, DecodedInstruction, IoAccess, MemoryAccess};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionExplanation {
    pub summary: String,
    pub reads: String,
    pub writes: String,
    pub flags: String,
    pub memory: String,
    pub io: String,
    pub flow: String,
    pub context: Vec<String>,
}

fn operand(decoded: &DecodedInstruction, index: usize) -> &str {
    decoded.operands.get(index).map(String::as_str).unwrap_or("?")
}

fn summary(decoded: &DecodedInstruction) -> String {
    let a = operand(decoded, 0);
    let b = operand(decoded, 1);
    match decoded.mnemonic {
        "NOP" => "Do nothing. Execution continues with the next instruction.".into(),
        "HLT" => "Halt the 8080. The processor remains halted until reset or a supported interrupt resumes it.".into(),
        "MOV" => format!("Copy {b} into {a}. The source is not modified."),
        "MVI" => format!("Load the immediate byte {b} into {a}."),
        "LXI" => format!("Load the 16-bit immediate value {b} into register pair {a}."),
        "INR" => format!("Increment {a} by one. Carry is preserved."),
        "DCR" => format!("Decrement {a} by one. Carry is preserved."),
        "INX" => format!("Increment 16-bit register pair {a} by one. Flags are not changed."),
        "DCX" => format!("Decrement 16-bit register pair {a} by one. Flags are not changed."),
        "DAD" => format!("Add 16-bit register pair {a} to HL and store the 16-bit result in HL."),
        "STAX" => format!("Store accumulator A into memory addressed by register pair {a}."),
        "LDAX" => format!("Load accumulator A from memory addressed by register pair {a}."),
        "RLC" => "Rotate accumulator A left. Bit 7 moves into bit 0 and Carry.".into(),
        "RRC" => "Rotate accumulator A right. Bit 0 moves into bit 7 and Carry.".into(),
        "RAL" => "Rotate accumulator A left through Carry.".into(),
        "RAR" => "Rotate accumulator A right through Carry.".into(),
        "DAA" => "Decimal-adjust accumulator A after BCD arithmetic.".into(),
        "CMA" => "Complement every bit of accumulator A.".into(),
        "STC" => "Set the Carry flag.".into(),
        "CMC" => "Complement the Carry flag.".into(),
        "SHLD" => format!("Store L at {a} and H at the following memory address."),
        "LHLD" => format!("Load L from {a} and H from the following memory address."),
        "STA" => format!("Store accumulator A at memory address {a}."),
        "LDA" => format!("Load accumulator A from memory address {a}."),
        "ADD" => format!("Add {a} to accumulator A."),
        "ADC" => format!("Add {a} plus Carry to accumulator A."),
        "SUB" => format!("Subtract {a} from accumulator A."),
        "SBB" => format!("Subtract {a} and the Carry/borrow input from accumulator A."),
        "ANA" => format!("Bitwise-AND accumulator A with {a}."),
        "XRA" => format!("Bitwise-XOR accumulator A with {a}."),
        "ORA" => format!("Bitwise-OR accumulator A with {a}."),
        "CMP" => format!("Compare accumulator A with {a} by performing a subtraction only for flags; A is unchanged."),
        "ADI" => format!("Add immediate byte {a} to accumulator A."),
        "ACI" => format!("Add immediate byte {a} plus Carry to accumulator A."),
        "SUI" => format!("Subtract immediate byte {a} from accumulator A."),
        "SBI" => format!("Subtract immediate byte {a} and Carry/borrow from accumulator A."),
        "ANI" => format!("Bitwise-AND accumulator A with immediate byte {a}."),
        "XRI" => format!("Bitwise-XOR accumulator A with immediate byte {a}."),
        "ORI" => format!("Bitwise-OR accumulator A with immediate byte {a}."),
        "CPI" => format!("Compare accumulator A with immediate byte {a}; only flags receive the subtraction result."),
        "PUSH" => format!("Push register pair {a} onto the stack. SP moves down by two bytes."),
        "POP" => format!("Pop two bytes from the stack into {a}. SP moves up by two bytes."),
        "XTHL" => "Exchange L/H with the two bytes at the top of the stack without changing SP.".into(),
        "XCHG" => "Exchange register pairs DE and HL.".into(),
        "SPHL" => "Copy HL into SP.".into(),
        "PCHL" => "Copy HL into PC, transferring execution to the address held in HL.".into(),
        "IN" => format!("Read one byte from I/O port {a} into accumulator A."),
        "OUT" => format!("Write accumulator A to I/O port {a}."),
        "DI" => "Disable maskable interrupts.".into(),
        "EI" => "Enable maskable interrupts after the following instruction, matching Intel 8080 EI delay semantics.".into(),
        // Resolve unconditional opcodes before their conditional mnemonic
        // families. CALL begins with C and RET/RST begin with R, so prefix
        // matching first would teach the wrong semantics.
        "CALL" => format!("Push the return address onto the stack and continue execution at {a}."),
        "RET" => "Pop a return address from the stack into PC.".into(),
        "RST" => format!("Push the return address and call restart vector {a}."),
        mnemonic if mnemonic.starts_with('J') => format!("Evaluate the branch condition and, when satisfied, load PC with {a}."),
        mnemonic if mnemonic.starts_with('C') && mnemonic != "CMC" && mnemonic != "CMA" && mnemonic != "CMP" && mnemonic != "CPI" => {
            format!("Evaluate the call condition and, when satisfied, push the return address then continue at {a}.")
        }
        mnemonic if mnemonic.starts_with('R') && mnemonic != "RLC" && mnemonic != "RRC" && mnemonic != "RAL" && mnemonic != "RAR" => {
            "Evaluate the return condition and, when satisfied, pop the return address from the stack into PC.".into()
        }
        _ => format!("Execute {} using normal Intel 8080 semantics.", decoded.text()),
    }
}

fn register_effects(decoded: &DecodedInstruction) -> (String, String) {
    let a = operand(decoded, 0);
    let b = operand(decoded, 1);
    match decoded.mnemonic {
        "NOP" | "HLT" | "DI" | "EI" => ("none".into(), "none".into()),
        "MOV" => (b.into(), a.into()),
        "MVI" => ("immediate byte".into(), a.into()),
        "LXI" => ("16-bit immediate".into(), a.into()),
        "INR" | "DCR" => (a.into(), a.into()),
        "INX" | "DCX" => (a.into(), a.into()),
        "DAD" => (format!("HL, {a}"), "HL".into()),
        "STAX" => (format!("A, {a}"), "memory".into()),
        "LDAX" => (format!("{a}, memory"), "A".into()),
        "RLC" | "RRC" | "RAL" | "RAR" | "DAA" | "CMA" => ("A".into(), "A".into()),
        "STC" | "CMC" => ("Carry flag".into(), "Carry flag".into()),
        "SHLD" => ("HL".into(), "memory".into()),
        "LHLD" => ("memory".into(), "HL".into()),
        "STA" => ("A".into(), "memory".into()),
        "LDA" => ("memory".into(), "A".into()),
        "ADD" | "ADC" | "SUB" | "SBB" | "ANA" | "XRA" | "ORA" => (format!("A, {a}"), "A".into()),
        "CMP" => (format!("A, {a}"), "flags only".into()),
        "ADI" | "ACI" | "SUI" | "SBI" | "ANI" | "XRI" | "ORI" => ("A, immediate byte".into(), "A".into()),
        "CPI" => ("A, immediate byte".into(), "flags only".into()),
        "PUSH" => (format!("{a}, SP"), "SP, stack memory".into()),
        "POP" => ("SP, stack memory".into(), format!("{a}, SP")),
        "XTHL" => ("HL, SP, stack memory".into(), "HL, stack memory".into()),
        "XCHG" => ("DE, HL".into(), "DE, HL".into()),
        "SPHL" => ("HL".into(), "SP".into()),
        "PCHL" => ("HL".into(), "PC".into()),
        "IN" => ("I/O port".into(), "A".into()),
        "OUT" => ("A".into(), "I/O port".into()),
        "CALL" | "RST" => ("PC, SP".into(), "PC, SP, stack memory".into()),
        "RET" => ("SP, stack memory".into(), "PC, SP".into()),
        mnemonic if mnemonic.starts_with('J') => ("condition flags".into(), "PC when taken".into()),
        mnemonic if mnemonic.starts_with('C') => ("condition flags, PC, SP".into(), "PC/SP/stack when taken".into()),
        mnemonic if mnemonic.starts_with('R') => ("condition flags, SP/stack when taken".into(), "PC/SP when taken".into()),
        _ => ("see instruction semantics".into(), "see instruction semantics".into()),
    }
}

fn memory_text(decoded: &DecodedInstruction) -> String {
    match decoded.memory {
        MemoryAccess::None => "No data-memory access beyond instruction fetch.".into(),
        MemoryAccess::Read => "Reads data memory.".into(),
        MemoryAccess::Write => "Writes data memory.".into(),
        MemoryAccess::ReadWrite => "Reads and then writes data memory.".into(),
        MemoryAccess::StackRead => "Reads the stack through SP.".into(),
        MemoryAccess::StackWrite => "Writes the stack through SP.".into(),
        MemoryAccess::StackReadWrite => "Reads and writes the stack through SP.".into(),
    }
}

fn io_text(decoded: &DecodedInstruction) -> String {
    match decoded.io {
        IoAccess::None => "No I/O port access.".into(),
        IoAccess::Read(port) => format!("Reads I/O port ${port:02X}."),
        IoAccess::Write(port) => format!("Writes I/O port ${port:02X}."),
    }
}

fn flow_text(decoded: &DecodedInstruction, flags: u8) -> String {
    match decoded.control_flow {
        ControlFlow::Jump { condition: Some(condition), .. }
        | ControlFlow::Call { condition: Some(condition), .. }
        | ControlFlow::Return { condition: Some(condition) } => format!(
            "{} Current condition {} is {}.",
            decoded.flow_label(),
            condition.label(),
            if condition.evaluate(flags) { "TRUE / TAKEN" } else { "FALSE / NOT TAKEN" }
        ),
        _ => decoded.flow_label(),
    }
}

pub fn explain_instruction(
    decoded: &DecodedInstruction,
    cpu: Intel8080State,
    memory_at_hl: Option<u8>,
) -> InstructionExplanation {
    let (reads, writes) = register_effects(decoded);
    let mut context = Vec::new();

    if decoded.operands.iter().any(|operand| operand == "M") {
        let hl = cpu.hl();
        context.push(match memory_at_hl {
            Some(value) => format!(
                "M is not a register: M means memory[HL]. Current HL=${hl:04X}, so M=[${hl:04X}]=${value:02X}."
            ),
            None => format!(
                "M is not a register: M means memory[HL]. Current HL=${hl:04X}, but that address is outside installed RAM."
            ),
        });
    }

    if matches!(decoded.control_flow, ControlFlow::IndirectJump) {
        context.push(format!("Current HL=${:04X}; PCHL would therefore continue at that address.", cpu.hl()));
    }

    if matches!(decoded.memory, MemoryAccess::StackRead | MemoryAccess::StackWrite | MemoryAccess::StackReadWrite) {
        context.push(format!("Current SP=${:04X}.", cpu.sp));
    }

    if decoded.undocumented_alias {
        context.push("This byte is an undocumented 8080 alias accepted by the RusTair CPU cores.".into());
    }

    InstructionExplanation {
        summary: summary(decoded),
        reads,
        writes,
        flags: format!("Affected flags: {}.", decoded.flags.label()),
        memory: memory_text(decoded),
        io: io_text(decoded),
        flow: flow_text(decoded, cpu.flags),
        context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder8080::decode_8080;

    #[test]
    fn explains_m_as_memory_at_live_hl() {
        let decoded = decode_8080(0x7e, 0, 0); // MOV A,M
        let cpu = Intel8080State { h: 0x12, l: 0x34, ..Intel8080State::default() };
        let explanation = explain_instruction(&decoded, cpu, Some(0x5a));
        assert!(explanation.summary.contains("Copy M into A"));
        assert!(explanation.context.iter().any(|line| line.contains("HL=$1234")));
        assert!(explanation.context.iter().any(|line| line.contains("$5A")));
    }

    #[test]
    fn conditional_flow_uses_live_flags() {
        let decoded = decode_8080(0xc2, 0x34, 0x12); // JNZ 1234h
        let cpu = Intel8080State { flags: 0, ..Intel8080State::default() };
        let explanation = explain_instruction(&decoded, cpu, None);
        assert!(explanation.flow.contains("TRUE / TAKEN"));
    }

    #[test]
    fn unconditional_call_return_and_restart_are_not_described_as_conditional() {
        let cpu = Intel8080State::default();
        let call = explain_instruction(&decode_8080(0xcd, 0x34, 0x12), cpu, None);
        let ret = explain_instruction(&decode_8080(0xc9, 0, 0), cpu, None);
        let rst = explain_instruction(&decode_8080(0xcf, 0, 0), cpu, None);
        assert!(!call.summary.contains("condition"));
        assert!(!ret.summary.contains("condition"));
        assert!(!rst.summary.contains("condition"));
        assert!(call.summary.contains("return address"));
        assert!(ret.summary.contains("Pop a return address"));
        assert!(rst.summary.contains("restart vector"));
    }
}

use crate::cpu8080::{FLAG_AC, FLAG_C, FLAG_P, FLAG_S, FLAG_Z};

const REG: [&str; 8] = ["B", "C", "D", "E", "H", "L", "M", "A"];
const RP: [&str; 4] = ["B", "D", "H", "SP"];
const RP_PUSH: [&str; 4] = ["B", "D", "H", "PSW"];
const ALU: [&str; 8] = ["ADD", "ADC", "SUB", "SBB", "ANA", "XRA", "ORA", "CMP"];
const COND_RET: [&str; 8] = ["RNZ", "RZ", "RNC", "RC", "RPO", "RPE", "RP", "RM"];
const COND_JMP: [&str; 8] = ["JNZ", "JZ", "JNC", "JC", "JPO", "JPE", "JP", "JM"];
const COND_CALL: [&str; 8] = ["CNZ", "CZ", "CNC", "CC", "CPO", "CPE", "CP", "CM"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Condition {
    NotZero,
    Zero,
    NoCarry,
    Carry,
    ParityOdd,
    ParityEven,
    Positive,
    Minus,
}

impl Condition {
    pub const fn from_code(code: u8) -> Self {
        match code & 7 {
            0 => Self::NotZero,
            1 => Self::Zero,
            2 => Self::NoCarry,
            3 => Self::Carry,
            4 => Self::ParityOdd,
            5 => Self::ParityEven,
            6 => Self::Positive,
            _ => Self::Minus,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::NotZero => "NZ",
            Self::Zero => "Z",
            Self::NoCarry => "NC",
            Self::Carry => "C",
            Self::ParityOdd => "PO",
            Self::ParityEven => "PE",
            Self::Positive => "P",
            Self::Minus => "M",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::NotZero => "Zero flag is clear",
            Self::Zero => "Zero flag is set",
            Self::NoCarry => "Carry flag is clear",
            Self::Carry => "Carry flag is set",
            Self::ParityOdd => "Parity flag is clear",
            Self::ParityEven => "Parity flag is set",
            Self::Positive => "Sign flag is clear",
            Self::Minus => "Sign flag is set",
        }
    }

    pub const fn evaluate(self, flags: u8) -> bool {
        match self {
            Self::NotZero => flags & FLAG_Z == 0,
            Self::Zero => flags & FLAG_Z != 0,
            Self::NoCarry => flags & FLAG_C == 0,
            Self::Carry => flags & FLAG_C != 0,
            Self::ParityOdd => flags & FLAG_P == 0,
            Self::ParityEven => flags & FLAG_P != 0,
            Self::Positive => flags & FLAG_S == 0,
            Self::Minus => flags & FLAG_S != 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlFlow {
    Linear,
    Halt,
    Jump {
        target: u16,
        condition: Option<Condition>,
    },
    Call {
        target: u16,
        condition: Option<Condition>,
    },
    Return {
        condition: Option<Condition>,
    },
    Restart {
        vector: u16,
    },
    IndirectJump,
}

impl ControlFlow {
    pub const fn direct_target(self) -> Option<u16> {
        match self {
            Self::Jump { target, .. } | Self::Call { target, .. } => Some(target),
            Self::Restart { vector } => Some(vector),
            _ => None,
        }
    }

    pub const fn condition(self) -> Option<Condition> {
        match self {
            Self::Jump { condition, .. }
            | Self::Call { condition, .. }
            | Self::Return { condition } => condition,
            _ => None,
        }
    }

    pub const fn is_branch(self) -> bool {
        matches!(self, Self::Jump { .. } | Self::IndirectJump)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryAccess {
    None,
    Read,
    Write,
    ReadWrite,
    StackRead,
    StackWrite,
    StackReadWrite,
}

impl MemoryAccess {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "read + write",
            Self::StackRead => "stack read",
            Self::StackWrite => "stack write",
            Self::StackReadWrite => "stack read + write",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoAccess {
    None,
    Read(u8),
    Write(u8),
}

impl IoAccess {
    pub fn label(self) -> String {
        match self {
            Self::None => "none".into(),
            Self::Read(port) => format!("IN from port ${port:02X}"),
            Self::Write(port) => format!("OUT to port ${port:02X}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    pub base_t_states: u8,
    pub taken_t_states: Option<u8>,
}

impl Timing {
    pub const fn fixed(t_states: u8) -> Self {
        Self {
            base_t_states: t_states,
            taken_t_states: None,
        }
    }

    pub const fn conditional(not_taken: u8, taken: u8) -> Self {
        Self {
            base_t_states: not_taken,
            taken_t_states: Some(taken),
        }
    }

    pub fn label(self) -> String {
        match self.taken_t_states {
            Some(taken) if taken != self.base_t_states => {
                format!("{} T not taken / {taken} T taken", self.base_t_states)
            }
            _ => format!("{} T", self.base_t_states),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlagEffects {
    pub mask: u8,
}

impl FlagEffects {
    pub const NONE: Self = Self { mask: 0 };
    pub const SZP_AC: Self = Self {
        mask: FLAG_S | FLAG_Z | FLAG_P | FLAG_AC,
    };
    pub const SZP_AC_C: Self = Self {
        mask: FLAG_S | FLAG_Z | FLAG_P | FLAG_AC | FLAG_C,
    };
    pub const CARRY: Self = Self { mask: FLAG_C };

    pub fn label(self) -> String {
        if self.mask == 0 {
            return "none".into();
        }
        let mut names = Vec::with_capacity(5);
        if self.mask & FLAG_S != 0 {
            names.push("S");
        }
        if self.mask & FLAG_Z != 0 {
            names.push("Z");
        }
        if self.mask & FLAG_AC != 0 {
            names.push("AC");
        }
        if self.mask & FLAG_P != 0 {
            names.push("P");
        }
        if self.mask & FLAG_C != 0 {
            names.push("C");
        }
        names.join(" ")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedInstruction {
    pub opcode: u8,
    pub mnemonic: &'static str,
    pub operands: Vec<String>,
    pub length: u8,
    pub immediate8: Option<u8>,
    pub immediate16: Option<u16>,
    pub address_target: Option<u16>,
    pub flags: FlagEffects,
    pub timing: Timing,
    pub memory: MemoryAccess,
    pub io: IoAccess,
    pub control_flow: ControlFlow,
    pub undocumented_alias: bool,
}

impl DecodedInstruction {
    pub fn text(&self) -> String {
        if self.operands.is_empty() {
            self.mnemonic.into()
        } else {
            format!("{} {}", self.mnemonic, self.operands.join(","))
        }
    }

    pub fn bytes_text(&self, bytes: [u8; 3]) -> String {
        match self.length {
            1 => format!("{:02X}", bytes[0]),
            2 => format!("{:02X} {:02X}", bytes[0], bytes[1]),
            _ => format!("{:02X} {:02X} {:02X}", bytes[0], bytes[1], bytes[2]),
        }
    }

    /// Concise debugger-facing memory description. Conditional CALL/RET carry
    /// stack metadata because the taken path does touch SP, but the UI must not
    /// imply that transfer happens on the not-taken path.
    pub fn memory_label(&self) -> String {
        match (self.memory, self.control_flow) {
            (
                MemoryAccess::StackWrite,
                ControlFlow::Call {
                    condition: Some(_),
                    ..
                },
            ) => "stack write if taken".into(),
            (
                MemoryAccess::StackRead,
                ControlFlow::Return {
                    condition: Some(_),
                },
            ) => "stack read if taken".into(),
            _ => self.memory.label().into(),
        }
    }

    pub fn flow_label(&self) -> String {
        match self.control_flow {
            ControlFlow::Linear => "continues to the next instruction".into(),
            ControlFlow::Halt => "halts the CPU until an interrupt/reset".into(),
            ControlFlow::Jump {
                target,
                condition: None,
            } => format!("jumps to ${target:04X}"),
            ControlFlow::Jump {
                target,
                condition: Some(condition),
            } => format!(
                "jumps to ${target:04X} when {} ({})",
                condition.label(),
                condition.description()
            ),
            ControlFlow::Call {
                target,
                condition: None,
            } => format!("calls ${target:04X}"),
            ControlFlow::Call {
                target,
                condition: Some(condition),
            } => format!(
                "calls ${target:04X} when {} ({})",
                condition.label(),
                condition.description()
            ),
            ControlFlow::Return { condition: None } => "returns through the stack".into(),
            ControlFlow::Return {
                condition: Some(condition),
            } => format!(
                "returns when {} ({})",
                condition.label(),
                condition.description()
            ),
            ControlFlow::Restart { vector } => format!("RST call to ${vector:04X}"),
            ControlFlow::IndirectJump => "jumps indirectly to the address in HL".into(),
        }
    }
}

fn word(lo: u8, hi: u8) -> u16 {
    u16::from_le_bytes([lo, hi])
}

fn operand_register(code: u8) -> String {
    REG[(code & 7) as usize].into()
}

fn operand_pair(code: u8) -> String {
    RP[(code & 3) as usize].into()
}

fn operand_push_pair(code: u8) -> String {
    RP_PUSH[(code & 3) as usize].into()
}

fn decoded(
    opcode: u8,
    mnemonic: &'static str,
    operands: Vec<String>,
    length: u8,
    immediate8: Option<u8>,
    immediate16: Option<u16>,
    address_target: Option<u16>,
    flags: FlagEffects,
    timing: Timing,
    memory: MemoryAccess,
    io: IoAccess,
    control_flow: ControlFlow,
    undocumented_alias: bool,
) -> DecodedInstruction {
    DecodedInstruction {
        opcode,
        mnemonic,
        operands,
        length,
        immediate8,
        immediate16,
        address_target,
        flags,
        timing,
        memory,
        io,
        control_flow,
        undocumented_alias,
    }
}

pub fn decode_8080(opcode: u8, b1: u8, b2: u8) -> DecodedInstruction {
    if (0x40..=0x7f).contains(&opcode) {
        if opcode == 0x76 {
            return decoded(
                opcode,
                "HLT",
                vec![],
                1,
                None,
                None,
                None,
                FlagEffects::NONE,
                Timing::fixed(7),
                MemoryAccess::None,
                IoAccess::None,
                ControlFlow::Halt,
                false,
            );
        }
        let dst = (opcode >> 3) & 7;
        let src = opcode & 7;
        let memory = match (dst == 6, src == 6) {
            (true, false) => MemoryAccess::Write,
            (false, true) => MemoryAccess::Read,
            _ => MemoryAccess::None,
        };
        return decoded(
            opcode,
            "MOV",
            vec![operand_register(dst), operand_register(src)],
            1,
            None,
            None,
            None,
            FlagEffects::NONE,
            Timing::fixed(if dst == 6 || src == 6 { 7 } else { 5 }),
            memory,
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if (0x80..=0xbf).contains(&opcode) {
        let alu = (opcode >> 3) & 7;
        let src = opcode & 7;
        return decoded(
            opcode,
            ALU[alu as usize],
            vec![operand_register(src)],
            1,
            None,
            None,
            None,
            FlagEffects::SZP_AC_C,
            Timing::fixed(if src == 6 { 7 } else { 4 }),
            if src == 6 {
                MemoryAccess::Read
            } else {
                MemoryAccess::None
            },
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xc7 == 0x04 {
        let reg = (opcode >> 3) & 7;
        return decoded(
            opcode,
            "INR",
            vec![operand_register(reg)],
            1,
            None,
            None,
            None,
            FlagEffects::SZP_AC,
            Timing::fixed(if reg == 6 { 10 } else { 5 }),
            if reg == 6 {
                MemoryAccess::ReadWrite
            } else {
                MemoryAccess::None
            },
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xc7 == 0x05 {
        let reg = (opcode >> 3) & 7;
        return decoded(
            opcode,
            "DCR",
            vec![operand_register(reg)],
            1,
            None,
            None,
            None,
            FlagEffects::SZP_AC,
            Timing::fixed(if reg == 6 { 10 } else { 5 }),
            if reg == 6 {
                MemoryAccess::ReadWrite
            } else {
                MemoryAccess::None
            },
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xc7 == 0x06 {
        let reg = (opcode >> 3) & 7;
        return decoded(
            opcode,
            "MVI",
            vec![operand_register(reg), format!("${b1:02X}")],
            2,
            Some(b1),
            None,
            None,
            FlagEffects::NONE,
            Timing::fixed(if reg == 6 { 10 } else { 7 }),
            if reg == 6 {
                MemoryAccess::Write
            } else {
                MemoryAccess::None
            },
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xcf == 0x01 {
        let pair = (opcode >> 4) & 3;
        let value = word(b1, b2);
        return decoded(
            opcode,
            "LXI",
            vec![operand_pair(pair), format!("${value:04X}")],
            3,
            None,
            Some(value),
            None,
            FlagEffects::NONE,
            Timing::fixed(10),
            MemoryAccess::None,
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xcf == 0x03 {
        return decoded(
            opcode,
            "INX",
            vec![operand_pair((opcode >> 4) & 3)],
            1,
            None,
            None,
            None,
            FlagEffects::NONE,
            Timing::fixed(5),
            MemoryAccess::None,
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xcf == 0x0b {
        return decoded(
            opcode,
            "DCX",
            vec![operand_pair((opcode >> 4) & 3)],
            1,
            None,
            None,
            None,
            FlagEffects::NONE,
            Timing::fixed(5),
            MemoryAccess::None,
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xcf == 0x09 {
        return decoded(
            opcode,
            "DAD",
            vec![operand_pair((opcode >> 4) & 3)],
            1,
            None,
            None,
            None,
            FlagEffects::CARRY,
            Timing::fixed(10),
            MemoryAccess::None,
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xc7 == 0xc0 {
        let condition = Condition::from_code((opcode >> 3) & 7);
        return decoded(
            opcode,
            COND_RET[((opcode >> 3) & 7) as usize],
            vec![],
            1,
            None,
            None,
            None,
            FlagEffects::NONE,
            Timing::conditional(5, 11),
            MemoryAccess::StackRead,
            IoAccess::None,
            ControlFlow::Return {
                condition: Some(condition),
            },
            false,
        );
    }

    if opcode & 0xc7 == 0xc2 {
        let condition = Condition::from_code((opcode >> 3) & 7);
        let target = word(b1, b2);
        return decoded(
            opcode,
            COND_JMP[((opcode >> 3) & 7) as usize],
            vec![format!("${target:04X}")],
            3,
            None,
            Some(target),
            Some(target),
            FlagEffects::NONE,
            Timing::conditional(10, 10),
            MemoryAccess::None,
            IoAccess::None,
            ControlFlow::Jump {
                target,
                condition: Some(condition),
            },
            false,
        );
    }

    if opcode & 0xc7 == 0xc4 {
        let condition = Condition::from_code((opcode >> 3) & 7);
        let target = word(b1, b2);
        return decoded(
            opcode,
            COND_CALL[((opcode >> 3) & 7) as usize],
            vec![format!("${target:04X}")],
            3,
            None,
            Some(target),
            Some(target),
            FlagEffects::NONE,
            Timing::conditional(11, 17),
            MemoryAccess::StackWrite,
            IoAccess::None,
            ControlFlow::Call {
                target,
                condition: Some(condition),
            },
            false,
        );
    }

    if opcode & 0xcf == 0xc1 {
        let pair = (opcode >> 4) & 3;
        return decoded(
            opcode,
            "POP",
            vec![operand_push_pair(pair)],
            1,
            None,
            None,
            None,
            if pair == 3 {
                FlagEffects::SZP_AC_C
            } else {
                FlagEffects::NONE
            },
            Timing::fixed(10),
            MemoryAccess::StackRead,
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xcf == 0xc5 {
        return decoded(
            opcode,
            "PUSH",
            vec![operand_push_pair((opcode >> 4) & 3)],
            1,
            None,
            None,
            None,
            FlagEffects::NONE,
            Timing::fixed(11),
            MemoryAccess::StackWrite,
            IoAccess::None,
            ControlFlow::Linear,
            false,
        );
    }

    if opcode & 0xc7 == 0xc7 {
        let vector = u16::from(opcode & 0x38);
        return decoded(
            opcode,
            "RST",
            vec![format!("{}", (opcode >> 3) & 7)],
            1,
            None,
            None,
            Some(vector),
            FlagEffects::NONE,
            Timing::fixed(11),
            MemoryAccess::StackWrite,
            IoAccess::None,
            ControlFlow::Restart { vector },
            false,
        );
    }

    let value = word(b1, b2);
    match opcode {
        0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => decoded(
            opcode,
            "NOP",
            vec![],
            1,
            None,
            None,
            None,
            FlagEffects::NONE,
            Timing::fixed(4),
            MemoryAccess::None,
            IoAccess::None,
            ControlFlow::Linear,
            opcode != 0x00,
        ),
        0x02 => decoded(opcode, "STAX", vec!["B".into()], 1, None, None, None, FlagEffects::NONE, Timing::fixed(7), MemoryAccess::Write, IoAccess::None, ControlFlow::Linear, false),
        0x07 => decoded(opcode, "RLC", vec![], 1, None, None, None, FlagEffects::CARRY, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0x0a => decoded(opcode, "LDAX", vec!["B".into()], 1, None, None, None, FlagEffects::NONE, Timing::fixed(7), MemoryAccess::Read, IoAccess::None, ControlFlow::Linear, false),
        0x0f => decoded(opcode, "RRC", vec![], 1, None, None, None, FlagEffects::CARRY, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0x12 => decoded(opcode, "STAX", vec!["D".into()], 1, None, None, None, FlagEffects::NONE, Timing::fixed(7), MemoryAccess::Write, IoAccess::None, ControlFlow::Linear, false),
        0x17 => decoded(opcode, "RAL", vec![], 1, None, None, None, FlagEffects::CARRY, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0x1a => decoded(opcode, "LDAX", vec!["D".into()], 1, None, None, None, FlagEffects::NONE, Timing::fixed(7), MemoryAccess::Read, IoAccess::None, ControlFlow::Linear, false),
        0x1f => decoded(opcode, "RAR", vec![], 1, None, None, None, FlagEffects::CARRY, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0x22 => decoded(opcode, "SHLD", vec![format!("${value:04X}")], 3, None, Some(value), Some(value), FlagEffects::NONE, Timing::fixed(16), MemoryAccess::Write, IoAccess::None, ControlFlow::Linear, false),
        0x27 => decoded(opcode, "DAA", vec![], 1, None, None, None, FlagEffects::SZP_AC_C, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0x2a => decoded(opcode, "LHLD", vec![format!("${value:04X}")], 3, None, Some(value), Some(value), FlagEffects::NONE, Timing::fixed(16), MemoryAccess::Read, IoAccess::None, ControlFlow::Linear, false),
        0x2f => decoded(opcode, "CMA", vec![], 1, None, None, None, FlagEffects::NONE, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0x32 => decoded(opcode, "STA", vec![format!("${value:04X}")], 3, None, Some(value), Some(value), FlagEffects::NONE, Timing::fixed(13), MemoryAccess::Write, IoAccess::None, ControlFlow::Linear, false),
        0x37 => decoded(opcode, "STC", vec![], 1, None, None, None, FlagEffects::CARRY, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0x3a => decoded(opcode, "LDA", vec![format!("${value:04X}")], 3, None, Some(value), Some(value), FlagEffects::NONE, Timing::fixed(13), MemoryAccess::Read, IoAccess::None, ControlFlow::Linear, false),
        0x3f => decoded(opcode, "CMC", vec![], 1, None, None, None, FlagEffects::CARRY, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xc3 | 0xcb => decoded(opcode, "JMP", vec![format!("${value:04X}")], 3, None, Some(value), Some(value), FlagEffects::NONE, Timing::fixed(10), MemoryAccess::None, IoAccess::None, ControlFlow::Jump { target: value, condition: None }, opcode == 0xcb),
        0xc6 => decoded(opcode, "ADI", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::SZP_AC_C, Timing::fixed(7), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xc9 | 0xd9 => decoded(opcode, "RET", vec![], 1, None, None, None, FlagEffects::NONE, Timing::fixed(10), MemoryAccess::StackRead, IoAccess::None, ControlFlow::Return { condition: None }, opcode == 0xd9),
        0xcd | 0xdd | 0xed | 0xfd => decoded(opcode, "CALL", vec![format!("${value:04X}")], 3, None, Some(value), Some(value), FlagEffects::NONE, Timing::fixed(17), MemoryAccess::StackWrite, IoAccess::None, ControlFlow::Call { target: value, condition: None }, opcode != 0xcd),
        0xce => decoded(opcode, "ACI", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::SZP_AC_C, Timing::fixed(7), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xd3 => decoded(opcode, "OUT", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::NONE, Timing::fixed(10), MemoryAccess::None, IoAccess::Write(b1), ControlFlow::Linear, false),
        0xd6 => decoded(opcode, "SUI", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::SZP_AC_C, Timing::fixed(7), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xdb => decoded(opcode, "IN", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::NONE, Timing::fixed(10), MemoryAccess::None, IoAccess::Read(b1), ControlFlow::Linear, false),
        0xde => decoded(opcode, "SBI", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::SZP_AC_C, Timing::fixed(7), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xe3 => decoded(opcode, "XTHL", vec![], 1, None, None, None, FlagEffects::NONE, Timing::fixed(18), MemoryAccess::StackReadWrite, IoAccess::None, ControlFlow::Linear, false),
        0xe6 => decoded(opcode, "ANI", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::SZP_AC_C, Timing::fixed(7), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xe9 => decoded(opcode, "PCHL", vec![], 1, None, None, None, FlagEffects::NONE, Timing::fixed(5), MemoryAccess::None, IoAccess::None, ControlFlow::IndirectJump, false),
        0xeb => decoded(opcode, "XCHG", vec![], 1, None, None, None, FlagEffects::NONE, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xee => decoded(opcode, "XRI", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::SZP_AC_C, Timing::fixed(7), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xf3 => decoded(opcode, "DI", vec![], 1, None, None, None, FlagEffects::NONE, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xf6 => decoded(opcode, "ORI", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::SZP_AC_C, Timing::fixed(7), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xf9 => decoded(opcode, "SPHL", vec![], 1, None, None, None, FlagEffects::NONE, Timing::fixed(5), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xfb => decoded(opcode, "EI", vec![], 1, None, None, None, FlagEffects::NONE, Timing::fixed(4), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        0xfe => decoded(opcode, "CPI", vec![format!("${b1:02X}")], 2, Some(b1), None, None, FlagEffects::SZP_AC_C, Timing::fixed(7), MemoryAccess::None, IoAccess::None, ControlFlow::Linear, false),
        _ => decoded(
            opcode,
            "DB",
            vec![format!("${opcode:02X}")],
            1,
            None,
            None,
            None,
            FlagEffects::NONE,
            Timing::fixed(4),
            MemoryAccess::None,
            IoAccess::None,
            ControlFlow::Linear,
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_opcode_has_structured_metadata() {
        for opcode in 0u8..=u8::MAX {
            let decoded = decode_8080(opcode, 0x34, 0x12);
            assert!((1..=3).contains(&decoded.length), "opcode {opcode:02X}");
            assert!(!decoded.mnemonic.is_empty(), "opcode {opcode:02X}");
            assert!(!decoded.text().is_empty(), "opcode {opcode:02X}");
            assert!(decoded.timing.base_t_states > 0, "opcode {opcode:02X}");
        }
    }

    #[test]
    fn undocumented_8080_aliases_are_explicit() {
        for opcode in [0x08, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0xcb, 0xd9, 0xdd, 0xed, 0xfd] {
            assert!(decode_8080(opcode, 0x34, 0x12).undocumented_alias, "opcode {opcode:02X}");
        }
        assert!(!decode_8080(0x00, 0, 0).undocumented_alias);
        assert!(!decode_8080(0xc3, 0x34, 0x12).undocumented_alias);
        assert!(!decode_8080(0xc9, 0, 0).undocumented_alias);
        assert!(!decode_8080(0xcd, 0x34, 0x12).undocumented_alias);
    }

    #[test]
    fn branch_metadata_exposes_targets_and_live_conditions() {
        let jnz = decode_8080(0xc2, 0x34, 0x12);
        assert_eq!(jnz.text(), "JNZ $1234");
        assert_eq!(jnz.address_target, Some(0x1234));
        assert_eq!(
            jnz.control_flow,
            ControlFlow::Jump {
                target: 0x1234,
                condition: Some(Condition::NotZero),
            }
        );
        assert!(Condition::NotZero.evaluate(0));
        assert!(!Condition::NotZero.evaluate(FLAG_Z));
    }

    #[test]
    fn pop_psw_reports_restored_condition_flags() {
        let pop_h = decode_8080(0xe1, 0, 0);
        let pop_psw = decode_8080(0xf1, 0, 0);
        assert_eq!(pop_h.flags, FlagEffects::NONE);
        assert_eq!(pop_psw.text(), "POP PSW");
        assert_eq!(pop_psw.flags, FlagEffects::SZP_AC_C);
    }

    #[test]
    fn conditional_stack_metadata_does_not_imply_an_unconditional_transfer() {
        let call = decode_8080(0xc4, 0x00, 0x20);
        let ret = decode_8080(0xc0, 0, 0);
        assert_eq!(call.memory, MemoryAccess::StackWrite);
        assert_eq!(call.memory_label(), "stack write if taken");
        assert_eq!(ret.memory, MemoryAccess::StackRead);
        assert_eq!(ret.memory_label(), "stack read if taken");
    }

    #[test]
    fn special_transfer_timings_match_the_8080_cores() {
        assert_eq!(decode_8080(0xeb, 0, 0).timing, Timing::fixed(4)); // XCHG
        assert_eq!(decode_8080(0xe3, 0, 0).timing, Timing::fixed(18)); // XTHL
        assert_eq!(decode_8080(0xe9, 0, 0).timing, Timing::fixed(5)); // PCHL
        assert_eq!(decode_8080(0xf9, 0, 0).timing, Timing::fixed(5)); // SPHL
    }

    #[test]
    fn memory_io_flags_and_timing_are_not_inferred_from_text() {
        let inr_m = decode_8080(0x34, 0, 0);
        assert_eq!(inr_m.memory, MemoryAccess::ReadWrite);
        assert_eq!(inr_m.flags, FlagEffects::SZP_AC);
        assert_eq!(inr_m.timing, Timing::fixed(10));

        let input = decode_8080(0xdb, 0x11, 0);
        assert_eq!(input.io, IoAccess::Read(0x11));
        assert_eq!(input.timing, Timing::fixed(10));

        let call = decode_8080(0xc4, 0x00, 0x20);
        assert_eq!(call.memory, MemoryAccess::StackWrite);
        assert_eq!(call.timing, Timing::conditional(11, 17));
    }
}

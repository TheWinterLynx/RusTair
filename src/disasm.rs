pub fn instruction_len(op: u8) -> u8 {
    match op {
        0x01|0x11|0x21|0x31|0x22|0x2a|0x32|0x3a|
        0xc2|0xc3|0xc4|0xca|0xcc|0xcd|0xd2|0xd4|0xda|0xdc|
        0xe2|0xe4|0xea|0xec|0xf2|0xf4|0xfa|0xfc => 3,
        0x06|0x0e|0x16|0x1e|0x26|0x2e|0x36|0x3e|
        0xc6|0xce|0xd3|0xd6|0xdb|0xde|0xe6|0xee|0xf6|0xfe => 2,
        _ => 1,
    }
}

fn reg(code: u8) -> &'static str {
    ["B","C","D","E","H","L","M","A"][(code & 7) as usize]
}

fn rp(code: u8) -> &'static str {
    ["B","D","H","SP"][(code & 3) as usize]
}

fn rp_push(code: u8) -> &'static str {
    ["B","D","H","PSW"][(code & 3) as usize]
}

fn cond(code: u8) -> &'static str {
    ["NZ","Z","NC","C","PO","PE","P","M"][(code & 7) as usize]
}

pub fn disassemble(op: u8, b1: u8, b2: u8) -> String {
    let d8 = format!("${b1:02X}");
    let d16 = format!("${:04X}", u16::from_le_bytes([b1,b2]));

    // MOV matrix and HLT.
    if (0x40..=0x7f).contains(&op) {
        if op == 0x76 { return "HLT".into(); }
        return format!("MOV {},{}", reg(op >> 3), reg(op));
    }
    // Register ALU matrix.
    if (0x80..=0xbf).contains(&op) {
        let names=["ADD","ADC","SUB","SBB","ANA","XRA","ORA","CMP"];
        return format!("{} {}", names[((op>>3)&7) as usize], reg(op));
    }

    match op {
        0x00|0x08|0x10|0x18|0x20|0x28|0x30|0x38 => "NOP".into(),
        0x07=>"RLC".into(), 0x0f=>"RRC".into(), 0x17=>"RAL".into(), 0x1f=>"RAR".into(),
        0x27=>"DAA".into(), 0x2f=>"CMA".into(), 0x37=>"STC".into(), 0x3f=>"CMC".into(),

        x if x&0xcf==0x01 => format!("LXI {},{}",rp((x>>4)&3),d16),
        x if x&0xcf==0x03 => format!("INX {}",rp((x>>4)&3)),
        x if x&0xcf==0x0b => format!("DCX {}",rp((x>>4)&3)),
        x if x&0xcf==0x09 => format!("DAD {}",rp((x>>4)&3)),
        x if x&0xc7==0x04 => format!("INR {}",reg(x>>3)),
        x if x&0xc7==0x05 => format!("DCR {}",reg(x>>3)),
        x if x&0xc7==0x06 => format!("MVI {},{}",reg(x>>3),d8),

        0x02=>"STAX B".into(), 0x12=>"STAX D".into(),
        0x0a=>"LDAX B".into(), 0x1a=>"LDAX D".into(),
        0x22=>format!("SHLD {d16}"), 0x2a=>format!("LHLD {d16}"),
        0x32=>format!("STA {d16}"), 0x3a=>format!("LDA {d16}"),

        x if x&0xc7==0xc0 => format!("R{}",cond((x>>3)&7)),
        x if x&0xc7==0xc2 => format!("J{} {d16}",cond((x>>3)&7)),
        x if x&0xc7==0xc4 => format!("C{} {d16}",cond((x>>3)&7)),
        x if x&0xcf==0xc1 => format!("POP {}",rp_push((x>>4)&3)),
        x if x&0xcf==0xc5 => format!("PUSH {}",rp_push((x>>4)&3)),
        x if x&0xc7==0xc7 => format!("RST {}",(x>>3)&7),

        0xc3|0xcb=>format!("JMP {d16}"),
        0xc9|0xd9=>"RET".into(),
        0xcd|0xdd|0xed|0xfd=>format!("CALL {d16}"),
        0xc6=>format!("ADI {d8}"), 0xce=>format!("ACI {d8}"),
        0xd6=>format!("SUI {d8}"), 0xde=>format!("SBI {d8}"),
        0xe6=>format!("ANI {d8}"), 0xee=>format!("XRI {d8}"),
        0xf6=>format!("ORI {d8}"), 0xfe=>format!("CPI {d8}"),
        0xd3=>format!("OUT {d8}"), 0xdb=>format!("IN {d8}"),
        0xe3=>"XTHL".into(), 0xe9=>"PCHL".into(), 0xeb=>"XCHG".into(),
        0xf3=>"DI".into(), 0xf9=>"SPHL".into(), 0xfb=>"EI".into(),
        // 8080 treats these undocumented encodings as NOPs; keeping a label
        // makes the inspector useful while retaining silicon behaviour.
        _=>format!("DB ${op:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn decodes_common_altair_instructions(){
        assert_eq!(disassemble(0x3e,0x8c,0),"MVI A,$8C");
        assert_eq!(disassemble(0xc3,0x34,0x12),"JMP $1234");
        assert_eq!(disassemble(0x76,0,0),"HLT");
        assert_eq!(instruction_len(0x21),3);
    }
}

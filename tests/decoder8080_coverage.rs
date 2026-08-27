use rustair::cpu8080::{Bus, Cpu8080};
use rustair::decoder8080::{decode_8080, ControlFlow, IoAccess, MemoryAccess};

#[test]
fn all_256_8080_opcode_values_decode_as_instructions_or_known_aliases() {
    for opcode in 0u8..=u8::MAX {
        let decoded = decode_8080(opcode, 0x34, 0x12);
        assert_ne!(
            decoded.mnemonic,
            "DB",
            "opcode {opcode:02X} fell through the structured 8080 decoder"
        );
        assert!((1..=3).contains(&decoded.length), "opcode {opcode:02X}");
        assert!(decoded.timing.base_t_states > 0, "opcode {opcode:02X}");
    }
}

#[test]
fn representative_metadata_is_structural_not_text_only() {
    let mov_m_a = decode_8080(0x77, 0, 0);
    assert_eq!(mov_m_a.text(), "MOV M,A");
    assert_eq!(mov_m_a.memory, MemoryAccess::Write);

    let input = decode_8080(0xdb, 0x11, 0);
    assert_eq!(input.io, IoAccess::Read(0x11));

    let loop_branch = decode_8080(0xc2, 0x00, 0x01);
    assert!(matches!(
        loop_branch.control_flow,
        ControlFlow::Jump {
            target: 0x0100,
            condition: Some(_)
        }
    ));
}

struct TimingBus {
    memory: [u8; 65536],
}

impl Default for TimingBus {
    fn default() -> Self {
        Self { memory: [0; 65536] }
    }
}

impl Bus for TimingBus {
    fn read(&mut self, address: u16) -> u8 {
        self.memory[address as usize]
    }

    fn write(&mut self, address: u16, value: u8) {
        self.memory[address as usize] = value;
    }
}

fn timing_matches(actual: u32, decoded: &rustair::decoder8080::DecodedInstruction) -> bool {
    actual == u32::from(decoded.timing.base_t_states)
        || decoded
            .timing
            .taken_t_states
            .is_some_and(|taken| actual == u32::from(taken))
}

#[test]
fn decoder_timing_matches_validated_fast_core_for_all_256_opcodes() {
    // The fast and cycle cores already have a 256-opcode differential test.
    // This third comparison ties the debugger/explainer metadata to that same
    // execution reality so timing cannot silently drift from both CPU cores.
    // Two flag states exercise both polarities of every conditional family.
    let flag_seeds = [0x02, 0xd7];

    for opcode in 0u8..=u8::MAX {
        for flags in flag_seeds {
            let mut bus = TimingBus::default();
            bus.memory[0] = opcode;
            bus.memory[1] = 0x34;
            bus.memory[2] = 0x12;
            // Direct data, register-indirect data and a readable stack keep all
            // instruction families deterministic without changing their timing.
            bus.memory[0x1234] = 0x5a;
            bus.memory[0x1235] = 0xa5;
            bus.memory[0x3000] = 0x69;
            bus.memory[0x4000] = 0x78;
            bus.memory[0x4001] = 0x56;

            let mut cpu = Cpu8080::new();
            cpu.a = 0x96;
            cpu.b = 0x30;
            cpu.c = 0x00;
            cpu.d = 0x30;
            cpu.e = 0x00;
            cpu.h = 0x30;
            cpu.l = 0x00;
            cpu.f = flags;
            cpu.pc = 0;
            cpu.sp = 0x4000;

            let actual = cpu.step(&mut bus);
            let decoded = decode_8080(opcode, 0x34, 0x12);
            assert!(
                timing_matches(actual, &decoded),
                "opcode {opcode:02X}, flags {flags:02X}: core executed {actual}T but decoder advertises {}",
                decoded.timing.label(),
            );
        }
    }
}

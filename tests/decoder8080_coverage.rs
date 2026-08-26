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

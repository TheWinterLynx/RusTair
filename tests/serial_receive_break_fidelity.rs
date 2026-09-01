use std::time::Duration;

use rustair::backend::{BackendHost, BackendSerialPort, EmulationEngine};
use rustair::config::{SerialBoard, SioHardwareConfig, SioInterface};

#[test]
fn both_engines_deliver_asr_break_to_88_sio_as_continuous_space_with_framing_error() {
    for engine in EmulationEngine::ALL {
        let mut host = BackendHost::from_engine(engine).expect("built-in Rust 8080 engine");
        host.configure_serial_board(SerialBoard::Sio88);
        host.configure_sio_hardware(SioHardwareConfig {
            interface: SioInterface::TtyC,
            ..SioHardwareConfig::default()
        });
        host.power(true);
        host.front_panel_reset();

        assert!(host.serial_set_receive_break(BackendSerialPort::Port0, true));
        assert!(!host.serial_rx_line_idle(BackendSerialPort::Port0));
        assert_eq!(
            host.sio_logical_lines().expect("88-SIO lines").rsi_high,
            false,
            "{engine:?}: BREAK must force board-side RSI to SPACE immediately",
        );

        // Default 88-SIO is 110 baud, 8N2: one frame is 100 ms. A held BREAK
        // therefore reaches the COM2502 as zero data with a missing stop bit.
        host.commit_panel_activity(Duration::from_millis(110));
        assert_eq!(
            host.peek_io_port(0x00) & 0x09,
            0x08,
            "{engine:?}: Rev1 status must report RDA ready (D0 low) plus FE on D3",
        );
        assert_eq!(host.peek_io_port(0x01), 0x00);
        assert!(!host.serial_rx_line_idle(BackendSerialPort::Port0));

        assert!(host.serial_set_receive_break(BackendSerialPort::Port0, false));
        assert!(host.serial_rx_line_idle(BackendSerialPort::Port0));
        assert!(host.sio_logical_lines().expect("88-SIO lines").rsi_high);
        assert_eq!(host.debugger_input_port(0x01), 0x00);
    }
}

#[test]
fn both_engines_deliver_asr_break_to_88_2sio_as_space_and_mc6850_framing_error() {
    for engine in EmulationEngine::ALL {
        let mut host = BackendHost::from_engine(engine).expect("built-in Rust 8080 engine");
        host.configure_serial_board(SerialBoard::TwoSio88);
        host.power(true);
        host.front_panel_reset();
        host.debugger_output_port(0x10, 0x95); // /16, 8N1, receive IRQ enabled

        assert!(host.serial_set_receive_break(BackendSerialPort::Port0, true));
        assert!(!host.serial_rx_line_idle(BackendSerialPort::Port0));

        // Default Port 0 strap is 110 baud. 8N1 needs about 90.91 ms.
        host.commit_panel_activity(Duration::from_millis(110));
        assert_eq!(
            host.peek_io_port(0x10) & 0x11,
            0x11,
            "{engine:?}: MC6850 must expose RDRF plus FE after a held receive BREAK",
        );
        assert_eq!(host.peek_io_port(0x11), 0x00);
        assert!(!host.serial_rx_line_idle(BackendSerialPort::Port0));

        assert!(host.serial_set_receive_break(BackendSerialPort::Port0, false));
        assert!(host.serial_rx_line_idle(BackendSerialPort::Port0));
        assert_eq!(host.debugger_input_port(0x11), 0x00);
    }
}

#[test]
fn short_break_release_never_fabricates_a_nul_character_in_either_serial_board() {
    for engine in EmulationEngine::ALL {
        for board in [SerialBoard::Sio88, SerialBoard::TwoSio88] {
            let mut host = BackendHost::from_engine(engine).expect("built-in Rust 8080 engine");
            host.configure_serial_board(board);
            host.power(true);
            host.front_panel_reset();
            if board == SerialBoard::TwoSio88 {
                host.debugger_output_port(0x10, 0x15); // /16, 8N1
            }

            assert!(host.serial_set_receive_break(BackendSerialPort::Port0, true));
            host.commit_panel_activity(Duration::from_millis(20));
            assert!(host.serial_set_receive_break(BackendSerialPort::Port0, false));
            assert!(host.serial_rx_line_idle(BackendSerialPort::Port0));
            assert_eq!(
                host.serial_rx_len(BackendSerialPort::Port0),
                0,
                "{engine:?} {board:?}: releasing BREAK before one complete frame must not synthesize 00h",
            );
        }
    }
}

#[test]
fn asr_keyboard_break_is_not_encoded_as_a_byte() {
    let source = include_str!("../src/peripherals/asr33/keyboard.rs");
    assert!(source.contains("KeyKind::Break => return None"));
    assert!(!source.contains("KeyKind::Break => return Some(0)"));
    assert!(!source.contains("KeyKind::Break => return Some(0x00)"));
}

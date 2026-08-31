use std::time::Duration;

use rustair::backend::{BackendHost, BackendSerialPort, EmulationEngine};
use rustair::config::SerialBoard;

const TCP_APP: &str = include_str!("../src/app/external_serial.rs");
const TERMINAL_APP: &str = include_str!("../src/app/terminal_serial.rs");
const ASR_APP: &str = include_str!("../src/app/asr33_controller.rs");

#[test]
fn both_engines_keep_tdr_tsr_clocking_under_break_without_fabricating_wire_bytes() {
    for engine in EmulationEngine::ALL {
        let mut host = BackendHost::from_engine(engine).expect("built-in Rust 8080 engine");
        host.configure_serial_board(SerialBoard::TwoSio88);
        host.power(true);
        host.front_panel_reset();

        // 75h = /16, 8N1, CR6:CR5=11: RTS LOW + continuous BREAK/SPACE.
        host.debugger_output_port(0x10, 0x75);
        assert!(
            host.serial_modem_lines(BackendSerialPort::Port0)
                .expect("88-2SIO exposes MC6850 pins")
                .break_active,
            "{engine:?}: BREAK must be visible as a physical line condition",
        );

        host.debugger_output_port(0x11, b'B');
        assert_eq!(host.peek_io_port(0x10) & 0x02, 0, "{engine:?}: TDR starts full");

        // At the default 110-baud tap this 8N1 frame needs about 90.91 ms.
        // Chassis clocks continue while the CPU is STOPped, so 110 ms is enough
        // for TDR->TSR and full internal shift completion in either backend.
        host.commit_panel_activity(Duration::from_millis(110));
        assert_eq!(
            host.peek_io_port(0x10) & 0x02,
            0x02,
            "{engine:?}: BREAK must not inhibit TDR->TSR or TDRE",
        );
        assert!(
            !host.serial_tx_busy(BackendSerialPort::Port0),
            "{engine:?}: TSR must continue clocking internally under BREAK",
        );
        assert_eq!(
            host.serial_tx_front(BackendSerialPort::Port0),
            None,
            "{engine:?}: a frame overlapped by BREAK cannot emerge as a valid byte",
        );

        // Releasing BREAK restores normal framing for the next complete byte.
        host.debugger_output_port(0x10, 0x15); // /16, 8N1, normal TxD
        assert!(
            !host.serial_modem_lines(BackendSerialPort::Port0)
                .expect("88-2SIO exposes MC6850 pins")
                .break_active,
        );
        host.debugger_output_port(0x11, b'Z');
        host.commit_panel_activity(Duration::from_millis(110));
        assert_eq!(
            host.serial_tx_front(BackendSerialPort::Port0),
            Some(b'Z'),
            "{engine:?}: first complete post-BREAK frame must be valid",
        );
    }
}

#[test]
fn byte_only_internal_and_tcp_endpoints_do_not_invent_a_break_character() {
    for (name, source) in [
        ("External TCP", TCP_APP),
        ("Text Terminal", TERMINAL_APP),
        ("ASR-33", ASR_APP),
    ] {
        assert!(
            !source.contains("serial_break_active_at"),
            "{name} must not translate electrical BREAK into byte-level endpoint policy",
        );
        assert!(
            !source.contains("BREAK -> 0x00")
                && !source.contains("BREAK => 0x00")
                && !source.contains("break_byte"),
            "{name} must never fabricate NUL/00h for BREAK",
        );
    }
}

//! Regression guards for the ASR-33 reader conditions used by Authentic Load.
//!
//! These tests deliberately inspect the production reader controller rather
//! than duplicating its policy in a second helper. The physical invariant is
//! LINE mode plus a free receive shift path. An unread MC6850 RDR must NOT act
//! as hidden host flow control, because the next serial frame may arrive and
//! cause a genuine hardware overrun.

use std::time::Duration;

use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{RamInit, RamSize, SerialBoard};

const ASR33_WINDOW_SOURCE: &str = include_str!("../src/app/ui/asr33_window.rs");

fn production_reader_body() -> &'static str {
    let start = ASR33_WINDOW_SOURCE
        .find("fn update_paper_tape_reader(&mut self)")
        .expect("production ASR-33 reader controller must exist");
    let tail = &ASR33_WINDOW_SOURCE[start..];
    let end = tail
        .find("fn update_paper_tape_punch(&mut self)")
        .expect("reader controller must end before punch controller");
    &tail[..end]
}

#[test]
fn authentic_reader_cannot_advance_while_asr_is_off_or_local() {
    let body = production_reader_body();
    let line_gate = body
        .find("self.tty.mode != TtyMode::Line")
        .expect("reader must explicitly require ASR-33 LINE mode");
    let consume = body
        .find("self.tty.next_tape_byte()")
        .expect("reader controller must consume tape through next_tape_byte");

    assert!(
        line_gate < consume,
        "LINE-mode guard must run before any paper-tape byte is consumed"
    );
    assert!(
        body[line_gate..consume].contains("return;"),
        "OFF/LOCAL must leave the reader controller before tape advancement"
    );
}

#[test]
fn authentic_reader_waits_for_rx_shift_path_not_for_guest_to_empty_rdr() {
    let body = production_reader_body();
    let rx_gate = body
        .find("!self.asr_serial_rx_line_idle()")
        .expect("reader must wait while the physical RX shift path is occupied");
    let consume = body
        .find("self.tty.next_tape_byte()")
        .expect("reader controller must consume tape through next_tape_byte");

    assert!(
        rx_gate < consume,
        "RX-line occupancy guard must run before the next paper-tape byte is consumed"
    );
    assert!(
        body[rx_gate..consume].contains("return;"),
        "an in-progress serial frame must block another frame from starting"
    );
    assert!(
        !body.contains("!self.asr_serial_rx_empty()"),
        "RDR occupancy must not become hidden host flow control"
    );

    // Exercise the actual 88-2SIO receiver boundary through the unified
    // Adaptive Cycle backend. Configure port 0 exactly like the authentic
    // loader (11h = /16, 8N2, RTS LOW), then inject one tape character.
    let mut machine = BackendHost::default();
    machine.configure_memory(RamSize::K4, RamInit::Zeroed);
    machine.configure_serial_board(SerialBoard::TwoSio88);
    machine.power(true);
    machine.clear_serial();
    machine.debugger_output_port(0x10, 0x11);

    assert!(machine.serial_rx_line_idle(BackendSerialPort::Port0));
    assert!(machine.serial_rx_empty(BackendSerialPort::Port0));

    machine.serial_receive(BackendSerialPort::Port0, 0xAE);
    assert!(
        !machine.serial_rx_line_idle(BackendSerialPort::Port0),
        "the receiver shift path must be busy while the 110-baud frame is arriving"
    );

    machine.commit_panel_activity(Duration::from_millis(100));

    assert!(
        machine.serial_rx_line_idle(BackendSerialPort::Port0),
        "the physical RX line must become free when the frame completes"
    );
    assert!(
        !machine.serial_rx_empty(BackendSerialPort::Port0),
        "the completed character must remain unread in RDR until guest IN"
    );
}

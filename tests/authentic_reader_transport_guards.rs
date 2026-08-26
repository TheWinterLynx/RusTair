//! Regression guards for the ASR-33 reader conditions used by Authentic Load.
//!
//! These tests deliberately inspect the production reader controller rather
//! than duplicating its policy in a second helper. If a future refactor removes
//! the LINE-only or RX-empty gate before `next_tape_byte()`, the regression
//! suite fails immediately.

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
fn authentic_reader_cannot_advance_until_guest_consumes_pending_rx() {
    let body = production_reader_body();
    let rx_gate = body
        .find("!self.asr_serial_rx_empty()")
        .expect("reader must explicitly wait for an empty UART RX register");
    let consume = body
        .find("self.tty.next_tape_byte()")
        .expect("reader controller must consume tape through next_tape_byte");

    assert!(
        rx_gate < consume,
        "RX-empty guard must run before the next paper-tape byte is consumed"
    );
    assert!(
        body[rx_gate..consume].contains("return;"),
        "pending guest RX must leave the reader controller before tape advancement"
    );

    // Confirm that the backend predicate used by the production guard really
    // distinguishes an empty receiver from a byte still waiting for guest IN.
    let mut machine = BackendHost::rust_fast();
    machine.configure_memory(RamSize::K4, RamInit::Zeroed);
    machine.configure_serial_board(SerialBoard::Sio88);
    machine.power(true);
    machine.clear_serial();

    assert!(machine.serial_rx_empty(BackendSerialPort::Port0));
    machine.serial_receive(BackendSerialPort::Port0, 0xAE);
    assert!(
        !machine.serial_rx_empty(BackendSerialPort::Port0),
        "injected tape byte must remain pending until the guest consumes it"
    );
}

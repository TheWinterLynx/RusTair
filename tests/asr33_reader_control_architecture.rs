const ASR_WINDOW: &str = include_str!("../src/app/ui/asr33_window.rs");
const SERIAL_HARDWARE: &str = include_str!("../src/app/serial_hardware.rs");
const TERMINAL_CONTROLLER: &str = include_str!("../src/app/terminal_controller.rs");
const EXTERNAL_TCP: &str = include_str!("../src/app/external_serial.rs");
const EXTERNAL_COM: &str = include_str!("../src/app/external_com.rs");

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).expect("start marker must exist");
    let tail = &source[start..];
    let end = tail.find(end).expect("end marker must exist");
    &tail[..end]
}

#[test]
fn reader_control_uses_physical_rts_and_never_cpu_run_as_motor_authority() {
    let reader_update = between(
        ASR_WINDOW,
        "fn update_paper_tape_reader",
        "fn update_paper_tape_punch",
    );
    assert!(
        reader_update.contains("asr_reader_motor_running()"),
        "reader transport must resolve its motor command from the selected physical wiring",
    );
    assert!(
        reader_update.contains("asr_serial_rx_line_idle()"),
        "reader may start a character only when the physical RX shift path is free",
    );
    assert!(
        !reader_update.contains("machine.running()"),
        "an ASR-33 reader/88-TYA relay must not depend on the 8080 RUN latch",
    );
    assert!(
        !reader_update.contains("asr_serial_rx_empty()"),
        "RDRF/RDR occupancy must not become hidden reader flow control",
    );

    assert!(
        SERIAL_HARDWARE.contains("serial_modem_lines(port)")
            && SERIAL_HARDWARE.contains("lines.rts_high")
            && SERIAL_HARDWARE.contains("effective_running"),
        "88-TYA motor authority must come from the actual MC6850 RTS pin exposed by the backend",
    );
}

#[test]
fn manual_buttons_cannot_override_88_tya_reader_control() {
    let controls = between(
        ASR_WINDOW,
        "fn draw_tty_reader_controls",
        "fn draw_tty_punch_controls",
    );
    assert!(controls.contains("ReaderControlMode::ALL"));
    assert!(controls.contains("let manual = self.asr33.reader_control == ReaderControlMode::Manual"));
    assert!(controls.contains("manual && can_run && !self.asr33.reader_running"));
    assert!(controls.contains("manual && self.asr33.reader_running"));
    assert!(controls.contains("51h (121 octal)"));
    assert!(controls.contains("11h (021 octal)"));
}

#[test]
fn all_host_rx_sources_gate_on_line_occupancy_not_rdr_emptiness() {
    assert!(TERMINAL_CONTROLLER.contains("terminal_serial_rx_line_idle()"));
    assert!(!TERMINAL_CONTROLLER.contains("terminal_serial_rx_empty()"));

    assert!(EXTERNAL_TCP.contains("serial_rx_line_idle_at(connection)"));
    assert!(!EXTERNAL_TCP.contains("serial_rx_empty_at(connection)"));

    assert!(EXTERNAL_COM.contains("serial_rx_line_idle_at(connection)"));
    assert!(!EXTERNAL_COM.contains("serial_rx_empty_at(connection)"));
}

#[test]
fn tape_repaint_clock_keeps_reader_alive_with_cpu_stopped() {
    let repaint = between(
        ASR_WINDOW,
        "fn request_tape_transport_repaint",
        "fn draw_tty_window",
    );
    assert!(repaint.contains("asr_reader_motor_running()"));
    assert!(repaint.contains("asr_serial_rx_line_idle()"));
    assert!(
        !repaint.contains("machine.running()"),
        "stopped CPU must not freeze the independently powered reader/card clock",
    );
}

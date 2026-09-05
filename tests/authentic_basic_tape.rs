//! Integration/regression tests for the authentic Microsoft 4K BASIC 3.2 path.
//!
//! Normal tests use synthetic MITS paper-tape records and require no BASIC
//! media. The ignored end-to-end test reads an operator-supplied external tape
//! from `RUSTAIR_BASIC32_TAP`; the tape is never committed to RusTair.

use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{RamInit, RamSize, SerialBoard};
use std::path::{Path, PathBuf};

const BASIC32_LEADER: u8 = 0xAE;
const BASIC32_CHECKSUM_LOADER_LEN: usize = 0xAE;
const PROGRAM_RECORD_SYNC: u8 = 0x3C;
const GO_RECORD_SYNC: u8 = 0x78;
const BASIC32_IMAGE: &[u8; 4096] = include_bytes!("../assets/4kbas32.bin");

const BASIC32_4K_88_SIO: [u8; 20] = [
    0x21, 0xAE, 0x0F, 0x31, 0x12, 0x00, 0xDB, 0x00, 0x0F, 0xD8,
    0xDB, 0x01, 0xBD, 0xC8, 0x2D, 0x77, 0xC0, 0xE9, 0x03, 0x00,
];

const BASIC32_4K_88_2SIO: [u8; 28] = [
    0x3E, 0x03, 0xD3, 0x10, 0x3E, 0x11, 0xD3, 0x10, 0x21, 0xAE,
    0x0F, 0x31, 0x1A, 0x00, 0xDB, 0x10, 0x0F, 0xD0, 0xDB, 0x11,
    0xBD, 0xC8, 0x2D, 0x77, 0xC0, 0xE9, 0x0B, 0x00,
];

#[derive(Clone, Copy)]
struct TestBootstrap {
    bytes: &'static [u8],
    sense: u8,
}

fn bootstrap_for(board: SerialBoard) -> TestBootstrap {
    match board {
        SerialBoard::Sio88 => TestBootstrap {
            bytes: &BASIC32_4K_88_SIO,
            sense: 0x00,
        },
        SerialBoard::TwoSio88 => TestBootstrap {
            bytes: &BASIC32_4K_88_2SIO,
            sense: 0x08,
        },
    }
}

#[derive(Clone, Debug)]
struct ProgramRecord {
    offset: usize,
    address: u16,
    data: Vec<u8>,
    checksum: u8,
}

#[derive(Clone, Debug)]
struct ParsedBasic32Tape {
    reader_start: usize,
    leader_end: usize,
    checksum_loader_start: usize,
    checksum_loader_end: usize,
    program_start: usize,
    records: Vec<ProgramRecord>,
    go_offset: usize,
    go_address: u16,
}

fn find_basic32_leader(bytes: &[u8]) -> Result<(usize, usize), String> {
    const MIN_LEADER_RUN: usize = 16;
    let mut run_start = None;
    let mut run_len = 0usize;

    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte == BASIC32_LEADER {
            if run_start.is_none() {
                run_start = Some(index);
            }
            run_len += 1;
            continue;
        }

        if run_len >= MIN_LEADER_RUN {
            return Ok((run_start.expect("leader run has a start"), index));
        }
        run_start = None;
        run_len = 0;
    }

    if run_len >= MIN_LEADER_RUN {
        return Err("BASIC 3.2 leader reaches end-of-tape before the checksum loader".into());
    }
    Err(format!(
        "No BASIC 3.2 leader found: expected at least {MIN_LEADER_RUN} consecutive AEh bytes"
    ))
}

fn parse_basic32_tape(bytes: &[u8]) -> Result<ParsedBasic32Tape, String> {
    let (reader_start, leader_end) = find_basic32_leader(bytes)?;
    let checksum_loader_start = leader_end;
    let checksum_loader_end = checksum_loader_start
        .checked_add(BASIC32_CHECKSUM_LOADER_LEN)
        .ok_or_else(|| "Checksum-loader offset overflow".to_owned())?;
    if checksum_loader_end > bytes.len() {
        return Err(format!(
            "Premature end-of-tape in BASIC 3.2 checksum loader: need {} bytes after the leader, only {} remain",
            BASIC32_CHECKSUM_LOADER_LEN,
            bytes.len().saturating_sub(checksum_loader_start)
        ));
    }

    let mut cursor = checksum_loader_end;
    while bytes.get(cursor) == Some(&0x00) {
        cursor += 1;
    }
    let program_start = cursor;
    let mut records = Vec::new();

    loop {
        while bytes.get(cursor) == Some(&0x00) {
            cursor += 1;
        }
        let Some(sync) = bytes.get(cursor).copied() else {
            return Err("Premature end-of-tape: no 78h Go Record was found".into());
        };

        match sync {
            PROGRAM_RECORD_SYNC => {
                if cursor + 5 > bytes.len() {
                    return Err(format!(
                        "Premature end-of-tape in program-record header at file offset {cursor}"
                    ));
                }
                let count = bytes[cursor + 1] as usize;
                let record_len = count
                    .checked_add(5)
                    .ok_or_else(|| "Program-record length overflow".to_owned())?;
                let end = cursor
                    .checked_add(record_len)
                    .ok_or_else(|| "Program-record offset overflow".to_owned())?;
                if end > bytes.len() {
                    return Err(format!(
                        "Premature end-of-tape in program record at file offset {cursor}: declares {count} data bytes"
                    ));
                }

                let low = bytes[cursor + 2];
                let high = bytes[cursor + 3];
                let address = u16::from_le_bytes([low, high]);
                let data_start = cursor + 4;
                let data_end = data_start + count;
                let checksum = bytes[data_end];
                let calculated = bytes[cursor + 2..data_end]
                    .iter()
                    .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
                if checksum != calculated {
                    return Err(format!(
                        "Checksum failure at tape offset {cursor}: record {address:04X}h + {count} bytes has {checksum:02X}h, expected {calculated:02X}h"
                    ));
                }

                let end_address = usize::from(address)
                    .checked_add(count)
                    .ok_or_else(|| "Program-record address overflow".to_owned())?;
                if end_address > 0x1_0000 {
                    return Err(format!(
                        "Program record at tape offset {cursor} wraps past FFFFh"
                    ));
                }

                records.push(ProgramRecord {
                    offset: cursor,
                    address,
                    data: bytes[data_start..data_end].to_vec(),
                    checksum,
                });
                cursor = end;
            }
            GO_RECORD_SYNC => {
                if cursor + 3 > bytes.len() {
                    return Err(format!(
                        "Premature end-of-tape in Go Record at file offset {cursor}"
                    ));
                }
                if records.is_empty() {
                    return Err("Go Record encountered before any program records".into());
                }
                return Ok(ParsedBasic32Tape {
                    reader_start,
                    leader_end,
                    checksum_loader_start,
                    checksum_loader_end,
                    program_start,
                    records,
                    go_offset: cursor,
                    go_address: u16::from_le_bytes([bytes[cursor + 1], bytes[cursor + 2]]),
                });
            }
            other => {
                return Err(format!(
                    "Unexpected non-zero byte {other:02X}h at tape offset {cursor}; expected 3Ch program record, 78h Go Record, or a null gap"
                ));
            }
        }
    }
}

fn reconstruct_program_image(
    parsed: &ParsedBasic32Tape,
) -> Result<([u8; 4096], [bool; 4096]), String> {
    let mut image = [0u8; 4096];
    let mut covered = [false; 4096];

    for record in &parsed.records {
        let start = usize::from(record.address);
        let end = start
            .checked_add(record.data.len())
            .ok_or_else(|| format!("Record at offset {} address overflow", record.offset))?;
        if end > image.len() {
            return Err(format!(
                "4K BASIC tape record at offset {} targets {:04X}h..{:04X}h outside 4 KiB RAM",
                record.offset,
                record.address,
                end.saturating_sub(1)
            ));
        }
        image[start..end].copy_from_slice(&record.data);
        covered[start..end].fill(true);
    }

    Ok((image, covered))
}

fn install_bootstrap_through_panel(machine: &mut BackendHost, bootstrap: TestBootstrap) {
    machine.front_panel_reset();
    machine.set_switch_register(0x0000);
    machine.examine(false);
    assert_eq!(machine.front_panel_state().address, 0x0000);

    for (index, byte) in bootstrap.bytes.iter().copied().enumerate() {
        machine.set_switch_register(u16::from(byte));
        machine.deposit(index != 0);
        assert_eq!(
            machine.peek_memory(index as u16),
            Some(byte),
            "front-panel bootstrap deposit failed at {index:04X}h"
        );
    }
}

fn start_bootstrap(machine: &mut BackendHost, bootstrap: TestBootstrap) {
    machine.set_switch_register(0x0000);
    machine.examine(false);
    machine.set_switch_register(u16::from(bootstrap.sense) << 8);
    machine.set_running(true);
    // The 88-2SIO bootstrap first performs a master reset and framing setup.
    // Waiting here mirrors starting the reader only after the UART reaches its
    // status-poll loop and avoids injecting a byte before hardware init.
    machine.run_cycles(1024);
}

fn wait_for_guest_rx_empty(machine: &mut BackendHost, port: BackendSerialPort, tape_offset: usize) {
    for _ in 0..20_000 {
        if machine.serial_rx_empty(port) {
            return;
        }
        machine.run_cycles(16);
    }
    panic!(
        "guest did not consume UART RX byte for tape offset {tape_offset}; PC={:04X}h",
        machine.intel8080_state().pc
    );
}

fn feed_guest_paced(
    machine: &mut BackendHost,
    port: BackendSerialPort,
    bytes: &[u8],
    source_offset: usize,
) {
    for (relative, byte) in bytes.iter().copied().enumerate() {
        let tape_offset = source_offset + relative;
        wait_for_guest_rx_empty(machine, port, tape_offset);
        machine.serial_receive(port, byte);
        wait_for_guest_rx_empty(machine, port, tape_offset);
    }
}

fn assert_program_records_match_memory(machine: &mut BackendHost, parsed: &ParsedBasic32Tape) {
    for record in &parsed.records {
        for (offset, expected) in record.data.iter().copied().enumerate() {
            let address = record.address.wrapping_add(offset as u16);
            assert_eq!(
                machine.peek_memory(address),
                Some(expected),
                "authentic loader RAM mismatch at {address:04X}h from tape record offset {}",
                record.offset
            );
        }
    }
}

fn validate_external_tape_path() -> PathBuf {
    let path = std::env::var_os("RUSTAIR_BASIC32_TAP")
        .map(PathBuf::from)
        .expect("Set RUSTAIR_BASIC32_TAP to the external '4K BASIC Ver 3-2.tap' file before running this ignored test");
    assert!(
        path.is_file(),
        "RUSTAIR_BASIC32_TAP does not name a file: {}",
        path.display()
    );
    path
}

fn read_external_tape(path: &Path) -> Vec<u8> {
    std::fs::read(path)
        .unwrap_or_else(|error| panic!("Could not read {}: {error}", path.display()))
}

fn synthetic_basic32_tape(record_data: &[u8], checksum_delta: u8, include_go: bool) -> Vec<u8> {
    let mut tape = vec![BASIC32_LEADER; 24];
    // The parser needs the correct physical layout, not executable checksum
    // loader code, for synthetic format/error tests.
    tape.extend(std::iter::repeat_n(0x01, BASIC32_CHECKSUM_LOADER_LEN));
    tape.extend_from_slice(&[0x00, 0x00]);

    let address = 0x0100u16;
    let [low, high] = address.to_le_bytes();
    tape.push(PROGRAM_RECORD_SYNC);
    tape.push(record_data.len() as u8);
    tape.push(low);
    tape.push(high);
    tape.extend_from_slice(record_data);
    let checksum = std::iter::once(low)
        .chain(std::iter::once(high))
        .chain(record_data.iter().copied())
        .fold(0u8, |sum, byte| sum.wrapping_add(byte))
        .wrapping_add(checksum_delta);
    tape.push(checksum);
    tape.extend_from_slice(&[0x00, 0x00]);
    if include_go {
        tape.extend_from_slice(&[GO_RECORD_SYNC, 0x00, 0x01]);
    }
    tape
}

#[test]
fn mits_basic32_tape_parser_accepts_records_and_go_record() {
    let tape = synthetic_basic32_tape(&[0x21, 0x34, 0x12, 0x76], 0, true);
    let parsed = parse_basic32_tape(&tape).expect("synthetic tape should parse");
    assert_eq!(parsed.reader_start, 0);
    assert_eq!(parsed.leader_end, 24);
    assert_eq!(
        parsed.checksum_loader_end - parsed.checksum_loader_start,
        BASIC32_CHECKSUM_LOADER_LEN
    );
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].address, 0x0100);
    assert_eq!(parsed.records[0].data, [0x21, 0x34, 0x12, 0x76]);
    assert_eq!(parsed.go_address, 0x0100);
    assert!(parsed.program_start < parsed.go_offset);
}

#[test]
fn mits_basic32_tape_parser_reports_checksum_failure() {
    let tape = synthetic_basic32_tape(&[0xAA, 0x55, 0x10], 1, true);
    let error = parse_basic32_tape(&tape).unwrap_err();
    assert!(error.contains("Checksum failure"), "unexpected error: {error}");
}

#[test]
fn mits_basic32_tape_parser_reports_premature_end_of_tape() {
    let tape = synthetic_basic32_tape(&[0xAA, 0x55, 0x10], 0, false);
    let error = parse_basic32_tape(&tape).unwrap_err();
    assert!(
        error.contains("Premature end-of-tape"),
        "unexpected error: {error}"
    );
}

#[test]
fn wrong_board_bootstrap_does_not_consume_selected_uart_data() {
    for (board, wrong_bootstrap) in [
        (SerialBoard::Sio88, bootstrap_for(SerialBoard::TwoSio88)),
        (SerialBoard::TwoSio88, bootstrap_for(SerialBoard::Sio88)),
    ] {
        let mut machine = BackendHost::default();
        machine.configure_memory(RamSize::K4, RamInit::Zeroed);
        machine.configure_serial_board(board);
        machine.power(true);
        machine.set_running(false);
        install_bootstrap_through_panel(&mut machine, wrong_bootstrap);
        start_bootstrap(&mut machine, wrong_bootstrap);

        machine.serial_receive(BackendSerialPort::Port0, BASIC32_LEADER);
        machine.run_cycles(4096);
        assert!(
            !machine.serial_rx_empty(BackendSerialPort::Port0),
            "wrong bootstrap unexpectedly consumed data on {board:?}"
        );
    }
}

#[test]
fn two_sio_port1_cannot_feed_port0_basic_bootstrap() {
    let mut machine = BackendHost::default();
    machine.configure_memory(RamSize::K4, RamInit::Zeroed);
    machine.configure_serial_board(SerialBoard::TwoSio88);
    machine.power(true);
    machine.set_running(false);
    let bootstrap = bootstrap_for(SerialBoard::TwoSio88);
    install_bootstrap_through_panel(&mut machine, bootstrap);
    start_bootstrap(&mut machine, bootstrap);

    machine.serial_receive(BackendSerialPort::Port1, BASIC32_LEADER);
    machine.run_cycles(4096);
    assert!(
        !machine.serial_rx_empty(BackendSerialPort::Port1),
        "Port 0 bootstrap unexpectedly consumed Port 1 RX data"
    );
}

#[test]
fn bundled_quick_image_is_a_full_nonempty_4k_image() {
    assert_eq!(BASIC32_IMAGE.len(), 4096);
    assert_ne!(&BASIC32_IMAGE[..16], &[0u8; 16]);
}

#[test]
#[ignore = "requires external 4K BASIC Ver 3-2.tap via RUSTAIR_BASIC32_TAP"]
fn authentic_basic32_real_tape_matches_bundled_program_on_adaptive_cycle_and_both_boards() {
    let path = validate_external_tape_path();
    let tape = read_external_tape(&path);
    let parsed = parse_basic32_tape(&tape).unwrap_or_else(|error| {
        panic!(
            "{} is not a valid BASIC 3.2 MITS tape: {error}",
            path.display()
        )
    });

    assert_eq!(
        parsed.go_address, 0x0000,
        "BASIC 3.2 Go Record must enter at 0000h"
    );
    let (reconstructed, covered) =
        reconstruct_program_image(&parsed).expect("4K tape records must fit 4 KiB");
    let covered_count = covered.iter().filter(|covered| **covered).count();
    assert!(
        covered_count >= 3000,
        "tape records cover only {covered_count} bytes; this does not look like Microsoft 4K BASIC 3.2"
    );
    for address in 0..BASIC32_IMAGE.len() {
        if covered[address] {
            assert_eq!(
                reconstructed[address], BASIC32_IMAGE[address],
                "external BASIC tape differs from bundled Quick Load image at {address:04X}h"
            );
        }
    }

    for board in [SerialBoard::Sio88, SerialBoard::TwoSio88] {
        let mut machine = BackendHost::default();
        machine.configure_memory(RamSize::K4, RamInit::Zeroed);
        machine.configure_serial_board(board);
        machine.power(true);
        machine.set_running(false);

        let bootstrap = bootstrap_for(board);
        install_bootstrap_through_panel(&mut machine, bootstrap);
        start_bootstrap(&mut machine, bootstrap);

        let pre_go = &tape[parsed.reader_start..parsed.go_offset];
        feed_guest_paced(
            &mut machine,
            BackendSerialPort::Port0,
            pre_go,
            parsed.reader_start,
        );
        machine.run_cycles(512);
        assert_program_records_match_memory(&mut machine, &parsed);

        // Feed the Go Record only after RAM is verified. The checksum
        // loader must consume all three bytes and transfer execution away
        // from its temporary 0Fxx page to the 0000h BASIC entry path.
        feed_guest_paced(
            &mut machine,
            BackendSerialPort::Port0,
            &tape[parsed.go_offset..parsed.go_offset + 3],
            parsed.go_offset,
        );
        let mut entered_basic = false;
        for _ in 0..20_000 {
            machine.run_cycles(16);
            let pc = machine.intel8080_state().pc;
            if pc < 0x0F00 {
                entered_basic = true;
                break;
            }
        }
        assert!(
            entered_basic,
            "Adaptive Cycle / {board:?} did not leave checksum-loader page after Go Record; PC={:04X}h",
            machine.intel8080_state().pc
        );
    }

    eprintln!(
        "Validated {}: {} bytes, {} program records, {} covered RAM bytes, Go={:04X}h",
        path.display(),
        tape.len(),
        parsed.records.len(),
        covered_count,
        parsed.go_address
    );
    for record in &parsed.records {
        eprintln!(
            "  record @ tape {:05}: RAM {:04X}h + {:3} bytes checksum {:02X}h",
            record.offset,
            record.address,
            record.data.len(),
            record.checksum
        );
    }
}

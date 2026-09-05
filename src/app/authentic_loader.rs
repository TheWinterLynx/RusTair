use super::*;

/// Front-panel bootstrap for Microsoft 4K BASIC 3.2 paper tape.
///
/// The loader bytes are the MITS front-panel bootstrap, not a RusTair helper
/// program. BASIC 3.2 uses leader/checksum-loader marker 256 octal (AEh); 4K
/// uses checksum-loader selector 017 octal. The 88-2SIO variant below uses the
/// historically appropriate two-stop-bit ACIA setup for an ASR-33. The published
/// 10h/11h byte image remains the canonical template; when the physical 88-2SIO
/// A2-A7 straps select another block, only the immediate IN/OUT port operands are
/// changed exactly as a real operator would have to enter them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BootstrapDefinition {
    pub(super) board: SerialBoard,
    pub(super) name: &'static str,
    /// Canonical MITS template. `resolved_bytes()` applies only the installed
    /// 88-2SIO I/O-address operands; the historical template itself is immutable.
    pub(super) bytes: &'static [u8],
    pub(super) required_sense: u8,
    pub(super) status_port: u8,
    pub(super) data_port: u8,
    poll_start: u16,
    poll_end: u16,
}

const BASIC32_4K_END: u16 = 0x0FFF;
const CHECKSUM_LOADER_START: u16 = 0x0F00;
const CHECKSUM_LOADER_END: u16 = 0x0FAD;

const BASIC32_4K_88_SIO: [u8; 20] = [
    0x21, 0xAE, 0x0F, 0x31, 0x12, 0x00, 0xDB, 0x00, 0x0F, 0xD8, 0xDB, 0x01, 0xBD, 0xC8,
    0x2D, 0x77, 0xC0, 0xE9, 0x03, 0x00,
];

const BASIC32_4K_88_2SIO: [u8; 28] = [
    0x3E, 0x03, 0xD3, 0x10, 0x3E, 0x11, 0xD3, 0x10, 0x21, 0xAE, 0x0F, 0x31, 0x1A, 0x00,
    0xDB, 0x10, 0x0F, 0xD0, 0xDB, 0x11, 0xBD, 0xC8, 0x2D, 0x77, 0xC0, 0xE9, 0x0B, 0x00,
];

impl BootstrapDefinition {
    /// Canonical published/default installation. Tests use this to protect the
    /// original MITS byte image; production loading uses `for_installed()`.
    pub(super) const fn for_board(board: SerialBoard) -> Self {
        match board {
            SerialBoard::Sio88 => Self {
                board,
                name: "Microsoft 4K BASIC 3.2 — MITS 88-SIO rev. 1 bootstrap",
                bytes: &BASIC32_4K_88_SIO,
                required_sense: 0x00,
                status_port: 0x00,
                data_port: 0x01,
                poll_start: 0x0003,
                poll_end: 0x0009,
            },
            SerialBoard::TwoSio88 => Self {
                board,
                name: "Microsoft 4K BASIC 3.2 — MITS 88-2SIO Port 0 bootstrap",
                bytes: &BASIC32_4K_88_2SIO,
                required_sense: 0x08,
                status_port: 0x10,
                data_port: 0x11,
                poll_start: 0x000B,
                poll_end: 0x0011,
            },
        }
    }

    pub(super) const fn for_installed(board: SerialBoard, straps: TwoSioStraps) -> Self {
        match board {
            SerialBoard::Sio88 => Self::for_board(SerialBoard::Sio88),
            SerialBoard::TwoSio88 => Self {
                board,
                name: "Microsoft 4K BASIC 3.2 — MITS 88-2SIO Port 0 bootstrap",
                bytes: &BASIC32_4K_88_2SIO,
                required_sense: 0x08,
                status_port: straps.address.port0_status(),
                data_port: straps.address.port0_data(),
                poll_start: 0x000B,
                poll_end: 0x0011,
            },
        }
    }

    /// Return the actual bytes the operator must deposit for the installed card.
    /// The 88-2SIO instruction sequence is unchanged; only its four immediate
    /// port operands follow A2-A7. This is not an address alias or compatibility
    /// hack: a physical 8080 program must name the address decoded by the card.
    fn resolved_bytes(self) -> Vec<u8> {
        let mut bytes = self.bytes.to_vec();
        if self.board == SerialBoard::TwoSio88 {
            bytes[0x03] = self.status_port;
            bytes[0x07] = self.status_port;
            bytes[0x0F] = self.status_port;
            bytes[0x13] = self.data_port;
        }
        bytes
    }

    const fn last_address(self) -> u16 {
        self.bytes.len() as u16 - 1
    }

    const fn pc_is_polling(self, pc: u16) -> bool {
        pc >= self.poll_start && pc <= self.poll_end
    }
}

#[derive(Clone, Copy)]
struct BootstrapInstructionInfo {
    start: usize,
    len: usize,
    mnemonic: &'static str,
    effect: &'static str,
    purpose: &'static str,
}

const SIO_BOOTSTRAP_INSTRUCTIONS: &[BootstrapInstructionInfo] = &[
    BootstrapInstructionInfo {
        start: 0x00,
        len: 3,
        mnemonic: "LXI H,$0FAE",
        effect: "Loads HL with 0FAEh.",
        purpose: "Points one byte above the checksum-loader destination. The bootstrap decrements L before each store, so the first payload byte lands at 0FADh and the loader is built backwards toward 0F00h.",
    },
    BootstrapInstructionInfo {
        start: 0x03,
        len: 3,
        mnemonic: "LXI SP,$0012",
        effect: "Loads the stack pointer with 0012h.",
        purpose: "Makes the two bytes at 0012h/0013h act as a tiny return vector. Conditional RET instructions can therefore jump back to 0003h without needing a longer JMP instruction.",
    },
    BootstrapInstructionInfo {
        start: 0x06,
        len: 2,
        mnemonic: "IN $00",
        effect: "Reads the MITS 88-SIO status port into accumulator A.",
        purpose: "Checks whether the serial receiver has a paper-tape byte ready before attempting to read the data port.",
    },
    BootstrapInstructionInfo {
        start: 0x08,
        len: 1,
        mnemonic: "RRC",
        effect: "Rotates A right and copies status bit 0 into the Carry flag.",
        purpose: "Converts the 88-SIO active-low receiver-ready status bit into a condition that the following RC can test cheaply.",
    },
    BootstrapInstructionInfo {
        start: 0x09,
        len: 1,
        mnemonic: "RC",
        effect: "Returns through the stack vector when Carry is set.",
        purpose: "When the 88-SIO says no character is ready, this returns to 0003h and polls again instead of reading an empty data port.",
    },
    BootstrapInstructionInfo {
        start: 0x0A,
        len: 2,
        mnemonic: "IN $01",
        effect: "Reads one byte from the MITS 88-SIO data port into A.",
        purpose: "This is the real guest IN that consumes the next byte delivered by the ASR-33 paper-tape reader.",
    },
    BootstrapInstructionInfo {
        start: 0x0C,
        len: 1,
        mnemonic: "CMP L",
        effect: "Compares the received byte in A with register L and updates flags.",
        purpose: "Initially L is AEh, the BASIC 3.2 leader marker. This lets the bootstrap ignore leader bytes until actual checksum-loader data arrives.",
    },
    BootstrapInstructionInfo {
        start: 0x0D,
        len: 1,
        mnemonic: "RZ",
        effect: "Returns through the stack vector if the comparison was equal.",
        purpose: "Skips a leader byte and returns to the polling loop without storing it in RAM.",
    },
    BootstrapInstructionInfo {
        start: 0x0E,
        len: 1,
        mnemonic: "DCR L",
        effect: "Decrements the low byte of HL.",
        purpose: "Moves the destination from 0FAEh to 0FADh, then 0FACh, and so on, so the checksum loader is reconstructed backwards in memory.",
    },
    BootstrapInstructionInfo {
        start: 0x0F,
        len: 1,
        mnemonic: "MOV M,A",
        effect: "Stores accumulator A into memory at address HL.",
        purpose: "Deposits the received paper-tape byte into the checksum-loader image being assembled at the top of the 4 KiB address space.",
    },
    BootstrapInstructionInfo {
        start: 0x10,
        len: 1,
        mnemonic: "RNZ",
        effect: "Returns through the stack vector while the last DCR L result is non-zero.",
        purpose: "Keeps fetching and storing bytes until L reaches 00h, meaning the entire checksum loader down to 0F00h has been received.",
    },
    BootstrapInstructionInfo {
        start: 0x11,
        len: 1,
        mnemonic: "PCHL",
        effect: "Copies HL into the program counter.",
        purpose: "When HL has reached 0F00h, transfers execution to the checksum loader that was just read from paper tape.",
    },
    BootstrapInstructionInfo {
        start: 0x12,
        len: 2,
        mnemonic: "STACK RETURN VECTOR $0003",
        effect: "These two bytes form the little-endian address 0003h; they are data, not an instruction executed in sequence.",
        purpose: "RC, RZ and RNZ pop this address as their return target, producing a compact loop back to LXI SP at 0003h.",
    },
];

const TWO_SIO_BOOTSTRAP_INSTRUCTIONS: &[BootstrapInstructionInfo] = &[
    BootstrapInstructionInfo {
        start: 0x00,
        len: 2,
        mnemonic: "MVI A,$03",
        effect: "Loads accumulator A with 03h.",
        purpose: "Prepares the MC6850 master-reset control value used to initialize 88-2SIO Port 0 before the paper tape is read.",
    },
    BootstrapInstructionInfo {
        start: 0x02,
        len: 2,
        mnemonic: "OUT $10",
        effect: "Writes A to the 88-2SIO Port 0 control register at 10h.",
        purpose: "Sends the 03h master-reset command to the serial interface so loading starts from a known UART state.",
    },
    BootstrapInstructionInfo {
        start: 0x04,
        len: 2,
        mnemonic: "MVI A,$11",
        effect: "Loads accumulator A with 11h (021 octal).",
        purpose: "Prepares the historical BASIC 3.2 88-2SIO control value for the ASR-33 two-stop-bit setup.",
    },
    BootstrapInstructionInfo {
        start: 0x06,
        len: 2,
        mnemonic: "OUT $10",
        effect: "Writes the prepared control value to 88-2SIO Port 0.",
        purpose: "Configures the serial interface for the paper-tape/ASR-33 connection before polling for input.",
    },
    BootstrapInstructionInfo {
        start: 0x08,
        len: 3,
        mnemonic: "LXI H,$0FAE",
        effect: "Loads HL with 0FAEh.",
        purpose: "Points one byte above the checksum-loader destination so received bytes can be stored backwards from 0FADh toward 0F00h.",
    },
    BootstrapInstructionInfo {
        start: 0x0B,
        len: 3,
        mnemonic: "LXI SP,$001A",
        effect: "Loads the stack pointer with 001Ah.",
        purpose: "Makes the final two bootstrap bytes at 001Ah/001Bh a return vector to 000Bh for the conditional RET loop.",
    },
    BootstrapInstructionInfo {
        start: 0x0E,
        len: 2,
        mnemonic: "IN $10",
        effect: "Reads the 88-2SIO Port 0 status register into A.",
        purpose: "Checks the MC6850 receiver-data-ready bit before consuming a character from the data register.",
    },
    BootstrapInstructionInfo {
        start: 0x10,
        len: 1,
        mnemonic: "RRC",
        effect: "Rotates A right and copies status bit 0 into Carry.",
        purpose: "Turns the 88-2SIO receiver-ready bit into the Carry condition used by the following RNC.",
    },
    BootstrapInstructionInfo {
        start: 0x11,
        len: 1,
        mnemonic: "RNC",
        effect: "Returns through the stack vector when Carry is clear.",
        purpose: "If no received byte is ready, returns to 000Bh and polls again without touching the data register.",
    },
    BootstrapInstructionInfo {
        start: 0x12,
        len: 2,
        mnemonic: "IN $11",
        effect: "Reads one byte from the 88-2SIO Port 0 data register into A.",
        purpose: "This is the real guest IN that removes the next paper-tape byte from the emulated UART RX register.",
    },
    BootstrapInstructionInfo {
        start: 0x14,
        len: 1,
        mnemonic: "CMP L",
        effect: "Compares the received byte in A with register L.",
        purpose: "Initially detects the AEh BASIC 3.2 leader marker so leader characters are ignored rather than stored.",
    },
    BootstrapInstructionInfo {
        start: 0x15,
        len: 1,
        mnemonic: "RZ",
        effect: "Returns through the stack vector if A equals L.",
        purpose: "Skips a leader marker and immediately resumes the serial polling loop.",
    },
    BootstrapInstructionInfo {
        start: 0x16,
        len: 1,
        mnemonic: "DCR L",
        effect: "Decrements the low byte of HL.",
        purpose: "Advances the backwards destination from 0FADh toward 0F00h before each byte is stored.",
    },
    BootstrapInstructionInfo {
        start: 0x17,
        len: 1,
        mnemonic: "MOV M,A",
        effect: "Stores A at memory address HL.",
        purpose: "Writes the newly received byte into the checksum-loader image in RAM.",
    },
    BootstrapInstructionInfo {
        start: 0x18,
        len: 1,
        mnemonic: "RNZ",
        effect: "Returns through the stack vector while L is non-zero.",
        purpose: "Keeps receiving checksum-loader bytes until the backwards destination reaches 0F00h.",
    },
    BootstrapInstructionInfo {
        start: 0x19,
        len: 1,
        mnemonic: "PCHL",
        effect: "Copies HL into the program counter.",
        purpose: "Transfers control to 0F00h after the complete checksum loader has been reconstructed from tape.",
    },
    BootstrapInstructionInfo {
        start: 0x1A,
        len: 2,
        mnemonic: "STACK RETURN VECTOR $000B",
        effect: "These bytes form little-endian address 000Bh and are used as stack data rather than sequential code.",
        purpose: "RNC, RZ and RNZ pop 000Bh so the compact bootstrap loops back to LXI SP without a separate JMP instruction.",
    },
];

pub(super) struct AuthenticLoaderState {
    pub(super) window_open: bool,
    pub(super) last_install_log: Vec<String>,
}

impl Default for AuthenticLoaderState {
    fn default() -> Self {
        Self {
            window_open: false,
            last_install_log: Vec::new(),
        }
    }
}

fn require_panel_entry_ready(machine: &mut BackendHost) -> Result<(), String> {
    if !machine.powered() {
        return Err("Power ON the Altair before operating the front panel.".into());
    }
    if machine.running() {
        return Err("STOP the Altair before operating EXAMINE/DEPOSIT.".into());
    }
    Ok(())
}

/// Microsoft 4K BASIC is not satisfied by an aggregate count of 4096 installed
/// bytes. The bootstrap, return vector and checksum loader all rely on the low
/// 4 KiB address space. Every address in 0000h..0FFFh must therefore be decoded
/// by exactly one RAM card; gaps and overlapping responders are both invalid.
fn low_4k_mapping_issue(machine: &mut BackendHost) -> Option<String> {
    for address in 0..=BASIC32_4K_END {
        let inspection = machine.inspect_memory_mapping(address);
        match inspection.drivers.len() {
            1 => {}
            0 => {
                return Some(format!(
                    "{address:04X}h is unmapped; Microsoft 4K BASIC requires one RAM responder at every address from 0000h through 0FFFh"
                ));
            }
            count => {
                return Some(format!(
                    "{address:04X}h is decoded by {count} RAM cards; Microsoft 4K BASIC requires exactly one responder throughout 0000h..0FFFh"
                ));
            }
        }
    }
    None
}

fn bootstrap_matches(machine: &mut BackendHost, definition: BootstrapDefinition) -> bool {
    definition
        .resolved_bytes()
        .iter()
        .enumerate()
        .all(|(address, expected)| {
            let inspection = machine.inspect_memory_mapping(address as u16);
            matches!(inspection.drivers.as_slice(), [driver] if driver.value == *expected)
        })
}

fn install_via_front_panel(
    machine: &mut BackendHost,
    definition: BootstrapDefinition,
) -> Result<Vec<String>, String> {
    require_panel_entry_ready(machine)?;
    if let Some(issue) = low_4k_mapping_issue(machine) {
        return Err(format!(
            "Microsoft 4K BASIC 3.2 authentic loading requires a uniquely mapped low 4 KiB S-100 RAM window: {issue}."
        ));
    }

    machine.front_panel_reset();
    machine.set_switch_register(0x0000);
    machine.examine(false);
    if machine.front_panel_state().address != 0 {
        return Err("EXAMINE 0000h did not place the front panel at address 0000h.".into());
    }

    let bytes = definition.resolved_bytes();
    let mut log = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().copied().enumerate() {
        machine.set_switch_register(u16::from(byte));
        machine.deposit(index != 0);

        let address = index as u16;
        let inspection = machine.inspect_memory_mapping(address);
        match inspection.drivers.as_slice() {
            [] => {
                return Err(format!(
                    "Front-panel DEPOSIT bus cycle executed at {address:04X}h, but no RAM card decoded it."
                ));
            }
            [driver] if driver.value != byte => {
                return Err(format!(
                    "Front-panel DEPOSIT at {address:04X}h placed {byte:02X}h on the bus, but Slot {:02} contains {:02X}h{}.",
                    driver.slot,
                    driver.value,
                    if driver.protected {
                        " (card/protection state blocked the write)"
                    } else {
                        ""
                    },
                ));
            }
            [driver] => log.push(format!(
                "{address:04X}h / {address:03o}o  ←  {byte:02X}h / {byte:03o}o  · Slot {:02}",
                driver.slot
            )),
            drivers => {
                return Err(format!(
                    "Front-panel DEPOSIT bus cycle executed at {address:04X}h, but {} RAM cards decoded the address; the operator did not choose a card.",
                    drivers.len()
                ));
            }
        }
    }

    Ok(log)
}

fn grouped_binary16(value: u16) -> String {
    format!(
        "{:04b} {:04b} {:04b} {:04b}",
        (value >> 12) & 0x0F,
        (value >> 8) & 0x0F,
        (value >> 4) & 0x0F,
        value & 0x0F
    )
}

fn bootstrap_instruction_info(
    definition: BootstrapDefinition,
    index: usize,
) -> Option<BootstrapInstructionInfo> {
    let table = match definition.board {
        SerialBoard::Sio88 => SIO_BOOTSTRAP_INSTRUCTIONS,
        SerialBoard::TwoSio88 => TWO_SIO_BOOTSTRAP_INSTRUCTIONS,
    };
    table
        .iter()
        .copied()
        .find(|info| index >= info.start && index < info.start + info.len)
}

fn bootstrap_instruction_text(
    definition: BootstrapDefinition,
    info: BootstrapInstructionInfo,
) -> (String, String) {
    if definition.board == SerialBoard::TwoSio88 {
        match info.start {
            0x02 | 0x06 => {
                return (
                    format!("OUT ${:02X}", definition.status_port),
                    format!(
                        "Writes A to the installed 88-2SIO Port 0 control register at {:02X}h.",
                        definition.status_port
                    ),
                );
            }
            0x0E => {
                return (
                    format!("IN ${:02X}", definition.status_port),
                    format!(
                        "Reads the installed 88-2SIO Port 0 status register at {:02X}h into A.",
                        definition.status_port
                    ),
                );
            }
            0x12 => {
                return (
                    format!("IN ${:02X}", definition.data_port),
                    format!(
                        "Reads one byte from the installed 88-2SIO Port 0 data register at {:02X}h into A.",
                        definition.data_port
                    ),
                );
            }
            _ => {}
        }
    }
    (info.mnemonic.to_owned(), info.effect.to_owned())
}

fn bootstrap_switch_tooltip(
    definition: BootstrapDefinition,
    index: usize,
    byte: u8,
) -> String {
    let address = index as u16;
    let switch_value = u16::from(byte);
    let Some(info) = bootstrap_instruction_info(definition, index) else {
        return format!(
            "Configure A15..A0 = {switch_value:04X}h / {switch_value:06o}o\n{}\n\nThis puts byte {byte:02X}h on the data switches for address {address:04X}h. Config switches only moves the physical switches; it does not deposit or execute anything.",
            grouped_binary16(switch_value)
        );
    };
    let (mnemonic, effect) = bootstrap_instruction_text(definition, info);

    let row_role = if info.mnemonic.starts_with("STACK RETURN VECTOR") {
        if index == info.start {
            "Low byte of the stack return-vector data"
        } else {
            "High byte of the stack return-vector data"
        }
        .to_owned()
    } else if index == info.start {
        "Opcode byte — this is where the 8080 instruction begins".to_owned()
    } else {
        format!(
            "Operand byte {} of {} for the instruction beginning at {:04X}h",
            index - info.start,
            info.len - 1,
            info.start
        )
    };

    format!(
        "Configure A15..A0 = {switch_value:04X}h / {switch_value:06o}o\n{}\n\n8080 / bootstrap meaning\n{mnemonic}\n{row_role}\n\nWhat it does: {effect}\nWhy the loader needs it: {}\n\nConfig switches only moves the front-panel switches. The byte is not deposited and the instruction is not executed until the corresponding panel operations and later RUN occur.",
        grouped_binary16(switch_value),
        info.purpose
    )
}

impl RusTairApp {
    pub(in crate::app) fn open_authentic_basic_loader(&mut self) {
        self.authentic_loader.window_open = true;
        self.status =
            "Authentic BASIC 3.2 loader opened — BASIC will not be copied directly into RAM"
                .into();
    }

    fn configure_bootstrap_switches(&mut self, value: u16, description: &str) {
        self.machine.set_switch_register(value);
        self.status = format!(
            "Operator: switches configured to {value:04X}h — {description}; no panel operation executed yet"
        );
    }

    fn execute_bootstrap_examine(&mut self, address: u16) -> Result<(), String> {
        require_panel_entry_ready(&mut self.machine)?;
        let switches = self.machine.switch_register();
        if switches != address {
            return Err(format!(
                "Switches are {switches:04X}h, but EXAMINE requires {address:04X}h. Configure the switches first."
            ));
        }
        self.machine.examine(false);
        let actual = self.machine.front_panel_state().address;
        if actual != address {
            return Err(format!(
                "EXAMINE expected {address:04X}h but the front panel stopped at {actual:04X}h."
            ));
        }
        self.status =
            format!("Operator: EXAMINE {address:04X}h executed on the real front-panel path");
        Ok(())
    }

    fn execute_bootstrap_deposit(
        &mut self,
        address: u16,
        byte: u8,
        deposit_next: bool,
    ) -> Result<(), String> {
        require_panel_entry_ready(&mut self.machine)?;
        let switches = self.machine.switch_register();
        if switches != u16::from(byte) {
            return Err(format!(
                "Data switches are {switches:04X}h, but this row requires {byte:02X}h with A15..A8 DOWN. Configure the switches first."
            ));
        }

        let panel_address = self.machine.front_panel_state().address;
        let required_before = if deposit_next {
            address.wrapping_sub(1)
        } else {
            address
        };
        if panel_address != required_before {
            return Err(format!(
                "{} for {address:04X}h expects the panel address to be {required_before:04X}h first; it is currently {panel_address:04X}h. Execute the preceding rows instead of silently repositioning the panel.",
                if deposit_next { "DEPOSIT NEXT" } else { "DEPOSIT" }
            ));
        }

        self.machine.deposit(deposit_next);
        let inspection = self.machine.inspect_memory_mapping(address);
        let operation = if deposit_next { "DEPOSIT NEXT" } else { "DEPOSIT" };
        match inspection.drivers.as_slice() {
            [] => Err(format!(
                "{operation} bus cycle executed at {address:04X}h with {byte:02X}h, but no RAM card decoded the address."
            )),
            [driver] if driver.value != byte => Err(format!(
                "{operation} bus cycle executed at {address:04X}h with {byte:02X}h, but Slot {:02} now contains {:02X}h{}.",
                driver.slot,
                driver.value,
                if driver.protected {
                    " (card/protection state blocked the write)"
                } else {
                    ""
                },
            )),
            [driver] => {
                self.status = format!(
                    "Operator: {operation} stored {byte:02X}h at {address:04X}h in Slot {:02}",
                    driver.slot
                );
                Ok(())
            }
            drivers => Err(format!(
                "{operation} bus cycle executed at {address:04X}h with {byte:02X}h, but {} RAM cards decode that address. The operator did not choose one card.",
                drivers.len()
            )),
        }
    }

    fn arm_authentic_tape_reader(&mut self) -> Result<(), String> {
        let definition = BootstrapDefinition::for_installed(
            self.config.machine.serial_board,
            self.config.machine.two_sio_straps,
        );
        if let Some(issue) = low_4k_mapping_issue(&mut self.machine) {
            return Err(format!(
                "Microsoft 4K BASIC 3.2 requires a uniquely mapped low 4 KiB S-100 RAM window: {issue}."
            ));
        }
        if !bootstrap_matches(&mut self.machine, definition) {
            return Err("The installed board/strap configuration's BASIC 3.2 bootstrap is not verified at 0000h. Enter it manually or use Install bootstrap first.".into());
        }
        if self.tty.tape_input_total_len() == 0 {
            return Err("Mount a BASIC 3.2 paper-tape image first.".into());
        }
        if self.tty.mode != TtyMode::Line {
            return Err("Set the ASR-33 to LINE before starting the reader.".into());
        }
        if self.asr_connection() != SerialConnection::Port0 {
            return Err("Connect the ASR-33 to Port 0; the historical bootstrap reads the board's first port.".into());
        }
        if !self.machine.powered() {
            return Err("Power ON the Altair before starting the reader.".into());
        }
        let sense = (self.machine.switch_register() >> 8) as u8;
        if sense != definition.required_sense {
            return Err(format!(
                "Set sense switches A15..A8 to {:02X}h before starting the BASIC 3.2 reader; current value is {sense:02X}h.",
                definition.required_sense
            ));
        }

        self.asr33.reader_running = true;
        self.asr33.last_reader_tick = Instant::now()
            .checked_sub(self.asr33.reader_speed.char_time())
            .unwrap_or_else(Instant::now);
        self.audio.play_once("assets/click.mp3");
        Ok(())
    }

    fn authentic_stage_label(
        &mut self,
        definition: BootstrapDefinition,
        verified: bool,
        tape_position: usize,
        tape_total: usize,
        rx_len: usize,
    ) -> String {
        if !verified {
            return "Bootstrap not verified in RAM".into();
        }
        let cpu = self.machine.intel8080_state();
        if !self.machine.running() {
            return "Bootstrap verified · CPU stopped".into();
        }
        if definition.pc_is_polling(cpu.pc) {
            return if rx_len == 0 {
                format!(
                    "Bootstrap polling {:02X}h · waiting for next reader byte",
                    definition.status_port
                )
            } else {
                format!(
                    "Bootstrap has UART RX pending · next guest IN {:02X}h consumes it",
                    definition.data_port
                )
            };
        }
        if cpu.pc <= definition.last_address().saturating_add(1) {
            return format!("Bootstrap executing · PC {:04X}h", cpu.pc);
        }
        if (CHECKSUM_LOADER_START..=BASIC32_4K_END).contains(&cpu.pc) {
            return format!("Checksum loader executing · PC {:04X}h", cpu.pc);
        }
        if tape_total > 0 && tape_position >= tape_total {
            return format!("Paper tape reached end · guest PC {:04X}h", cpu.pc);
        }
        if tape_position > 0 {
            return format!("Tape/program load in progress · PC {:04X}h", cpu.pc);
        }
        format!("CPU running · PC {:04X}h", cpu.pc)
    }

    fn draw_authentic_loader_contents(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let definition = BootstrapDefinition::for_installed(
                        self.config.machine.serial_board,
                        self.config.machine.two_sio_straps,
                    );
                    let resolved_bootstrap = definition.resolved_bytes();
                    let panel = self.machine.front_panel_state();
                    let sense = (panel.switches >> 8) as u8;
                    let installed_ram = self.machine.installed_ram_bytes();
                    let low_4k_issue = low_4k_mapping_issue(&mut self.machine);
                    let ram_ok = low_4k_issue.is_none();
                    let bootstrap_verified = bootstrap_matches(&mut self.machine, definition);
                    let tape_total = self.tty.tape_input_total_len();
                    let tape_position = self.tty.tape_input_position();
                    let asr_port_ok = self.asr_connection() == SerialConnection::Port0;
                    let line_ok = self.tty.mode == TtyMode::Line;
                    let sense_ok = sense == definition.required_sense;
                    let rx_len = self.asr_serial_rx_len();
                    let stage = self.authentic_stage_label(
                        definition,
                        bootstrap_verified,
                        tape_position,
                        tape_total,
                        rx_len,
                    );

                    ui.strong(definition.name);
                    ui.small("Authentic path: the bootstrap executes on the emulated 8080 and consumes the mounted tape through the selected UART. No BASIC bytes are injected directly into RAM.");
                    ui.small("RAM qualification is based on the physical S-100 address map, not an aggregate capacity: every address in 0000h..0FFFh must have exactly one RAM responder.");
                    if definition.board == SerialBoard::TwoSio88 {
                        ui.small(format!(
                            "A2-A7 are currently strapped for {:02X}h-{:02X}h. The MITS 10h/11h bootstrap template is entered with its four immediate I/O operands resolved to {:02X}h/{:02X}h; no legacy-port alias is created.",
                            self.config.machine.two_sio_straps.address.base(),
                            self.config.machine.two_sio_straps.address.base() + 3,
                            definition.status_port,
                            definition.data_port,
                        ));
                    }
                    ui.add_space(6.0);

                    egui::CollapsingHeader::new("Machine / loader status")
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new("authentic-basic-status")
                                .num_columns(2)
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label("Serial board");
                                    ui.label(format!(
                                        "{} · status {:02X}h / data {:02X}h",
                                        definition.board.label(),
                                        definition.status_port,
                                        definition.data_port
                                    ));
                                    ui.end_row();

                                    ui.label("Installed S-100 RAM");
                                    ui.colored_label(
                                        if ram_ok {
                                            Color32::LIGHT_GREEN
                                        } else {
                                            Color32::LIGHT_RED
                                        },
                                        if let Some(issue) = &low_4k_issue {
                                            format!(
                                                "{installed_ram} bytes total · low 4 KiB invalid: {issue}"
                                            )
                                        } else {
                                            format!(
                                                "{installed_ram} bytes total · 0000h..0FFFh uniquely mapped"
                                            )
                                        },
                                    );
                                    ui.end_row();

                                    ui.label("ASR-33 cable");
                                    ui.colored_label(
                                        if asr_port_ok {
                                            Color32::LIGHT_GREEN
                                        } else {
                                            Color32::LIGHT_RED
                                        },
                                        if asr_port_ok {
                                            "Port 0 — correct"
                                        } else {
                                            "Must be connected to Port 0"
                                        },
                                    );
                                    ui.end_row();

                                    ui.label("ASR-33 mode");
                                    ui.colored_label(
                                        if line_ok {
                                            Color32::LIGHT_GREEN
                                        } else {
                                            Color32::LIGHT_RED
                                        },
                                        if line_ok { "LINE" } else { "Set to LINE" },
                                    );
                                    ui.end_row();

                                    ui.label("Sense switches A15..A8");
                                    ui.colored_label(
                                        if sense_ok {
                                            Color32::LIGHT_GREEN
                                        } else {
                                            Color32::YELLOW
                                        },
                                        format!(
                                            "current {sense:02X}h · required {:02X}h",
                                            definition.required_sense
                                        ),
                                    );
                                    ui.end_row();

                                    ui.label("Bootstrap RAM");
                                    ui.colored_label(
                                        if bootstrap_verified {
                                            Color32::LIGHT_GREEN
                                        } else {
                                            Color32::YELLOW
                                        },
                                        if bootstrap_verified {
                                            format!(
                                                "verified through unique physical RAM responders · {} bytes",
                                                resolved_bootstrap.len()
                                            )
                                        } else {
                                            "not installed / does not match current board straps / mapping is ambiguous".into()
                                        },
                                    );
                                    ui.end_row();

                                    ui.label("Checksum-loader destination");
                                    ui.label(format!(
                                        "{:04X}h..{:04X}h · loaded backwards by the bootstrap",
                                        CHECKSUM_LOADER_START, CHECKSUM_LOADER_END
                                    ));
                                    ui.end_row();

                                    ui.label("Paper tape");
                                    if tape_total == 0 {
                                        ui.colored_label(Color32::YELLOW, "not mounted");
                                    } else {
                                        let percent = 100.0 * tape_position as f32
                                            / tape_total.max(1) as f32;
                                        ui.label(format!(
                                            "{tape_position}/{tape_total} bytes ({percent:.1}%) · {}",
                                            self.asr33.reader_speed.label()
                                        ));
                                    }
                                    ui.end_row();

                                    ui.label("Guest UART RX");
                                    if rx_len == 0 {
                                        ui.label("RDR empty");
                                    } else {
                                        ui.colored_label(
                                            Color32::YELLOW,
                                            format!("{rx_len} byte(s) in/pending for guest UART RX"),
                                        );
                                    }
                                    ui.end_row();

                                    ui.label("Reader");
                                    ui.label(if self.asr_reader_motor_running() {
                                        "motor RUNNING"
                                    } else {
                                        "stopped / control not enabling motor"
                                    });
                                    ui.end_row();

                                    ui.label("Stage");
                                    ui.label(stage);
                                    ui.end_row();
                                });
                        });

                    egui::CollapsingHeader::new("Loader controls")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Put BASIC 3.2 tape…").clicked() {
                                    self.load_paper_tape();
                                }
                                if ui.button("Open ASR-33").clicked() {
                                    self.asr33.window_open = true;
                                }
                                if ui.button("Set ASR-33 LINE").clicked() {
                                    self.set_tty_mode(TtyMode::Line);
                                }
                                if ui.button("Install bootstrap via front panel").clicked() {
                                    match install_via_front_panel(&mut self.machine, definition) {
                                        Ok(log) => {
                                            self.authentic_loader.last_install_log = log;
                                            self.status = format!(
                                                "Authentic bootstrap installed via EXAMINE/DEPOSIT: {} bytes for status {:02X}h / data {:02X}h — now EXAMINE 0000h and set sense {:02X}h",
                                                resolved_bootstrap.len(),
                                                definition.status_port,
                                                definition.data_port,
                                                definition.required_sense
                                            );
                                        }
                                        Err(error) => self.report_load_error(error),
                                    }
                                }
                                if ui.button("Arm / start paper reader").clicked() {
                                    match self.arm_authentic_tape_reader() {
                                        Ok(()) => {
                                            self.status = format!(
                                                "ASR-33 paper reader switch started — {}; the physical reader motor is independent of the 8080 RUN latch",
                                                self.asr33.reader_speed.label()
                                            );
                                        }
                                        Err(error) => self.report_load_error(error),
                                    }
                                }
                                if ui.button("Front Panel Operator…").clicked() {
                                    self.open_standalone_front_panel_operator(ctx);
                                }
                            });

                            ui.small("Install bootstrap is the one-click assisted path. The row-by-row table below remains the didactic bootstrap path. The generic Front Panel Operator button now opens the single S-100-aware operator implementation; the older duplicate operator has been removed.");
                        });

                    egui::CollapsingHeader::new("Manual front-panel procedure")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.label("1. Power ON, STOP the machine, RESET, set the ASR-33 to LINE and connect it to Port 0.");
                            ui.label("2. Put all 16 switches DOWN and operate EXAMINE. Enter the first byte with switches A7..A0 and DEPOSIT; enter each following byte with DEPOSIT NEXT. For 88-2SIO the table below reflects the installed A2-A7 address straps.");
                            ui.label("3. Verify the loader if desired, then put all switches DOWN and EXAMINE 0000h again.");
                            ui.label(format!(
                                "4. Set A15..A8 to {:02X}h ({}).",
                                definition.required_sense,
                                match definition.board {
                                    SerialBoard::Sio88 =>
                                        "all sense switches down for 88-SIO rev. 1",
                                    SerialBoard::TwoSio88 =>
                                        "A11 up for 88-2SIO Port 0 with the ASR-33 two-stop-bit setting",
                                }
                            ));
                            ui.label(match definition.board {
                                SerialBoard::Sio88 => "5. Historical SIO sequence: start/arm the paper reader, then operate RUN. The reader transport is physical hardware and does not derive its motor state from the CPU RUN latch.",
                                SerialBoard::TwoSio88 => "5. Historical 2SIO sequence: operate RUN, then start the paper reader (or let 88-TYA Reader Control drive it through RTS when configured).",
                            });
                            ui.label("6. On 88-2SIO, a new character may start whenever the physical receive shift path is free even if RDR still contains an unread byte. If software falls behind, real MC6850 overrun semantics apply rather than hidden host flow control.");
                        });

                    egui::CollapsingHeader::new("Operator-assisted row-by-row bootstrap entry")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.small("For every row: first click Config switches and look at the physical panel; then click Execute. Execute never chooses a RAM card. After the physical bus cycle, host-side inspection reports the actual unique responder, an unmapped address, protection, or overlap.");

                            egui::ScrollArea::horizontal().show(ui, |ui| {
                                egui::Grid::new("authentic-bootstrap-operator-table")
                                    .num_columns(7)
                                    .striped(true)
                                    .spacing([10.0, 4.0])
                                    .show(ui, |ui| {
                                        ui.strong("Step");
                                        ui.strong("Address");
                                        ui.strong("Data");
                                        ui.strong("Hex");
                                        ui.strong("Panel operation");
                                        ui.strong("");
                                        ui.strong("");
                                        ui.end_row();

                                        ui.label("Prepare");
                                        ui.monospace("000");
                                        ui.monospace("---");
                                        ui.monospace("----");
                                        ui.label("EXAMINE");
                                        let config = ui.button("Config switches").on_hover_text(
                                            "Configure all A15..A0 switches DOWN to select bootstrap start address 0000h. No panel action is executed yet.",
                                        );
                                        if config.clicked() {
                                            self.configure_bootstrap_switches(
                                                0x0000,
                                                "bootstrap start address 0000h",
                                            );
                                        }
                                        if ui.button("Execute").clicked() {
                                            if let Err(error) = self.execute_bootstrap_examine(0) {
                                                self.report_load_error(error);
                                            }
                                        }
                                        ui.end_row();

                                        for (index, byte) in
                                            resolved_bootstrap.iter().copied().enumerate()
                                        {
                                            let address = index as u16;
                                            let inspection =
                                                self.machine.inspect_memory_mapping(address);
                                            let stored = matches!(
                                                inspection.drivers.as_slice(),
                                                [driver] if driver.value == byte
                                            );
                                            ui.colored_label(
                                                if stored {
                                                    Color32::LIGHT_GREEN
                                                } else {
                                                    ui.visuals().text_color()
                                                },
                                                format!("{}", index + 1),
                                            );
                                            ui.monospace(format!("{address:03o}"));
                                            ui.monospace(format!("{byte:03o}"));
                                            ui.monospace(format!("{byte:02X}"));
                                            ui.label(if index == 0 {
                                                "DEPOSIT"
                                            } else {
                                                "DEPOSIT NEXT"
                                            });
                                            let config = ui
                                                .button("Config switches")
                                                .on_hover_text(bootstrap_switch_tooltip(
                                                    definition,
                                                    index,
                                                    byte,
                                                ));
                                            if config.clicked() {
                                                self.configure_bootstrap_switches(
                                                    u16::from(byte),
                                                    &format!(
                                                        "data {byte:02X}h for address {address:04X}h"
                                                    ),
                                                );
                                            }
                                            if ui.button("Execute").clicked() {
                                                if let Err(error) = self.execute_bootstrap_deposit(
                                                    address,
                                                    byte,
                                                    index != 0,
                                                ) {
                                                    self.report_load_error(error);
                                                }
                                            }
                                            if stored {
                                                if let [driver] = inspection.drivers.as_slice() {
                                                    ui.small(format!("Slot {:02}", driver.slot));
                                                }
                                            }
                                            ui.end_row();
                                        }

                                        ui.label("Return");
                                        ui.monospace("000");
                                        ui.monospace("---");
                                        ui.monospace("----");
                                        ui.label("EXAMINE");
                                        let config = ui.button("Config switches").on_hover_text(
                                            "Return A15..A0 to 0000h so EXAMINE selects the bootstrap entry point before the sense switches and RUN are set.",
                                        );
                                        if config.clicked() {
                                            self.configure_bootstrap_switches(
                                                0,
                                                "return to bootstrap start address 0000h",
                                            );
                                        }
                                        if ui.button("Execute").clicked() {
                                            if let Err(error) = self.execute_bootstrap_examine(0) {
                                                self.report_load_error(error);
                                            }
                                        }
                                        ui.end_row();
                                    });
                            });

                            ui.small(format!(
                                "After Return/EXAMINE, set the sense byte to {:02X}h in A15..A8 before RUN. This remains manual because the sense switches are operator-visible hardware.",
                                definition.required_sense
                            ));
                        });

                    if !self.authentic_loader.last_install_log.is_empty() {
                        egui::CollapsingHeader::new("Last one-click assisted deposit log")
                            .default_open(false)
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(180.0)
                                    .show(ui, |ui| {
                                        for line in &self.authentic_loader.last_install_log {
                                            ui.monospace(line);
                                        }
                                    });
                            });
                    }
                });
        });
    }

    pub(in crate::app) fn draw_authentic_loader_window(&mut self, parent_ctx: &egui::Context) {
        if !self.authentic_loader.window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-authentic-basic-loader"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — Authentic Microsoft 4K BASIC 3.2 Loader")
                .with_inner_size([1120.0, 820.0])
                .with_min_inner_size([800.0, 520.0])
                .with_resizable(true),
            |loader_ctx, _class| {
                self.draw_authentic_loader_contents(loader_ctx);
                if loader_ctx.input(|input| input.viewport().close_requested()) {
                    self.authentic_loader.window_open = false;
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TwoSioAddressBlock;

    #[test]
    fn basic32_4k_bootstraps_keep_historical_leader_and_ports() {
        let sio = BootstrapDefinition::for_board(SerialBoard::Sio88);
        assert_eq!(sio.bytes.len(), 20);
        assert_eq!(&sio.bytes[0..3], &[0x21, 0xAE, 0x0F]);
        assert!(sio.bytes.windows(2).any(|bytes| bytes == [0xDB, 0x00]));
        assert!(sio.bytes.windows(2).any(|bytes| bytes == [0xDB, 0x01]));
        assert_eq!(sio.required_sense, 0x00);

        let two_sio = BootstrapDefinition::for_board(SerialBoard::TwoSio88);
        assert_eq!(two_sio.bytes.len(), 28);
        assert_eq!(
            &two_sio.bytes[0..8],
            &[0x3E, 0x03, 0xD3, 0x10, 0x3E, 0x11, 0xD3, 0x10]
        );
        assert_eq!(&two_sio.bytes[8..11], &[0x21, 0xAE, 0x0F]);
        assert!(two_sio
            .bytes
            .windows(2)
            .any(|bytes| bytes == [0xDB, 0x10]));
        assert!(two_sio
            .bytes
            .windows(2)
            .any(|bytes| bytes == [0xDB, 0x11]));
        assert_eq!(two_sio.required_sense, 0x08);
    }

    #[test]
    fn readdressed_two_sio_bootstrap_changes_only_physical_io_operands() {
        let straps = TwoSioStraps {
            address: TwoSioAddressBlock::try_new(0x44).unwrap(),
            ..TwoSioStraps::default()
        };
        let definition = BootstrapDefinition::for_installed(SerialBoard::TwoSio88, straps);
        let resolved = definition.resolved_bytes();
        assert_eq!(definition.status_port, 0x44);
        assert_eq!(definition.data_port, 0x45);
        assert_eq!(resolved[0x03], 0x44);
        assert_eq!(resolved[0x07], 0x44);
        assert_eq!(resolved[0x0F], 0x44);
        assert_eq!(resolved[0x13], 0x45);

        for (index, (&canonical, &actual)) in BASIC32_4K_88_2SIO
            .iter()
            .zip(resolved.iter())
            .enumerate()
        {
            if ![0x03, 0x07, 0x0F, 0x13].contains(&index) {
                assert_eq!(
                    actual, canonical,
                    "non-port bootstrap byte changed at {index:02X}h"
                );
            }
        }

        let out = bootstrap_switch_tooltip(definition, 2, resolved[2]);
        assert!(out.contains("OUT $44"));
        let input = bootstrap_switch_tooltip(definition, 18, resolved[18]);
        assert!(input.contains("IN $45"));
    }

    #[test]
    fn assisted_bootstrap_really_uses_front_panel_on_adaptive_cycle() {
        for board in [SerialBoard::Sio88, SerialBoard::TwoSio88] {
            let mut machine = BackendHost::default();
            machine.configure_memory(RamSize::K4, RamInit::Zeroed);
            machine.configure_serial_board(board);
            machine.power(true);
            machine.set_running(false);

            let definition = BootstrapDefinition::for_board(board);
            let log = install_via_front_panel(&mut machine, definition).unwrap();
            assert_eq!(log.len(), definition.bytes.len());
            assert!(bootstrap_matches(&mut machine, definition));
            assert_eq!(
                machine.front_panel_state().address,
                definition.last_address()
            );
            assert_eq!(machine.switch_register() & 0x00FF, 0x00);
        }
    }

    #[test]
    fn assisted_readdressed_bootstrap_deposits_actual_44h_45h_operands_on_adaptive_cycle() {
        let straps = TwoSioStraps {
            address: TwoSioAddressBlock::try_new(0x44).unwrap(),
            ..TwoSioStraps::default()
        };
        let definition = BootstrapDefinition::for_installed(SerialBoard::TwoSio88, straps);
        let mut machine = BackendHost::default();
        machine.configure_memory(RamSize::K4, RamInit::Zeroed);
        machine.configure_serial_board(SerialBoard::TwoSio88);
        machine.configure_two_sio_straps(straps);
        machine.power(true);
        machine.set_running(false);

        install_via_front_panel(&mut machine, definition).unwrap();
        assert!(bootstrap_matches(&mut machine, definition));
        assert_eq!(machine.peek_memory(0x0003), Some(0x44));
        assert_eq!(machine.peek_memory(0x0007), Some(0x44));
        assert_eq!(machine.peek_memory(0x000F), Some(0x44));
        assert_eq!(machine.peek_memory(0x0013), Some(0x45));
    }

    #[test]
    fn assisted_bootstrap_rejects_unsafe_machine_states_and_noncontiguous_low_4k() {
        let definition = BootstrapDefinition::for_board(SerialBoard::Sio88);
        let mut machine = BackendHost::default();
        machine.configure_memory(RamSize::K4, RamInit::Zeroed);

        let error = install_via_front_panel(&mut machine, definition).unwrap_err();
        assert!(error.contains("Power ON"));

        machine.power(true);
        machine.set_running(true);
        let error = install_via_front_panel(&mut machine, definition).unwrap_err();
        assert!(error.contains("STOP"));

        machine.set_running(false);
        machine.power(false);
        machine.configure_memory(RamSize::K1, RamInit::Zeroed);
        machine.power(true);
        let error = install_via_front_panel(&mut machine, definition).unwrap_err();
        assert!(error.contains("low 4 KiB"));
        assert!(error.contains("unmapped"));
    }

    #[test]
    fn bootstrap_consumes_reader_bytes_with_real_guest_in_on_adaptive_cycle() {
        for board in [SerialBoard::Sio88, SerialBoard::TwoSio88] {
            let mut machine = BackendHost::default();
            machine.configure_memory(RamSize::K4, RamInit::Zeroed);
            machine.configure_serial_board(board);
            machine.power(true);
            machine.set_running(false);

            let definition = BootstrapDefinition::for_board(board);
            install_via_front_panel(&mut machine, definition).unwrap();

            machine.set_switch_register(0x0000);
            machine.examine(false);
            machine.set_switch_register(u16::from(definition.required_sense) << 8);
            assert_eq!(machine.intel8080_state().pc, 0x0000);
            machine.set_running(true);

            for _ in 0..400 {
                machine.run_cycles(32);
                if definition.pc_is_polling(machine.intel8080_state().pc) {
                    break;
                }
            }
            assert!(definition.pc_is_polling(machine.intel8080_state().pc));

            machine.set_io_trace_enabled(true);
            let (_, _, leader_reads_before, _) =
                machine.io_port_activity(definition.data_port);
            machine.serial_receive(BackendSerialPort::Port0, 0xAE);
            if board == SerialBoard::TwoSio88 {
                assert!(!machine.serial_rx_empty(BackendSerialPort::Port0));
                assert_eq!(machine.peek_io_port(definition.status_port) & 0x01, 0);
            }
            for _ in 0..4_096 {
                machine.run_cycles(64);
                let (_, _, data_reads, _) =
                    machine.io_port_activity(definition.data_port);
                if data_reads > leader_reads_before {
                    break;
                }
            }
            let (_, _, leader_reads_after, _) =
                machine.io_port_activity(definition.data_port);
            assert!(
                leader_reads_after > leader_reads_before,
                "{board:?} never consumed the AEh leader through guest IN"
            );
            assert!(machine.serial_rx_empty(BackendSerialPort::Port0));
            assert_eq!(machine.peek_memory(CHECKSUM_LOADER_END), Some(0x00));

            let payload_reads_before = leader_reads_after;
            machine.serial_receive(BackendSerialPort::Port0, 0x42);
            if board == SerialBoard::TwoSio88 {
                assert!(!machine.serial_rx_empty(BackendSerialPort::Port0));
                assert_eq!(machine.peek_io_port(definition.status_port) & 0x01, 0);
            }
            for _ in 0..4_096 {
                machine.run_cycles(64);
                if machine.peek_memory(CHECKSUM_LOADER_END) == Some(0x42) {
                    break;
                }
            }
            assert_eq!(machine.peek_memory(CHECKSUM_LOADER_END), Some(0x42));
            let (_, _, payload_reads_after, _) =
                machine.io_port_activity(definition.data_port);
            assert!(
                payload_reads_after > payload_reads_before,
                "{board:?} stored the payload without a guest DATA-port IN"
            );
            assert!(machine.serial_rx_empty(BackendSerialPort::Port0));
        }
    }

    #[test]
    fn readdressed_bootstrap_executes_guest_io_on_44h_45h_not_legacy_10h_11h() {
        let straps = TwoSioStraps {
            address: TwoSioAddressBlock::try_new(0x44).unwrap(),
            ..TwoSioStraps::default()
        };
        let definition = BootstrapDefinition::for_installed(SerialBoard::TwoSio88, straps);
        let mut machine = BackendHost::default();
        machine.configure_memory(RamSize::K4, RamInit::Zeroed);
        machine.configure_serial_board(SerialBoard::TwoSio88);
        machine.configure_two_sio_straps(straps);
        machine.power(true);
        machine.set_running(false);
        install_via_front_panel(&mut machine, definition).unwrap();

        machine.set_switch_register(0x0000);
        machine.examine(false);
        machine.set_switch_register(u16::from(definition.required_sense) << 8);
        machine.set_io_trace_enabled(true);
        machine.set_running(true);

        for _ in 0..400 {
            machine.run_cycles(32);
            if definition.pc_is_polling(machine.intel8080_state().pc) {
                break;
            }
        }
        assert!(definition.pc_is_polling(machine.intel8080_state().pc));
        let (_, _, legacy_status_reads, _) = machine.io_port_activity(0x10);
        let (_, _, status_reads, _) = machine.io_port_activity(0x44);
        assert!(status_reads > 0, "readdressed bootstrap never polled 44h");
        assert_eq!(legacy_status_reads, 0, "readdressed bootstrap still touched legacy 10h");

        let (_, _, data_reads_before, _) = machine.io_port_activity(0x45);
        machine.serial_receive(BackendSerialPort::Port0, 0xAE);
        for _ in 0..4_096 {
            machine.run_cycles(64);
            let (_, _, data_reads, _) = machine.io_port_activity(0x45);
            if data_reads > data_reads_before {
                break;
            }
        }
        let (_, _, data_reads_after, _) = machine.io_port_activity(0x45);
        let (_, _, legacy_data_reads, _) = machine.io_port_activity(0x11);
        assert!(data_reads_after > data_reads_before, "readdressed bootstrap never read 45h");
        assert_eq!(legacy_data_reads, 0, "readdressed bootstrap still touched legacy 11h");
    }

    #[test]
    fn bootstrap_switch_tooltips_explain_opcode_operand_and_return_vector_roles() {
        let sio = BootstrapDefinition::for_board(SerialBoard::Sio88);
        let opcode = bootstrap_switch_tooltip(sio, 0, sio.bytes[0]);
        assert!(opcode.contains("LXI H,$0FAE"));
        assert!(opcode.contains("Opcode byte"));
        assert!(opcode.contains("Why the loader needs it"));

        let operand = bootstrap_switch_tooltip(sio, 1, sio.bytes[1]);
        assert!(operand.contains("Operand byte 1 of 2"));
        assert!(operand.contains("LXI H,$0FAE"));

        let return_vector = bootstrap_switch_tooltip(sio, 18, sio.bytes[18]);
        assert!(return_vector.contains("STACK RETURN VECTOR $0003"));
        assert!(return_vector.contains("stack return-vector data"));

        let two_sio = BootstrapDefinition::for_board(SerialBoard::TwoSio88);
        let out = bootstrap_switch_tooltip(two_sio, 2, two_sio.bytes[2]);
        assert!(out.contains("OUT $10"));
        assert!(out.contains("master-reset"));
    }
}

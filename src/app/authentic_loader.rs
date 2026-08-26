use super::*;

/// Front-panel bootstrap for Microsoft 4K BASIC 3.2 paper tape.
///
/// The loader bytes are the MITS front-panel bootstrap, not a RusTair helper
/// program. BASIC 3.2 uses leader/checksum-loader marker 256 octal (AEh); 4K
/// uses checksum-loader selector 017 octal. The 88-2SIO variant below uses the
/// historically appropriate two-stop-bit ACIA setup for an ASR-33.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BootstrapDefinition {
    pub(super) board: SerialBoard,
    pub(super) name: &'static str,
    pub(super) bytes: &'static [u8],
    pub(super) required_sense: u8,
    pub(super) status_port: u8,
    pub(super) data_port: u8,
    poll_start: u16,
    poll_end: u16,
}

const BASIC32_4K_MIN_RAM: usize = 4 * 1024;
const CHECKSUM_LOADER_START: u16 = 0x0F00;
const CHECKSUM_LOADER_END: u16 = 0x0FAD;

const BASIC32_4K_88_SIO: [u8; 20] = [
    0x21, 0xAE, 0x0F, 0x31, 0x12, 0x00, 0xDB, 0x00, 0x0F, 0xD8,
    0xDB, 0x01, 0xBD, 0xC8, 0x2D, 0x77, 0xC0, 0xE9, 0x03, 0x00,
];

const BASIC32_4K_88_2SIO: [u8; 28] = [
    0x3E, 0x03, 0xD3, 0x10, 0x3E, 0x11, 0xD3, 0x10, 0x21, 0xAE,
    0x0F, 0x31, 0x1A, 0x00, 0xDB, 0x10, 0x0F, 0xD0, 0xDB, 0x11,
    0xBD, 0xC8, 0x2D, 0x77, 0xC0, 0xE9, 0x0B, 0x00,
];

impl BootstrapDefinition {
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
    operator_window_open: bool,
    operator_source_name: String,
    operator_bytes: Vec<u8>,
    operator_base_address: u16,
    operator_base_text: String,
}

impl Default for AuthenticLoaderState {
    fn default() -> Self {
        Self {
            window_open: false,
            last_install_log: Vec::new(),
            operator_window_open: false,
            operator_source_name: String::new(),
            operator_bytes: Vec::new(),
            operator_base_address: 0,
            operator_base_text: "0000".into(),
        }
    }
}

fn bootstrap_matches(machine: &mut BackendHost, definition: BootstrapDefinition) -> bool {
    definition
        .bytes
        .iter()
        .enumerate()
        .all(|(address, expected)| machine.peek_memory(address as u16) == Some(*expected))
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

fn install_via_front_panel(
    machine: &mut BackendHost,
    definition: BootstrapDefinition,
) -> Result<Vec<String>, String> {
    require_panel_entry_ready(machine)?;
    if machine.installed_ram_bytes() < BASIC32_4K_MIN_RAM {
        return Err(format!(
            "Microsoft 4K BASIC 3.2 authentic loading requires at least 4 KiB RAM; the current machine has {} bytes.",
            machine.installed_ram_bytes()
        ));
    }

    machine.front_panel_reset();
    machine.set_switch_register(0x0000);
    machine.examine(false);
    if machine.front_panel_state().address != 0 {
        return Err("EXAMINE 0000h did not place the front panel at address 0000h.".into());
    }

    let mut log = Vec::with_capacity(definition.bytes.len());
    for (index, byte) in definition.bytes.iter().copied().enumerate() {
        machine.set_switch_register(u16::from(byte));
        machine.deposit(index != 0);

        let address = index as u16;
        let observed = machine.peek_memory(address);
        if observed != Some(byte) {
            return Err(format!(
                "Front-panel deposit failed at {address:04X}h: entered {byte:02X}h, read back {}.",
                observed
                    .map(|value| format!("{value:02X}h"))
                    .unwrap_or_else(|| "unmapped memory".into())
            ));
        }
        log.push(format!(
            "{address:04X}h / {address:03o}o  ←  {byte:02X}h / {byte:03o}o"
        ));
    }

    Ok(log)
}

fn strip_hex_affixes(text: &str) -> &str {
    let trimmed = text.trim();
    let trimmed = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    trimmed
        .strip_suffix('h')
        .or_else(|| trimmed.strip_suffix('H'))
        .unwrap_or(trimmed)
}

fn parse_hex_address(text: &str) -> Option<u16> {
    let trimmed = strip_hex_affixes(text);
    (!trimmed.is_empty())
        .then(|| u16::from_str_radix(trimmed, 16).ok())
        .flatten()
}

fn operator_target_address(base: u16, index: usize) -> Option<u16> {
    let offset = u16::try_from(index).ok()?;
    base.checked_add(offset)
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
        "Configure A15..A0 = {switch_value:04X}h / {switch_value:06o}o\n{}\n\n8080 / bootstrap meaning\n{}\n{row_role}\n\nWhat it does: {}\nWhy the loader needs it: {}\n\nConfig switches only moves the front-panel switches. The byte is not deposited and the instruction is not executed until the corresponding panel operations and later RUN occur.",
        grouped_binary16(switch_value),
        info.mnemonic,
        info.effect,
        info.purpose
    )
}

fn examine_switch_tooltip(address: u16, purpose: &str) -> String {
    format!(
        "Configure A15..A0 = {address:04X}h / {address:06o}o\n{}\n\nFront-panel operation: EXAMINE\nEXAMINE is not an 8080 instruction. It tells the Altair front panel which memory address the operator wants to inspect/select.\n\nPurpose here: {purpose}\n\nConfig switches only positions the switches; Execute performs EXAMINE.",
        grouped_binary16(address)
    )
}

fn generic_data_switch_tooltip(address: u16, byte: u8) -> String {
    let value = u16::from(byte);
    format!(
        "Configure A15..A0 = {value:04X}h / {value:06o}o\n{}\n\nThis prepares data byte {byte:02X}h for address {address:04X}h. A15..A8 stay DOWN and A7..A0 represent the byte.\n\nThe generic operator intentionally does not guess whether an arbitrary byte is an opcode or an operand, because data and code are indistinguishable without instruction-boundary context. Config switches only moves the panel switches; Execute performs DEPOSIT/DEPOSIT NEXT.",
        grouped_binary16(value)
    )
}

impl RusTairApp {
    pub(in crate::app) fn open_authentic_basic_loader(&mut self) {
        self.authentic_loader.window_open = true;
        self.status =
            "Authentic BASIC 3.2 loader opened — BASIC will not be copied directly into RAM"
                .into();
    }

    fn open_front_panel_operator(&mut self) {
        self.authentic_loader.operator_window_open = true;
        self.status =
            "Front Panel Operator opened — switch configuration and panel actions are manual/visible"
                .into();
    }

    fn load_front_panel_operator_source(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Binary / paper tape", &["bin", "tap", "rom", "com"])
            .pick_file()
        else {
            return;
        };

        match std::fs::read(&path) {
            Ok(bytes) if bytes.is_empty() => {
                self.report_load_error(format!(
                    "{} is empty; there are no bytes to enter from the front panel.",
                    path.display()
                ));
            }
            Ok(bytes) => {
                self.authentic_loader.operator_source_name = path.display().to_string();
                self.authentic_loader.operator_bytes = bytes;
                self.authentic_loader.operator_window_open = true;
                self.status = format!(
                    "Front Panel Operator source loaded: {} bytes from {}",
                    self.authentic_loader.operator_bytes.len(),
                    path.display()
                );
            }
            Err(error) => self.report_load_error(format!(
                "Could not read operator source {}: {error}",
                path.display()
            )),
        }
    }

    fn configure_operator_switches(&mut self, value: u16, description: &str) {
        self.machine.set_switch_register(value);
        self.status = format!(
            "Operator: switches configured to {value:04X}h — {description}; no panel operation executed yet"
        );
    }

    fn execute_operator_examine(&mut self, address: u16) -> Result<(), String> {
        require_panel_entry_ready(&mut self.machine)?;
        let switches = self.machine.switch_register();
        if switches != address {
            return Err(format!(
                "Switches are {switches:04X}h, but EXAMINE step requires {address:04X}h. Press Config switches first."
            ));
        }
        self.machine.examine(false);
        let actual = self.machine.front_panel_state().address;
        if actual != address {
            return Err(format!(
                "EXAMINE expected {address:04X}h but the front panel stopped at {actual:04X}h."
            ));
        }
        self.status = format!("Operator: EXAMINE {address:04X}h executed on the real front-panel path");
        Ok(())
    }

    fn execute_operator_deposit(
        &mut self,
        address: u16,
        byte: u8,
        deposit_next: bool,
    ) -> Result<(), String> {
        require_panel_entry_ready(&mut self.machine)?;

        let switches = self.machine.switch_register();
        if switches != u16::from(byte) {
            return Err(format!(
                "Data switches are {switches:04X}h, but this step requires {byte:02X}h with A15..A8 DOWN. Press Config switches first."
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
                "{} for {address:04X}h expects the panel address to be {required_before:04X}h first; it is currently {panel_address:04X}h. Execute the preceding steps instead of silently repositioning the panel.",
                if deposit_next { "DEPOSIT NEXT" } else { "DEPOSIT" }
            ));
        }

        if usize::from(address) >= self.machine.installed_ram_bytes() {
            return Err(format!(
                "Address {address:04X}h is outside the currently installed {} bytes of RAM.",
                self.machine.installed_ram_bytes()
            ));
        }

        self.machine.deposit(deposit_next);
        let observed = self.machine.peek_memory(address);
        if observed != Some(byte) {
            return Err(format!(
                "{} at {address:04X}h did not store {byte:02X}h; read-back is {}.",
                if deposit_next { "DEPOSIT NEXT" } else { "DEPOSIT" },
                observed
                    .map(|value| format!("{value:02X}h"))
                    .unwrap_or_else(|| "unmapped".into())
            ));
        }

        self.status = format!(
            "Operator: {} stored {byte:02X}h at {address:04X}h",
            if deposit_next { "DEPOSIT NEXT" } else { "DEPOSIT" }
        );
        Ok(())
    }

    fn arm_authentic_tape_reader(&mut self) -> Result<(), String> {
        let definition = BootstrapDefinition::for_board(self.config.machine.serial_board);
        if self.machine.installed_ram_bytes() < BASIC32_4K_MIN_RAM {
            return Err(format!(
                "Microsoft 4K BASIC 3.2 requires at least 4 KiB RAM; the current machine has {}.",
                self.config.machine.ram_size.label()
            ));
        }
        if !bootstrap_matches(&mut self.machine, definition) {
            return Err("The selected board's BASIC 3.2 bootstrap is not verified at 0000h. Enter it manually or use Install bootstrap first.".into());
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
        if (CHECKSUM_LOADER_START..=0x0FFF).contains(&cpu.pc) {
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
                    let definition =
                        BootstrapDefinition::for_board(self.config.machine.serial_board);
                    let panel = self.machine.front_panel_state();
                    let sense = (panel.switches >> 8) as u8;
                    let installed_ram = self.machine.installed_ram_bytes();
                    let ram_ok = installed_ram >= BASIC32_4K_MIN_RAM;
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

                                    ui.label("Installed RAM");
                                    ui.colored_label(
                                        if ram_ok {
                                            Color32::LIGHT_GREEN
                                        } else {
                                            Color32::LIGHT_RED
                                        },
                                        format!(
                                            "{} · {}",
                                            self.config.machine.ram_size.label(),
                                            if ram_ok {
                                                "4K BASIC requirement met"
                                            } else {
                                                "requires at least 4 KiB"
                                            }
                                        ),
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
                                            format!("verified · {} bytes", definition.bytes.len())
                                        } else {
                                            "not installed / does not match".into()
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
                                        ui.label("empty · reader may present the next byte");
                                    } else {
                                        ui.colored_label(
                                            Color32::YELLOW,
                                            format!(
                                                "{rx_len} byte(s) pending · WAIT GUEST RX until guest IN consumes data"
                                            ),
                                        );
                                    }
                                    ui.end_row();

                                    ui.label("Reader");
                                    ui.label(if self.asr33.reader_running {
                                        if self.machine.running() {
                                            "READING / guest-paced"
                                        } else {
                                            "ARMED · waiting for RUN"
                                        }
                                    } else {
                                        "stopped"
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
                                                "Authentic bootstrap installed via EXAMINE/DEPOSIT: {} bytes — now EXAMINE 0000h and set sense {:02X}h",
                                                definition.bytes.len(),
                                                definition.required_sense
                                            );
                                        }
                                        Err(error) => self.report_load_error(error),
                                    }
                                }
                                if ui.button("Arm / start paper reader").clicked() {
                                    match self.arm_authentic_tape_reader() {
                                        Ok(()) => {
                                            self.status = if self.machine.running() {
                                                format!(
                                                    "ASR-33 paper reader started — {}",
                                                    self.asr33.reader_speed.label()
                                                )
                                            } else {
                                                "ASR-33 paper reader armed — it will not advance until the Altair RUN latch is on".into()
                                            };
                                        }
                                        Err(error) => self.report_load_error(error),
                                    }
                                }
                                if ui.button("Front Panel Operator…").clicked() {
                                    self.open_front_panel_operator();
                                }
                            });

                            ui.small("Install bootstrap is the one-click assisted path. The operator table below is the didactic path: Config switches only moves the real front-panel switches; Execute then performs the actual EXAMINE / DEPOSIT / DEPOSIT NEXT operation. Watch the main Altair panel while using it.");
                        });

                    egui::CollapsingHeader::new("Manual front-panel procedure")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.label("1. Power ON, STOP the machine, RESET, set the ASR-33 to LINE and connect it to Port 0.");
                            ui.label("2. Put all 16 switches DOWN and operate EXAMINE. Enter the first byte with switches A7..A0 and DEPOSIT; enter each following byte with DEPOSIT NEXT.");
                            ui.label("3. Verify the loader if desired, then put all switches DOWN and EXAMINE 0000h again.");
                            ui.label(format!(
                                "4. Set A15..A8 to {:02X}h ({}) before loading BASIC 3.2.",
                                definition.required_sense,
                                match definition.board {
                                    SerialBoard::Sio88 =>
                                        "all sense switches down for 88-SIO rev. 1",
                                    SerialBoard::TwoSio88 =>
                                        "A11 up for 88-2SIO Port 0 with the ASR-33 two-stop-bit setting",
                                }
                            ));
                            ui.label(match definition.board {
                                SerialBoard::Sio88 => "5. Historical SIO sequence: start/arm the paper reader, then operate RUN. RusTair will keep the tape stationary until RUN is actually active.",
                                SerialBoard::TwoSio88 => "5. Historical 2SIO sequence: operate RUN, then start the paper reader.",
                            });
                            ui.label("6. The reader advances only when the guest UART can accept another byte. WAIT GUEST RX therefore means the bootstrap/checksum loader has not yet consumed the previous byte with a real IN instruction.");
                        });

                    egui::CollapsingHeader::new("Operator-assisted row-by-row bootstrap entry")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.small("For every row: first click Config switches and look at the physical panel; then click Execute. Hover Config switches to see which 8080 instruction that byte belongs to, what the instruction does, and why it exists in the bootstrap. Execute refuses to silently fix a wrong address or switch value.");

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
                                        let config = ui
                                            .button("Config switches")
                                            .on_hover_text(examine_switch_tooltip(
                                                0x0000,
                                                "Select bootstrap start address 0000h before the first DEPOSIT.",
                                            ));
                                        if config.clicked() {
                                            self.configure_operator_switches(
                                                0x0000,
                                                "bootstrap start address 0000h",
                                            );
                                        }
                                        if ui.button("Execute").clicked() {
                                            if let Err(error) =
                                                self.execute_operator_examine(0x0000)
                                            {
                                                self.report_load_error(error);
                                            }
                                        }
                                        ui.end_row();

                                        for (index, byte) in
                                            definition.bytes.iter().copied().enumerate()
                                        {
                                            let address = index as u16;
                                            let stored =
                                                self.machine.peek_memory(address) == Some(byte);
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
                                                self.configure_operator_switches(
                                                    u16::from(byte),
                                                    &format!(
                                                        "data {byte:02X}h for address {address:04X}h"
                                                    ),
                                                );
                                            }
                                            if ui.button("Execute").clicked() {
                                                if let Err(error) =
                                                    self.execute_operator_deposit(
                                                        address,
                                                        byte,
                                                        index != 0,
                                                    )
                                                {
                                                    self.report_load_error(error);
                                                }
                                            }
                                            ui.end_row();
                                        }

                                        ui.label("Return");
                                        ui.monospace("000");
                                        ui.monospace("---");
                                        ui.monospace("----");
                                        ui.label("EXAMINE");
                                        let config = ui
                                            .button("Config switches")
                                            .on_hover_text(examine_switch_tooltip(
                                                0x0000,
                                                "Return the front-panel address to bootstrap entry point 0000h before setting the sense switches and RUN.",
                                            ));
                                        if config.clicked() {
                                            self.configure_operator_switches(
                                                0x0000,
                                                "return to bootstrap start address 0000h",
                                            );
                                        }
                                        if ui.button("Execute").clicked() {
                                            if let Err(error) =
                                                self.execute_operator_examine(0x0000)
                                            {
                                                self.report_load_error(error);
                                            }
                                        }
                                        ui.end_row();
                                    });
                            });

                            ui.small(format!(
                                "After the Return/EXAMINE row, set the sense byte to {:02X}h in A15..A8 before RUN. This is deliberately not done automatically because the sense switches are part of the operator-visible machine configuration.",
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

    fn draw_front_panel_operator_contents(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("front-panel-operator-toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Load binary / tape…").clicked() {
                    self.load_front_panel_operator_source();
                }
                ui.separator();
                ui.label("Base address:");
                let response = ui.add_sized(
                    [86.0, 24.0],
                    egui::TextEdit::singleline(
                        &mut self.authentic_loader.operator_base_text,
                    )
                    .font(egui::TextStyle::Monospace),
                );
                let enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ui.button("Apply").clicked() || enter {
                    match parse_hex_address(&self.authentic_loader.operator_base_text) {
                        Some(address) => {
                            self.authentic_loader.operator_base_address = address;
                            self.authentic_loader.operator_base_text =
                                format!("{address:04X}");
                            self.status =
                                format!("Front Panel Operator base address: {address:04X}h");
                        }
                        None => self.report_load_error(
                            "Front Panel Operator base address must be hexadecimal, e.g. 0000 or 0x0100.",
                        ),
                    }
                }
                ui.separator();
                if self.authentic_loader.operator_source_name.is_empty() {
                    ui.label("No source loaded");
                } else {
                    ui.label(format!(
                        "{} · {} bytes",
                        self.authentic_loader.operator_source_name,
                        self.authentic_loader.operator_bytes.len()
                    ));
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.strong("Front Panel Operator");
            ui.small("Teaching mode for entering an arbitrary byte image exactly as an Altair operator would: configure switches, then perform EXAMINE / DEPOSIT / DEPOSIT NEXT. Nothing is copied directly into RAM.");
            ui.small("A .bin/.rom/.com file is treated as sequential bytes at the selected base address. A .tap file is intentionally treated as its raw tape bytes here; that is useful for studying/entering bytes but is not equivalent to authentic serial paper-tape loading. For authentic tape transport use the ASR-33 and a real bootstrap.");
            ui.add_space(6.0);

            let base = self.authentic_loader.operator_base_address;
            let byte_count = self.authentic_loader.operator_bytes.len();
            let last_address = byte_count
                .checked_sub(1)
                .and_then(|index| operator_target_address(base, index));

            egui::CollapsingHeader::new("Image / machine summary")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("front-panel-operator-summary")
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Base");
                            ui.monospace(format!("{base:04X}h / {base:06o}o"));
                            ui.end_row();
                            ui.label("Bytes");
                            ui.label(byte_count.to_string());
                            ui.end_row();
                            ui.label("Range");
                            ui.label(match last_address {
                                Some(last) => format!("{base:04X}h..{last:04X}h"),
                                None if byte_count == 0 => "—".into(),
                                None => "wraps past FFFFh — invalid".into(),
                            });
                            ui.end_row();
                            ui.label("Machine");
                            ui.label(format!(
                                "{} · {}",
                                if self.machine.powered() {
                                    "POWER ON"
                                } else {
                                    "POWER OFF"
                                },
                                if self.machine.running() {
                                    "RUNNING"
                                } else {
                                    "STOPPED"
                                }
                            ));
                            ui.end_row();
                        });
                });

            egui::CollapsingHeader::new("Initial address — EXAMINE")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("Initial address");
                        ui.monospace(format!("{base:04X}h"));
                        let config = ui
                            .button("Config switches")
                            .on_hover_text(examine_switch_tooltip(
                                base,
                                "Select the program's base address before depositing its first byte.",
                            ));
                        if config.clicked() {
                            self.configure_operator_switches(base, "program base address");
                        }
                        if ui.button("Execute").clicked() {
                            if let Err(error) = self.execute_operator_examine(base) {
                                self.report_load_error(error);
                            }
                        }
                        ui.label("EXAMINE");
                    });
                });

            if byte_count == 0 {
                ui.label("Load a binary or tape image to create the operator sequence.");
                return;
            }
            if last_address.is_none() {
                ui.colored_label(
                    Color32::LIGHT_RED,
                    "The selected base + file length exceeds FFFFh. Choose a lower base or a smaller image.",
                );
                return;
            }

            egui::CollapsingHeader::new("Byte-by-byte entry")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("Address");
                        ui.add_space(28.0);
                        ui.strong("Octal");
                        ui.add_space(24.0);
                        ui.strong("Data");
                        ui.add_space(20.0);
                        ui.strong("Operation");
                        ui.add_space(40.0);
                        ui.strong("Operator controls");
                    });

                    let row_height = 27.0;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(430.0)
                        .show_rows(ui, row_height, byte_count, |ui, row_range| {
                            for index in row_range {
                                let byte = self.authentic_loader.operator_bytes[index];
                                let Some(address) = operator_target_address(base, index) else {
                                    continue;
                                };
                                let stored = self.machine.peek_memory(address) == Some(byte);
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        if stored {
                                            Color32::LIGHT_GREEN
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                        egui::RichText::new(format!("{address:04X}"))
                                            .monospace(),
                                    );
                                    ui.add_sized(
                                        [70.0, 18.0],
                                        egui::Label::new(
                                            egui::RichText::new(format!("{address:06o}"))
                                                .monospace(),
                                        ),
                                    );
                                    ui.add_sized(
                                        [48.0, 18.0],
                                        egui::Label::new(
                                            egui::RichText::new(format!("{byte:02X}"))
                                                .monospace(),
                                        ),
                                    );
                                    ui.add_sized(
                                        [94.0, 18.0],
                                        egui::Label::new(if index == 0 {
                                            "DEPOSIT"
                                        } else {
                                            "DEPOSIT NEXT"
                                        }),
                                    );
                                    let config = ui
                                        .button("Config switches")
                                        .on_hover_text(generic_data_switch_tooltip(
                                            address, byte,
                                        ));
                                    if config.clicked() {
                                        self.configure_operator_switches(
                                            u16::from(byte),
                                            &format!(
                                                "data {byte:02X}h for address {address:04X}h"
                                            ),
                                        );
                                    }
                                    if ui.button("Execute").clicked() {
                                        if let Err(error) = self.execute_operator_deposit(
                                            address,
                                            byte,
                                            index != 0,
                                        ) {
                                            self.report_load_error(error);
                                        }
                                    }
                                    if stored {
                                        ui.small("stored");
                                    }
                                });
                            }
                        });

                    ui.small("Green rows already match RAM. That is only a read-back indicator: Execute still uses the real panel operation and will never silently jump to the row's address.");
                });

            egui::CollapsingHeader::new("Run from base")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong("Run from base:");
                        let config = ui
                            .button("Config switches")
                            .on_hover_text(examine_switch_tooltip(
                                base,
                                "Select the program entry address so EXAMINE places the front panel/PC at the point from which RUN should begin.",
                            ));
                        if config.clicked() {
                            self.configure_operator_switches(base, "program start address");
                        }
                        if ui.button("Execute EXAMINE").clicked() {
                            if let Err(error) = self.execute_operator_examine(base) {
                                self.report_load_error(error);
                            }
                        }
                        if ui
                            .add_enabled(
                                self.machine.powered() && !self.machine.running(),
                                egui::Button::new("RUN"),
                            )
                            .clicked()
                        {
                            self.machine.set_running(true);
                            self.status = format!(
                                "Operator: RUN latch enabled from front-panel address {base:04X}h"
                            );
                        }
                    });
                });
        });
    }

    fn show_front_panel_operator_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.authentic_loader.operator_window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-front-panel-operator"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — Front Panel Operator")
                .with_inner_size([1040.0, 760.0])
                .with_min_inner_size([760.0, 460.0])
                .with_resizable(true),
            |operator_ctx, _class| {
                self.draw_front_panel_operator_contents(operator_ctx);
                if operator_ctx.input(|input| input.viewport().close_requested()) {
                    self.authentic_loader.operator_window_open = false;
                }
            },
        );
    }

    pub(in crate::app) fn draw_authentic_loader_window(&mut self, parent_ctx: &egui::Context) {
        if self.authentic_loader.window_open {
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

        self.show_front_panel_operator_viewport(parent_ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn assisted_bootstrap_really_uses_front_panel_on_both_rust_engines() {
        for engine in [
            EmulationEngine::RustFast8080,
            EmulationEngine::RustCycleAccurate8080,
        ] {
            for board in [SerialBoard::Sio88, SerialBoard::TwoSio88] {
                let mut machine = BackendHost::from_engine(engine).unwrap();
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
    }

    #[test]
    fn assisted_bootstrap_rejects_unsafe_machine_states_and_too_little_ram() {
        let definition = BootstrapDefinition::for_board(SerialBoard::Sio88);
        let mut machine = BackendHost::rust_fast();
        machine.configure_memory(RamSize::K4, RamInit::Zeroed);

        let error = install_via_front_panel(&mut machine, definition).unwrap_err();
        assert!(error.contains("Power ON"));

        machine.power(true);
        machine.set_running(true);
        let error = install_via_front_panel(&mut machine, definition).unwrap_err();
        assert!(error.contains("STOP"));

        machine.set_running(false);
        machine.configure_memory(RamSize::K1, RamInit::Zeroed);
        let error = install_via_front_panel(&mut machine, definition).unwrap_err();
        assert!(error.contains("at least 4 KiB"));
    }

    #[test]
    fn bootstrap_consumes_reader_bytes_with_real_guest_in_on_both_rust_engines() {
        for engine in [
            EmulationEngine::RustFast8080,
            EmulationEngine::RustCycleAccurate8080,
        ] {
            for board in [SerialBoard::Sio88, SerialBoard::TwoSio88] {
                let mut machine = BackendHost::from_engine(engine).unwrap();
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

                // Let the guest reach the actual status-poll loop before putting
                // the first byte into RX. This matters for 88-2SIO because its
                // opening master-reset OUT would legitimately clear a byte that
                // had been unrealistically pre-injected before initialization.
                for _ in 0..400 {
                    machine.run_cycles(32);
                    if definition.pc_is_polling(machine.intel8080_state().pc) {
                        break;
                    }
                }
                assert!(definition.pc_is_polling(machine.intel8080_state().pc));

                machine.serial_receive(BackendSerialPort::Port0, 0xAE);
                for _ in 0..200 {
                    machine.run_cycles(64);
                    if machine.serial_rx_empty(BackendSerialPort::Port0) {
                        break;
                    }
                }
                assert!(machine.serial_rx_empty(BackendSerialPort::Port0));
                assert_eq!(machine.peek_memory(CHECKSUM_LOADER_END), Some(0x00));

                machine.serial_receive(BackendSerialPort::Port0, 0x42);
                for _ in 0..400 {
                    machine.run_cycles(64);
                    if machine.peek_memory(CHECKSUM_LOADER_END) == Some(0x42) {
                        break;
                    }
                }
                assert_eq!(machine.peek_memory(CHECKSUM_LOADER_END), Some(0x42));
                assert!(machine.serial_rx_empty(BackendSerialPort::Port0));
            }
        }
    }

    #[test]
    fn operator_address_math_rejects_16_bit_wrap() {
        assert_eq!(operator_target_address(0x0100, 0), Some(0x0100));
        assert_eq!(operator_target_address(0x0100, 2), Some(0x0102));
        assert_eq!(operator_target_address(0xFFFF, 0), Some(0xFFFF));
        assert_eq!(operator_target_address(0xFFFF, 1), None);
    }

    #[test]
    fn operator_hex_address_parser_accepts_common_forms() {
        assert_eq!(parse_hex_address("0100"), Some(0x0100));
        assert_eq!(parse_hex_address("0x0100"), Some(0x0100));
        assert_eq!(parse_hex_address("0100h"), Some(0x0100));
        assert_eq!(parse_hex_address("10000"), None);
        assert_eq!(parse_hex_address("xyz"), None);
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

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serialport::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};

use crate::config::{
    ComDataBits, ComFlowControl, ComParity, ComStopBits, ExternalComConfig,
};

const WORKER_TIMEOUT: Duration = Duration::from_millis(15);
const WORKER_TX_QUEUE: usize = 4096;
const RX_QUEUE_LIMIT: usize = 64 * 1024;
const COM_TRACE_LIMIT: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
struct HardwareConfig {
    port_name: String,
    baud_rate: u32,
    data_bits: ComDataBits,
    parity: ComParity,
    stop_bits: ComStopBits,
    flow_control: ComFlowControl,
}

impl From<&ExternalComConfig> for HardwareConfig {
    fn from(config: &ExternalComConfig) -> Self {
        Self {
            port_name: config.port_name.clone(),
            baud_rate: config.baud_rate,
            data_bits: config.data_bits,
            parity: config.parity,
            stop_bits: config.stop_bits,
            flow_control: config.flow_control,
        }
    }
}

impl HardwareConfig {
    fn serial_data_bits(&self) -> DataBits {
        match self.data_bits {
            ComDataBits::Five => DataBits::Five,
            ComDataBits::Six => DataBits::Six,
            ComDataBits::Seven => DataBits::Seven,
            ComDataBits::Eight => DataBits::Eight,
        }
    }

    fn serial_parity(&self) -> Parity {
        match self.parity {
            ComParity::None => Parity::None,
            ComParity::Odd => Parity::Odd,
            ComParity::Even => Parity::Even,
        }
    }

    fn serial_stop_bits(&self) -> StopBits {
        match self.stop_bits {
            ComStopBits::One => StopBits::One,
            ComStopBits::Two => StopBits::Two,
        }
    }

    fn serial_flow_control(&self) -> FlowControl {
        match self.flow_control {
            ComFlowControl::None => FlowControl::None,
            ComFlowControl::Software => FlowControl::Software,
            ComFlowControl::Hardware => FlowControl::Hardware,
        }
    }
}

enum WorkerCommand {
    Write(u8),
    ClearRx,
    SetBreak(bool),
}

enum WorkerEvent {
    Opened,
    Rx(Vec<u8>),
    /// Host API semantics: booleans mean the RS-232 signal is asserted. The
    /// app converts these to MC6850 TTL pin levels, whose active state is LOW.
    ModemPins { cts_asserted: bool, dcd_asserted: bool },
    Error(String),
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComTransportState {
    Disabled,
    Closed,
    Opening,
    Open,
    Error,
}

#[derive(Clone, Debug)]
struct ComTraceEvent {
    sequence: u64,
    inbound: bool,
    byte: u8,
    port_name: String,
}

/// Host-side physical/virtual serial transport.
///
/// The worker thread owns the OS serial handle. The emulator thread only moves
/// bytes and electrical-control requests through bounded channels, so unplugged
/// USB adapters, slow drivers and hardware flow control cannot block egui or the
/// Altair CPU loop.
pub(crate) struct ComSerialTransport {
    command_tx: Option<SyncSender<WorkerCommand>>,
    event_rx: Option<Receiver<WorkerEvent>>,
    worker: Option<JoinHandle<()>>,
    stop_flag: Option<Arc<AtomicBool>>,
    active_config: Option<HardwareConfig>,
    restart_requested: bool,
    state: ComTransportState,
    rx_queue: VecDeque<u8>,
    rx_bytes: u64,
    tx_bytes: u64,
    dropped_rx_bytes: u64,
    dropped_tx_bytes: u64,
    last_error: Option<String>,
    modem_pins_asserted: Option<(bool, bool)>,
    break_sent: Option<bool>,
    trace_enabled: bool,
    trace: VecDeque<ComTraceEvent>,
    next_trace_sequence: u64,
}

impl Default for ComSerialTransport {
    fn default() -> Self {
        Self {
            command_tx: None,
            event_rx: None,
            worker: None,
            stop_flag: None,
            active_config: None,
            restart_requested: false,
            state: ComTransportState::Disabled,
            rx_queue: VecDeque::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            dropped_rx_bytes: 0,
            dropped_tx_bytes: 0,
            last_error: None,
            modem_pins_asserted: None,
            break_sent: None,
            trace_enabled: false,
            trace: VecDeque::new(),
            next_trace_sequence: 1,
        }
    }
}

impl ComSerialTransport {
    pub(crate) fn available_port_names() -> Result<Vec<String>, String> {
        let mut ports = serialport::available_ports()
            .map_err(|error| format!("Could not enumerate serial ports: {error}"))?
            .into_iter()
            .map(|port| port.port_name)
            .collect::<Vec<_>>();
        ports.sort_by_key(|name| name.to_ascii_lowercase());
        ports.dedup();
        Ok(ports)
    }

    pub(crate) fn poll(&mut self, config: &ExternalComConfig) {
        if !config.enabled {
            if self.worker.is_some() || self.active_config.is_some() {
                self.stop_worker();
            }
            self.state = ComTransportState::Disabled;
            return;
        }

        if config.port_name.trim().is_empty() {
            if self.worker.is_some() || self.active_config.is_some() {
                self.stop_worker();
            }
            self.state = ComTransportState::Closed;
            self.last_error = Some("Select a COM/serial port before opening the endpoint".into());
            return;
        }

        let desired = HardwareConfig::from(config);
        if self.restart_requested || self.active_config.as_ref() != Some(&desired) {
            self.start_worker(desired);
            self.restart_requested = false;
        }

        self.drain_events();
    }

    fn start_worker(&mut self, config: HardwareConfig) {
        self.stop_worker();
        self.rx_queue.clear();
        self.last_error = None;
        self.modem_pins_asserted = None;
        self.break_sent = None;
        self.state = ComTransportState::Opening;

        let (command_tx, command_rx) = mpsc::sync_channel(WORKER_TX_QUEUE);
        let (event_tx, event_rx) = mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop_flag);
        let worker_config = config.clone();
        let worker = thread::Builder::new()
            .name("rustair-com-serial".into())
            .spawn(move || run_worker(worker_config, command_rx, event_tx, worker_stop));

        match worker {
            Ok(worker) => {
                self.command_tx = Some(command_tx);
                self.event_rx = Some(event_rx);
                self.worker = Some(worker);
                self.stop_flag = Some(stop_flag);
                self.active_config = Some(config);
            }
            Err(error) => {
                self.command_tx = None;
                self.event_rx = None;
                self.worker = None;
                self.stop_flag = None;
                self.active_config = Some(config);
                self.state = ComTransportState::Error;
                self.last_error = Some(format!("Could not start COM worker: {error}"));
            }
        }
    }

    fn drain_events(&mut self) {
        loop {
            let event = match self.event_rx.as_ref() {
                Some(receiver) => match receiver.try_recv() {
                    Ok(event) => event,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if self.state != ComTransportState::Error {
                            self.state = ComTransportState::Closed;
                        }
                        break;
                    }
                },
                None => break,
            };

            match event {
                WorkerEvent::Opened => {
                    self.state = ComTransportState::Open;
                    self.last_error = None;
                }
                WorkerEvent::Rx(bytes) => {
                    let port_name = self.active_port_name().to_owned();
                    for byte in bytes {
                        self.rx_bytes = self.rx_bytes.saturating_add(1);
                        self.record_trace(true, byte, &port_name);
                        if self.rx_queue.len() < RX_QUEUE_LIMIT {
                            self.rx_queue.push_back(byte);
                        } else {
                            self.dropped_rx_bytes = self.dropped_rx_bytes.saturating_add(1);
                        }
                    }
                }
                WorkerEvent::ModemPins { cts_asserted, dcd_asserted } => {
                    self.modem_pins_asserted = Some((cts_asserted, dcd_asserted));
                }
                WorkerEvent::Error(error) => {
                    self.state = ComTransportState::Error;
                    self.last_error = Some(error);
                }
                WorkerEvent::Closed => {
                    if self.state != ComTransportState::Error {
                        self.state = ComTransportState::Closed;
                    }
                }
            }
        }
    }

    pub(crate) fn queue_tx(&mut self, byte: u8) {
        let port_name = self.active_port_name().to_owned();
        let Some(sender) = self.command_tx.as_ref() else {
            self.dropped_tx_bytes = self.dropped_tx_bytes.saturating_add(1);
            return;
        };

        match sender.try_send(WorkerCommand::Write(byte)) {
            Ok(()) => {
                self.tx_bytes = self.tx_bytes.saturating_add(1);
                self.record_trace(false, byte, &port_name);
            }
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped_tx_bytes = self.dropped_tx_bytes.saturating_add(1);
            }
        }
    }

    /// Apply the emulated MC6850 BREAK output to a real host serial line. BREAK
    /// is an out-of-band continuous spacing condition and must never be encoded
    /// as a magic byte in the COM data stream.
    pub(crate) fn set_break_active(&mut self, active: bool) {
        if self.break_sent == Some(active) {
            return;
        }
        let Some(sender) = self.command_tx.as_ref() else {
            self.break_sent = None;
            return;
        };
        match sender.try_send(WorkerCommand::SetBreak(active)) {
            Ok(()) => self.break_sent = Some(active),
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.break_sent = None;
            }
        }
    }

    /// RS-232/OS assertion semantics, not MC6850 TTL pin levels.
    pub(crate) fn modem_pins_asserted(&self) -> Option<(bool, bool)> {
        self.modem_pins_asserted
    }

    pub(crate) fn pop_rx(&mut self) -> Option<u8> {
        self.rx_queue.pop_front()
    }

    pub(crate) fn clear_rx(&mut self) {
        self.rx_queue.clear();
        if let Some(sender) = &self.command_tx {
            let _ = sender.try_send(WorkerCommand::ClearRx);
        }
    }

    pub(crate) fn restart_on_next_poll(&mut self) {
        self.restart_requested = true;
        self.last_error = None;
    }

    pub(crate) fn state(&self) -> ComTransportState {
        self.state
    }

    pub(crate) fn active_port_name(&self) -> &str {
        self.active_config
            .as_ref()
            .map(|config| config.port_name.as_str())
            .unwrap_or("")
    }

    pub(crate) fn rx_pending(&self) -> usize {
        self.rx_queue.len()
    }

    pub(crate) fn rx_bytes(&self) -> u64 {
        self.rx_bytes
    }

    pub(crate) fn tx_bytes(&self) -> u64 {
        self.tx_bytes
    }

    pub(crate) fn dropped_rx_bytes(&self) -> u64 {
        self.dropped_rx_bytes
    }

    pub(crate) fn dropped_tx_bytes(&self) -> u64 {
        self.dropped_tx_bytes
    }

    pub(crate) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub(crate) fn trace_enabled(&self) -> bool {
        self.trace_enabled
    }

    pub(crate) fn set_trace_enabled(&mut self, enabled: bool) {
        self.trace_enabled = enabled;
    }

    pub(crate) fn trace_snapshot(&self) -> Vec<(u64, bool, u8, String)> {
        self.trace
            .iter()
            .map(|event| {
                (
                    event.sequence,
                    event.inbound,
                    event.byte,
                    event.port_name.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn clear_trace(&mut self) {
        self.trace.clear();
    }

    fn record_trace(&mut self, inbound: bool, byte: u8, port_name: &str) {
        if !self.trace_enabled {
            return;
        }
        self.trace.push_back(ComTraceEvent {
            sequence: self.next_trace_sequence,
            inbound,
            byte,
            port_name: port_name.to_owned(),
        });
        self.next_trace_sequence = self.next_trace_sequence.saturating_add(1);
        while self.trace.len() > COM_TRACE_LIMIT {
            self.trace.pop_front();
        }
    }

    fn stop_worker(&mut self) {
        if let Some(stop_flag) = self.stop_flag.take() {
            stop_flag.store(true, Ordering::Release);
        }
        self.command_tx = None;
        self.event_rx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.active_config = None;
        self.rx_queue.clear();
        self.modem_pins_asserted = None;
        self.break_sent = None;
    }
}

impl Drop for ComSerialTransport {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

fn run_worker(
    config: HardwareConfig,
    command_rx: Receiver<WorkerCommand>,
    event_tx: mpsc::Sender<WorkerEvent>,
    stop_flag: Arc<AtomicBool>,
) {
    let mut port = match serialport::new(&config.port_name, config.baud_rate)
        .data_bits(config.serial_data_bits())
        .parity(config.serial_parity())
        .stop_bits(config.serial_stop_bits())
        .flow_control(config.serial_flow_control())
        .timeout(WORKER_TIMEOUT)
        .open()
    {
        Ok(port) => port,
        Err(error) => {
            let _ = event_tx.send(WorkerEvent::Error(format!(
                "Could not open {}: {error}",
                config.port_name
            )));
            return;
        }
    };

    let _ = event_tx.send(WorkerEvent::Opened);
    let mut buffer = [0_u8; 256];
    let mut last_modem_pins = None;

    'worker: loop {
        if stop_flag.load(Ordering::Acquire) {
            break;
        }

        loop {
            if stop_flag.load(Ordering::Acquire) {
                break 'worker;
            }
            match command_rx.try_recv() {
                Ok(WorkerCommand::Write(byte)) => {
                    if let Err(error) = port.write_all(&[byte]) {
                        let _ = event_tx.send(WorkerEvent::Error(format!(
                            "Write to {} failed: {error}",
                            config.port_name
                        )));
                        break 'worker;
                    }
                }
                Ok(WorkerCommand::ClearRx) => {
                    if let Err(error) = port.clear(ClearBuffer::Input) {
                        let _ = event_tx.send(WorkerEvent::Error(format!(
                            "Could not clear input on {}: {error}",
                            config.port_name
                        )));
                        break 'worker;
                    }
                }
                Ok(WorkerCommand::SetBreak(active)) => {
                    let result = if active { port.set_break() } else { port.clear_break() };
                    if let Err(error) = result {
                        let action = if active { "assert" } else { "clear" };
                        let _ = event_tx.send(WorkerEvent::Error(format!(
                            "Could not {action} BREAK on {}: {error}",
                            config.port_name
                        )));
                        break 'worker;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break 'worker,
            }
        }

        if stop_flag.load(Ordering::Acquire) {
            break;
        }

        // serialport reports logical RS-232 assertion state. Keep that contract
        // here; conversion to the active-LOW MC6850 CTS/DCD pins belongs at the
        // emulated cable boundary in the app.
        if let (Ok(cts_asserted), Ok(dcd_asserted)) =
            (port.read_clear_to_send(), port.read_carrier_detect())
        {
            let pins = (cts_asserted, dcd_asserted);
            if last_modem_pins != Some(pins) {
                if event_tx
                    .send(WorkerEvent::ModemPins { cts_asserted, dcd_asserted })
                    .is_err()
                {
                    break;
                }
                last_modem_pins = Some(pins);
            }
        }

        match port.read(&mut buffer) {
            Ok(0) => {}
            Ok(count) => {
                if event_tx.send(WorkerEvent::Rx(buffer[..count].to_vec())).is_err() {
                    break;
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {}
            Err(error) => {
                let _ = event_tx.send(WorkerEvent::Error(format!(
                    "Read from {} failed: {error}",
                    config.port_name
                )));
                break;
            }
        }
    }

    let _ = event_tx.send(WorkerEvent::Closed);
}
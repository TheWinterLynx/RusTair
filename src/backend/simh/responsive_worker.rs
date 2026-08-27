use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, Weak, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::backend::{
    BackendCapabilities, BackendError, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, EmulationEngine, FrontPanelState, Intel8080State, IoPortActivity, IoTraceSnapshot,
    MachineBackend,
};
use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::{CpuDiagnosticResult, PanelLampSnapshot};

use super::serial_bridge::{SimhM2SioBridge, SimhM2SioRuntimeConfig};
use super::{
    AltairZ80CpuMode, SimhLaunchConfig, SimhOperationalState, SimhSession,
    embedded_altair_launch_config, embedded_altairz80_launch_config,
    set_altairz80_switch_register_low, set_switch_register,
};

const MEMORY_BYTES: usize = 64 * 1024;
const WORKER_TICK: Duration = Duration::from_millis(2);
const PANEL_TICK: Duration = Duration::from_millis(16);
const SWITCH_DEBOUNCE: Duration = Duration::from_millis(35);
const CONSOLE_LINES: usize = 2_000;

fn op_error(operation: &'static str, detail: impl Into<String>) -> BackendError {
    BackendError::Operation { operation, detail: detail.into() }
}

fn port_index(port: BackendSerialPort) -> usize {
    match port {
        BackendSerialPort::Port0 => 0,
        BackendSerialPort::Port1 => 1,
    }
}

fn port_name(port: BackendSerialPort) -> &'static str {
    match port {
        BackendSerialPort::Port0 => "M2SIO0 (10h/11h)",
        BackendSerialPort::Port1 => "M2SIO1 (12h/13h)",
    }
}

struct SharedState {
    cpu: CpuState,
    panel: FrontPanelState,
    memory: Vec<u8>,
    to_simh: [usize; 2],
    from_simh: [VecDeque<u8>; 2],
    io_trace_enabled: bool,
    busy: bool,
    last_error: Option<String>,
    console_available: bool,
    console_lines: VecDeque<String>,
    switch_generation: u64,
}

impl SharedState {
    fn new() -> Self {
        Self {
            cpu: CpuState::Intel8080(Intel8080State::default()),
            panel: FrontPanelState::default(),
            memory: vec![0; MEMORY_BYTES],
            to_simh: [0, 0],
            from_simh: [VecDeque::new(), VecDeque::new()],
            io_trace_enabled: false,
            busy: false,
            last_error: None,
            console_available: false,
            console_lines: VecDeque::new(),
            switch_generation: 0,
        }
    }

    fn log(&mut self, line: impl Into<String>) {
        if self.console_lines.len() >= CONSOLE_LINES {
            self.console_lines.pop_front();
        }
        self.console_lines.push_back(line.into());
    }

    fn error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.last_error = Some(message.clone());
        self.log(format!("ERROR: {message}"));
    }
}

enum Command {
    Power(bool),
    Running(bool),
    Step,
    Reset,
    Examine(bool),
    Deposit(bool),
    ConfigureSerial(SerialBoard),
    Load(u16, Vec<u8>),
    Write(u16, u8),
    SerialRx(BackendSerialPort, u8),
    ClearSerial,
    Console(String),
    Shutdown,
}

#[derive(Clone, Debug, Default)]
pub struct SimhConsoleSnapshot {
    pub engine: String,
    pub powered: bool,
    pub running: bool,
    pub busy: bool,
    pub console_available: bool,
    pub last_error: Option<String>,
    pub lines: Vec<String>,
}

struct ActiveConsoleEndpoint {
    engine: EmulationEngine,
    tx: mpsc::Sender<Command>,
    shared: Weak<Mutex<SharedState>>,
}

static ACTIVE_CONSOLE: OnceLock<Mutex<Option<ActiveConsoleEndpoint>>> = OnceLock::new();

fn active_console_slot() -> &'static Mutex<Option<ActiveConsoleEndpoint>> {
    ACTIVE_CONSOLE.get_or_init(|| Mutex::new(None))
}

fn register_active_console(
    engine: EmulationEngine,
    tx: mpsc::Sender<Command>,
    shared: &Arc<Mutex<SharedState>>,
) {
    let mut slot = active_console_slot().lock().unwrap_or_else(|p| p.into_inner());
    *slot = Some(ActiveConsoleEndpoint {
        engine,
        tx,
        shared: Arc::downgrade(shared),
    });
}

pub fn active_console_snapshot() -> Option<SimhConsoleSnapshot> {
    let slot = active_console_slot().lock().unwrap_or_else(|p| p.into_inner());
    let endpoint = slot.as_ref()?;
    let shared = endpoint.shared.upgrade()?;
    let shared = shared.lock().unwrap_or_else(|p| p.into_inner());
    Some(SimhConsoleSnapshot {
        engine: endpoint.engine.label().to_owned(),
        powered: shared.panel.powered,
        running: shared.panel.running,
        busy: shared.busy,
        console_available: shared.console_available,
        last_error: shared.last_error.clone(),
        lines: shared.console_lines.iter().cloned().collect(),
    })
}

pub fn submit_active_console(command: impl Into<String>) -> Result<(), String> {
    let command = command.into();
    let slot = active_console_slot().lock().unwrap_or_else(|p| p.into_inner());
    let Some(endpoint) = slot.as_ref() else {
        return Err("no active Open-SIMH backend".into());
    };
    endpoint
        .tx
        .send(Command::Console(command))
        .map_err(|error| format!("SIMH worker is no longer available: {error}"))
}

pub struct SimhThreadedBackend {
    engine: EmulationEngine,
    tx: mpsc::Sender<Command>,
    shared: Arc<Mutex<SharedState>>,
}

impl SimhThreadedBackend {
    pub fn new(engine: EmulationEngine) -> BackendResult<Self> {
        if !matches!(engine, EmulationEngine::SimhAltair | EmulationEngine::SimhAltairZ80) {
            return Err(BackendError::Unsupported {
                operation: "threaded SIMH backend creation",
                engine,
            });
        }

        let shared = Arc::new(Mutex::new(SharedState::new()));
        {
            let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
            state.log(format!("RusTair selected {}", engine.label()));
            state.log("Low-latency bridge active: egui never waits for FrontPanel/SCP.");
            state.log("Panel source: during RUN, backend-observed SIMH PC/A samples; during STOP, exact binary PC/A or the operator EXAMINE/DEPOSIT latch. FrontPanel API v12 exposes no exact S-100 MEMR/M1/WO/INP/OUT bus feed, so unsupported status lamps stay dark instead of being synthesized.");
        }

        let worker_shared = Arc::clone(&shared);
        let failure_shared = Arc::clone(&shared);
        let (tx, rx) = mpsc::channel();

        thread::Builder::new()
            .name(match engine {
                EmulationEngine::SimhAltair => "rustair-simh-altair".into(),
                _ => "rustair-simh-altairz80".into(),
            })
            .spawn(move || match Worker::new(engine, worker_shared) {
                Ok(mut worker) => worker.run(rx),
                Err(error) => {
                    let mut state = failure_shared.lock().unwrap_or_else(|p| p.into_inner());
                    state.busy = false;
                    state.panel.powered = false;
                    state.panel.running = false;
                    state.panel.lamps = PanelLampSnapshot::default();
                    state.error(format!("worker initialization failed: {error}"));
                }
            })
            .map_err(|error| op_error("spawn SIMH worker", error.to_string()))?;

        register_active_console(engine, tx.clone(), &shared);
        Ok(Self { engine, tx, shared })
    }

    fn read_shared<T>(&self, read: impl FnOnce(&SharedState) -> T) -> T {
        let guard = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        read(&guard)
    }

    fn write_shared<T>(&self, write: impl FnOnce(&mut SharedState) -> T) -> T {
        let mut guard = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        write(&mut guard)
    }

    fn enqueue(&self, command: Command) {
        if let Err(error) = self.tx.send(command) {
            self.write_shared(|state| state.error(format!("could not queue command: {error}")));
        }
    }
}

impl Drop for SimhThreadedBackend {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        // FrontPanel shutdown is intentionally allowed to finish off the UI thread.
    }
}

impl MachineBackend for SimhThreadedBackend {
    fn engine(&self) -> EmulationEngine { self.engine }

    fn name(&self) -> &'static str {
        match self.engine {
            EmulationEngine::SimhAltair => "Open SIMH classic Altair — low-latency worker",
            EmulationEngine::SimhAltairZ80 => "Open SIMH AltairZ80 — low-latency worker",
            _ => "Open SIMH",
        }
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            front_panel: true,
            exact_bus_activity: false,
            exact_t_state_timing: false,
            memory_protection: false,
            hold_hlda: false,
            direct_memory_access: true,
            serial_routing: self.engine == EmulationEngine::SimhAltairZ80,
            disk_mount: true,
        }
    }

    fn execution_model(&self) -> BackendExecutionModel { BackendExecutionModel::ExternalProcess }

    // Every method on the egui hot path below is local-memory only.
    fn cpu_state(&mut self) -> BackendResult<CpuState> { Ok(self.read_shared(|s| s.cpu)) }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { Ok(self.read_shared(|s| s.panel)) }
    fn service_execution(&mut self, _t_state_budget: u32) -> BackendResult<()> { Ok(()) }
    fn commit_panel_activity(&mut self, _dt: Duration) -> BackendResult<()> { Ok(()) }

    fn configure_memory(&mut self, _size: RamSize, _init: RamInit) -> BackendResult<()> { Ok(()) }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        self.write_shared(|state| {
            // The physical switch moves immediately, but LEDs do not predict
            // what SIMH will show. They stay dark until a real sample arrives.
            state.panel.powered = on;
            state.panel.running = false;
            state.busy = on;
            state.panel.lamps = PanelLampSnapshot::default();
            if on {
                state.log("POWER ON requested");
            } else {
                state.console_available = false;
                state.log("POWER OFF requested");
            }
        });
        self.enqueue(Command::Power(on));
        Ok(())
    }

    fn power_with_historical_run_latch(&mut self, on: bool, _historical: bool) -> BackendResult<()> {
        self.power(on)
    }

    fn run(&mut self) -> BackendResult<()> {
        // RUN/STOP switch feedback may be immediate, but the lamp image remains
        // the last backend sample until SIMH itself reports new state/data.
        self.write_shared(|state| state.panel.running = true);
        self.enqueue(Command::Running(true));
        Ok(())
    }

    fn halt(&mut self) -> BackendResult<()> {
        self.write_shared(|state| state.panel.running = false);
        self.enqueue(Command::Running(false));
        Ok(())
    }

    fn step(&mut self) -> BackendResult<()> { self.enqueue(Command::Step); Ok(()) }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { if run { self.run() } else { self.halt() } }
    fn release_run_stop(&mut self, _run: bool) -> BackendResult<()> { Ok(()) }
    fn assert_reset(&mut self) -> BackendResult<()> { self.enqueue(Command::Reset); Ok(()) }
    fn release_reset(&mut self) -> BackendResult<()> { Ok(()) }
    fn assert_clear(&mut self) -> BackendResult<()> { self.clear_serial() }
    fn release_clear(&mut self) -> BackendResult<()> { Ok(()) }
    fn request_hold(&mut self, _hold: bool) -> BackendResult<()> { Ok(()) }

    fn panel_examine(&mut self, next: bool) -> BackendResult<()> { self.enqueue(Command::Examine(next)); Ok(()) }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> { self.enqueue(Command::Deposit(next)); Ok(()) }
    fn protect_current_board(&mut self, _protected: bool) -> BackendResult<()> { Ok(()) }

    fn switch_register(&mut self) -> BackendResult<u16> { Ok(self.read_shared(|s| s.panel.switches)) }

    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        // The physical lever is local and immediate; the worker coalesces a
        // burst and writes only the final 16-bit value while halted or before RUN.
        self.write_shared(|state| {
            state.panel.switches = value;
            state.switch_generation = state.switch_generation.wrapping_add(1);
        });
        Ok(())
    }

    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        self.enqueue(Command::ConfigureSerial(board));
        Ok(())
    }

    fn serial_board(&mut self) -> BackendResult<SerialBoard> { Ok(SerialBoard::TwoSio88) }

    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        if self.engine != EmulationEngine::SimhAltairZ80 { return Ok(()); }
        let index = port_index(port);
        self.write_shared(|s| s.to_simh[index] = s.to_simh[index].saturating_add(1));
        self.enqueue(Command::SerialRx(port, byte));
        Ok(())
    }

    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        Ok(self.read_shared(|s| s.to_simh[port_index(port)] == 0))
    }
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> {
        Ok(self.read_shared(|s| s.to_simh[port_index(port)]))
    }
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        Ok(self.read_shared(|s| !s.from_simh[port_index(port)].is_empty()))
    }
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        Ok(self.read_shared(|s| s.from_simh[port_index(port)].front().copied()))
    }
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        Ok(self.write_shared(|s| s.from_simh[port_index(port)].pop_front()))
    }
    fn clear_serial(&mut self) -> BackendResult<()> {
        self.write_shared(|s| {
            s.to_simh = [0, 0];
            s.from_simh[0].clear();
            s.from_simh[1].clear();
        });
        self.enqueue(Command::ClearSerial);
        Ok(())
    }

    fn installed_ram_bytes(&mut self) -> BackendResult<usize> { Ok(MEMORY_BYTES) }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        Ok(Some(self.read_shared(|s| s.memory[address as usize])))
    }
    fn write_memory(&mut self, address: u16, value: u8, _respect_protection: bool) -> BackendResult<bool> {
        self.write_shared(|s| s.memory[address as usize] = value);
        self.enqueue(Command::Write(address, value));
        Ok(true)
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.write_shared(|s| {
            for (offset, byte) in bytes.iter().copied().enumerate() {
                let pos = address as usize + offset;
                if pos >= s.memory.len() { break; }
                s.memory[pos] = byte;
            }
            s.log(format!("queued {} bytes at {:04X}h", bytes.len(), address));
        });
        self.enqueue(Command::Load(address, bytes.to_vec()));
        Ok(())
    }
    fn memory_is_protected(&mut self, _address: u16) -> BackendResult<bool> { Ok(false) }
    fn clear_memory_protection(&mut self) -> BackendResult<()> { Ok(()) }
    fn clear_transient_memory_guards(&mut self) -> BackendResult<()> { Ok(()) }
    fn arm_basic32_full_memory_probe_guard(&mut self) -> BackendResult<bool> { Ok(false) }
    fn cancel_cpu_diagnostic_meter(&mut self) -> BackendResult<()> { Ok(()) }
    fn take_cpu_diagnostic_result(&mut self) -> BackendResult<Option<CpuDiagnosticResult>> { Ok(None) }

    fn peek_io_port(&mut self, _port: u8) -> BackendResult<u8> { Ok(0) }
    fn io_port_activity(&mut self, _port: u8) -> BackendResult<IoPortActivity> { Ok((None, None, 0, 0)) }
    fn io_trace_snapshot(&mut self) -> BackendResult<IoTraceSnapshot> { Ok(Vec::new()) }
    fn io_trace_enabled(&mut self) -> BackendResult<bool> { Ok(self.read_shared(|s| s.io_trace_enabled)) }
    fn set_io_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> {
        self.write_shared(|s| s.io_trace_enabled = enabled);
        Ok(())
    }
    fn clear_io_trace(&mut self) -> BackendResult<()> { Ok(()) }
    fn debugger_input_port(&mut self, _port: u8) -> BackendResult<u8> { Ok(0) }
    fn debugger_output_port(&mut self, _port: u8, _value: u8) -> BackendResult<()> { Ok(()) }
    fn debugger_inject_serial_rx(&mut self, _port: u8, _byte: u8) -> BackendResult<bool> { Ok(false) }
    fn debugger_clear_serial_rx(&mut self, _port: u8) -> BackendResult<bool> { Ok(false) }
    fn debugger_clear_serial_tx(&mut self, _port: u8) -> BackendResult<bool> { Ok(false) }
    fn debugger_complete_serial_tx(&mut self, _port: u8) -> BackendResult<Option<u8>> { Ok(None) }
}

struct PendingLoad {
    base: u16,
    bytes: Vec<u8>,
}

struct Worker {
    engine: EmulationEngine,
    launch: SimhLaunchConfig,
    session: Option<SimhSession>,
    bridge: Option<SimhM2SioBridge>,
    _serial_config: Option<SimhM2SioRuntimeConfig>,
    shared: Arc<Mutex<SharedState>>,
    panel_address: u16,
    panel_data: u8,
    operator_panel_latched: bool,
    pending_loads: Vec<PendingLoad>,
    applied_switches: u16,
    observed_switch_generation: u64,
    switch_sync_due: Option<Instant>,
    last_panel_copy: Instant,
    last_running: bool,
    load_faulted: bool,
    bridge_connected: [bool; 2],
    first_guest_tx_seen: [bool; 2],
    first_host_rx_seen: [bool; 2],
}

impl Worker {
    fn new(engine: EmulationEngine, shared: Arc<Mutex<SharedState>>) -> BackendResult<Self> {
        {
            let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = true;
            state.log("Preparing embedded Open-SIMH runtime…");
        }

        let (launch, bridge, serial_config) = match engine {
            EmulationEngine::SimhAltair => {
                let launch = embedded_altair_launch_config()
                    .map_err(|e| op_error("prepare embedded SIMH Altair", e.to_string()))?;
                (launch, None, None)
            }
            EmulationEngine::SimhAltairZ80 => {
                let mut launch = embedded_altairz80_launch_config(AltairZ80CpuMode::Intel8080)
                    .map_err(|e| op_error("prepare embedded SIMH AltairZ80", e.to_string()))?;
                let bridge = SimhM2SioBridge::bind_loopback()
                    .map_err(|e| op_error("bind SIMH M2SIO bridge", e.to_string()))?;
                let (p0, p1) = bridge.listen_ports();
                let serial_config = SimhM2SioRuntimeConfig::create(launch.simulator_config(), p0, p1)
                    .map_err(|e| op_error("prepare SIMH M2SIO runtime config", e.to_string()))?;
                launch.simulator_config = serial_config.path().to_path_buf();
                (launch, Some(bridge), Some(serial_config))
            }
            _ => return Err(BackendError::Unsupported { operation: "SIMH worker engine", engine }),
        };

        {
            let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = false;
            state.log("SIMH worker ready. POWER remains operator-controlled.");
        }

        Ok(Self {
            engine,
            launch,
            session: None,
            bridge,
            _serial_config: serial_config,
            shared,
            panel_address: 0,
            panel_data: 0,
            operator_panel_latched: false,
            pending_loads: Vec::new(),
            applied_switches: 0,
            observed_switch_generation: 0,
            switch_sync_due: None,
            last_panel_copy: Instant::now(),
            last_running: false,
            load_faulted: false,
            bridge_connected: [false; 2],
            first_guest_tx_seen: [false; 2],
            first_host_rx_seen: [false; 2],
        })
    }

    fn run(&mut self, rx: mpsc::Receiver<Command>) {
        loop {
            match rx.recv_timeout(WORKER_TICK) {
                Ok(Command::Shutdown) => break,
                Ok(command) => self.handle(command),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.background();
        }
        self.session.take();
        if let Some(bridge) = self.bridge.as_mut() { bridge.disconnect(); }
    }

    fn handle(&mut self, command: Command) {
        let started = Instant::now();
        let (operation, result) = match command {
            Command::Power(on) => (
                if on { "POWER ON" } else { "POWER OFF" },
                self.power(on),
            ),
            Command::Running(run) => (
                if run { "RUN" } else { "STOP" },
                self.set_running(run),
            ),
            Command::Step => ("STEP", self.step()),
            Command::Reset => ("RESET", self.reset()),
            Command::Examine(next) => (
                if next { "EXAMINE NEXT" } else { "EXAMINE" },
                self.examine(next),
            ),
            Command::Deposit(next) => (
                if next { "DEPOSIT NEXT" } else { "DEPOSIT" },
                self.deposit(next),
            ),
            Command::ConfigureSerial(board) => {
                let result = if self.engine == EmulationEngine::SimhAltairZ80
                    && board != SerialBoard::TwoSio88
                {
                    Err(op_error("serial configuration", "embedded AltairZ80 is fixed to MITS 88-2SIO"))
                } else {
                    Ok(())
                };
                ("serial configuration", result)
            }
            Command::Load(address, bytes) => {
                let result = self.load(address, &bytes);
                self.load_faulted = result.is_err();
                ("memory load", result)
            }
            Command::Write(address, value) => ("memory write", self.write(address, value).map(|_| ())),
            Command::SerialRx(port, byte) => {
                self.serial_rx(port, byte);
                return;
            }
            Command::ClearSerial => {
                if let Some(bridge) = self.bridge.as_mut() { bridge.clear_queues(); }
                return;
            }
            Command::Console(command) => {
                self.console(command);
                return;
            }
            Command::Shutdown => return,
        };
        self.finish_timed(operation, started, result);
    }

    fn finish_timed(&mut self, operation: &'static str, started: Instant, result: BackendResult<()>) {
        let elapsed = started.elapsed();
        let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        match result {
            Ok(()) => {
                if operation == "memory load" {
                    state.last_error = None;
                }
                state.log(format!("{operation}: {} ms", elapsed.as_millis()));
            }
            Err(error) => {
                if operation == "POWER ON" {
                    state.panel.powered = false;
                    state.panel.running = false;
                    state.panel.lamps = PanelLampSnapshot::default();
                    state.console_available = false;
                }
                if operation == "RUN" || operation == "STOP" {
                    state.panel.running = false;
                }
                state.busy = false;
                state.error(format!("{operation} after {} ms: {error}", elapsed.as_millis()));
            }
        }
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        if on && self.session.is_none() {
            self.operator_panel_latched = false;
            let power_started = Instant::now();
            {
                let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                state.busy = true;
                state.panel.lamps = PanelLampSnapshot::default();
                state.log("Starting SIMH process + FrontPanel remote connection…");
            }

            let session = SimhSession::start(
                self.launch.executable(),
                self.launch.simulator_config(),
                self.launch.device_panel_count,
            )
            .map_err(|e| op_error("SIMH power on", e.to_string()))?;
            let timings = session.startup_timings();
            self.session = Some(session);
            self.load_faulted = false;

            {
                let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                state.log(format!(
                    "Startup phases: runtime {} ms | DLL load {} ms | sim_panel_start_simulator {} ms | live register/callback setup {} ms | total {} ms",
                    timings.runtime_ms,
                    timings.dll_load_ms,
                    timings.start_api_ms,
                    timings.live_panel_setup_ms,
                    timings.total_ms,
                ));
            }

            self.sync_switches_now()?;

            if !self.pending_loads.is_empty() {
                let pending = std::mem::take(&mut self.pending_loads);
                for load in pending {
                    self.load(load.base, &load.bytes)?;
                }
            }

            let sample = self.session
                .as_mut()
                .expect("session just started")
                .refresh_live_panel_now()
                .map_err(|e| op_error("initial SIMH panel refresh", e.to_string()))?;
            if sample.valid {
                {
                    let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                    state.log(format!(
                        "Initial halted SIMH registers: PC={:04X}h A={:02X}h SP={:04X}h",
                        sample.pc, sample.a, sample.sp
                    ));
                }
                self.apply_sample(sample, false);
            }

            let console_available = self.session.as_ref().is_some_and(SimhSession::console_available);
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = false;
            state.panel.powered = true;
            state.panel.running = false;
            state.console_available = console_available;
            state.last_error = None;
            state.log(format!("POWER ON complete in {} ms", power_started.elapsed().as_millis()));
            if !console_available {
                state.log("Interactive sim> extension is not present in the currently embedded DLL; worker timing is still available.");
            }
            return Ok(());
        }

        if !on && self.session.is_some() {
            {
                let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                state.busy = true;
                state.log("Stopping SIMH process…");
            }
            self.session.take();
            if let Some(bridge) = self.bridge.as_mut() { bridge.disconnect(); }
            self.last_running = false;
            self.load_faulted = false;
            self.operator_panel_latched = false;
            self.applied_switches = 0;
            self.bridge_connected = [false; 2];
            self.first_guest_tx_seen = [false; 2];
            self.first_host_rx_seen = [false; 2];
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = false;
            state.panel.powered = false;
            state.panel.running = false;
            state.panel.lamps = PanelLampSnapshot::default();
            state.console_available = false;
        }
        Ok(())
    }

    fn set_running(&mut self, running: bool) -> BackendResult<()> {
        if self.session.is_none() {
            return Err(op_error("SIMH RUN/STOP", "simulator is powered off or still starting"));
        }

        self.operator_panel_latched = false;
        if running {
            if self.load_faulted {
                return Err(op_error(
                    "SIMH RUN",
                    "blocked because the previous memory load failed; perform a successful load or power-cycle before RUN",
                ));
            }
            self.sync_switches_now()?;
            self.session
                .as_mut()
                .expect("checked session")
                .run()
                .map_err(|e| op_error("SIMH RUN", e.to_string()))?;
            self.last_running = true;
            self.update_running_state(true);
        } else {
            self.session
                .as_mut()
                .expect("checked session")
                .halt()
                .map_err(|e| op_error("SIMH HALT", e.to_string()))?;
            self.last_running = false;
            self.update_running_state(false);
        }
        Ok(())
    }

    fn step(&mut self) -> BackendResult<()> {
        if self.session.is_none() {
            return Err(op_error("SIMH STEP", "simulator is powered off or still starting"));
        }
        self.operator_panel_latched = false;
        self.sync_switches_now()?;
        self.session
            .as_mut()
            .expect("checked session")
            .step()
            .map_err(|e| op_error("SIMH STEP", e.to_string()))?;

        // STEP is an explicit operator action, so an immediate halted sample is
        // appropriate here. Unlike the periodic RUN sampling path, a halted
        // sample is rendered as exact binary PC/A values, not historical duty
        // cycle accumulated before STOP.
        let sample = self.session
            .as_mut()
            .expect("checked session")
            .refresh_live_panel_now()
            .map_err(|e| op_error("SIMH STEP panel refresh", e.to_string()))?;
        if sample.valid {
            self.apply_sample(sample, false);
        }
        Ok(())
    }

    fn reset(&mut self) -> BackendResult<()> {
        self.operator_panel_latched = false;
        self.panel_address = 0;
        if self.session.is_none() { return Ok(()); }
        let was_running = self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running);
        if was_running {
            self.session.as_mut().expect("checked session").halt()
                .map_err(|e| op_error("SIMH RESET halt", e.to_string()))?;
        }
        self.session.as_mut().expect("checked session").deposit_register_u32("PC", 0)
            .map_err(|e| op_error("SIMH RESET PC", e.to_string()))?;
        if was_running {
            self.sync_switches_now()?;
            self.session.as_mut().expect("checked session").run()
                .map_err(|e| op_error("SIMH RESET resume", e.to_string()))?;
        }
        self.last_running = was_running;
        self.panel_address = 0;
        let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        state.panel.address = 0;
        Ok(())
    }

    fn examine(&mut self, next: bool) -> BackendResult<()> {
        if self.session.is_none() {
            return Err(op_error("SIMH EXAMINE", "simulator is powered off or still starting"));
        }
        if self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running) {
            return Err(op_error("SIMH EXAMINE", "front-panel EXAMINE requires STOP"));
        }
        let desired_switches = self.desired_switches();
        let address = if next { self.panel_address.wrapping_add(1) } else { desired_switches };
        let data = self.session.as_ref().expect("checked session").read_byte(address)
            .map_err(|e| op_error("SIMH EXAMINE", e.to_string()))?;
        self.panel_address = address;
        self.panel_data = data;
        self.session.as_mut().expect("checked session").deposit_register_u32("PC", address as u32)
            .map_err(|e| op_error("SIMH EXAMINE PC", e.to_string()))?;
        self.set_stopped_panel(address, data);
        Ok(())
    }

    fn deposit(&mut self, next: bool) -> BackendResult<()> {
        if self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running) {
            return Err(op_error("SIMH DEPOSIT", "front-panel DEPOSIT requires STOP"));
        }
        let address = if next { self.panel_address.wrapping_add(1) } else { self.panel_address };
        let value = self.desired_switches() as u8;
        if let Some(session) = self.session.as_mut() {
            session.write_byte(address, value)
                .map_err(|e| op_error("SIMH DEPOSIT", e.to_string()))?;
            session.deposit_register_u32("PC", address as u32)
                .map_err(|e| op_error("SIMH DEPOSIT PC", e.to_string()))?;
        } else {
            self.pending_loads.push(PendingLoad { base: address, bytes: vec![value] });
        }
        self.panel_address = address;
        self.panel_data = value;
        self.set_stopped_panel(address, value);
        Ok(())
    }

    fn load(&mut self, base: u16, bytes: &[u8]) -> BackendResult<()> {
        if self.session.is_none() {
            self.pending_loads.push(PendingLoad { base, bytes: bytes.to_vec() });
            return Ok(());
        }

        let was_running = self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running);
        if was_running {
            self.session.as_mut().expect("checked session").halt()
                .map_err(|e| op_error("SIMH memory load halt", e.to_string()))?;
            self.last_running = false;
        }

        {
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = true;
            state.log(format!("Loading {} bytes at {:04X}h into SIMH…", bytes.len(), base));
        }

        self.session.as_mut().expect("checked session").load_bytes(base, bytes)
            .map_err(|e| op_error("SIMH memory load", e.to_string()))?;

        {
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = false;
            state.log(format!("Loaded {} bytes at {:04X}h.", bytes.len(), base));
        }

        // Do not implicitly resume here. The app queues its intended RUN/STOP
        // state explicitly after a load, preserving command order.
        Ok(())
    }

    fn write(&mut self, address: u16, value: u8) -> BackendResult<bool> {
        if let Some(session) = self.session.as_mut() {
            if session.state() == SimhOperationalState::Running { return Ok(false); }
            session.write_byte(address, value)
                .map_err(|e| op_error("SIMH memory write", e.to_string()))?;
        } else {
            self.pending_loads.push(PendingLoad { base: address, bytes: vec![value] });
        }
        Ok(true)
    }

    fn serial_rx(&mut self, port: BackendSerialPort, byte: u8) {
        let index = port_index(port);
        let queue_error = self.bridge.as_mut()
            .and_then(|bridge| bridge.queue_to_simh(port, byte).err())
            .map(|error| error.to_string());
        if let Some(error) = queue_error {
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.error(format!("serial RX queue: {error}"));
        } else if self.bridge.is_some() && !self.first_host_rx_seen[index] {
            self.first_host_rx_seen[index] = true;
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.log(format!("{} first RusTair→guest byte: {:02X}h", port_name(port), byte));
        }
        let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        state.to_simh[index] = state.to_simh[index].saturating_sub(1);
    }

    fn console(&mut self, command: String) {
        let command = command.trim().to_owned();
        if command.is_empty() { return; }
        {
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.log(format!("sim> {command}"));
        }
        let started = Instant::now();
        let result = match self.session.as_mut() {
            Some(session) => session.console_command(&command),
            None => Err(super::SimhSessionError::Api {
                operation: "console command",
                detail: "simulator is powered off or still starting".into(),
            }),
        };
        let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        match result {
            Ok(response) => {
                if response.trim().is_empty() {
                    state.log("(command completed with no output)");
                } else {
                    for line in response.lines() { state.log(line.to_owned()); }
                }
                state.log(format!("console round trip: {} ms", started.elapsed().as_millis()));
            }
            Err(error) => state.error(format!("SIMH console: {error}")),
        }
    }

    fn background(&mut self) {
        self.poll_bridge();
        self.observe_switch_changes();
        self.sync_debounced_switches();

        if self.session.is_none() || self.last_panel_copy.elapsed() < PANEL_TICK { return; }
        self.last_panel_copy = Instant::now();

        let state = self.session.as_ref().map(|s| s.state()).unwrap_or(SimhOperationalState::Halted);
        let running = state == SimhOperationalState::Running;
        let sample = self.session.as_ref().expect("checked session").live_panel_sample();
        if sample.valid {
            self.apply_sample(sample, running);
        }
        self.last_running = running;
    }

    fn poll_bridge(&mut self) {
        let poll_error = self.bridge.as_mut()
            .and_then(|bridge| bridge.poll().err())
            .map(|error| error.to_string());
        if let Some(error) = poll_error {
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.error(format!("M2SIO bridge poll: {error}"));
        }

        let mut log_lines = Vec::new();
        if let Some(bridge) = self.bridge.as_mut() {
            for port in BackendSerialPort::ALL {
                let index = port_index(port);
                let connected = bridge.connected(port);
                if connected != self.bridge_connected[index] {
                    self.bridge_connected[index] = connected;
                    log_lines.push(format!(
                        "{} bridge {}",
                        port_name(port),
                        if connected { "CONNECTED" } else { "DISCONNECTED" }
                    ));
                }

                while let Some(byte) = bridge.pop_from_simh(port) {
                    if !self.first_guest_tx_seen[index] {
                        self.first_guest_tx_seen[index] = true;
                        log_lines.push(format!("{} first guest→RusTair byte: {:02X}h", port_name(port), byte));
                    }
                    let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                    let queue = &mut state.from_simh[index];
                    if queue.len() < 64 * 1024 { queue.push_back(byte); }
                }
            }
        }
        if !log_lines.is_empty() {
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            for line in log_lines { state.log(line); }
        }
    }

    fn observe_switch_changes(&mut self) {
        let generation = {
            let state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.switch_generation
        };
        if generation != self.observed_switch_generation {
            self.observed_switch_generation = generation;
            self.switch_sync_due = Some(Instant::now() + SWITCH_DEBOUNCE);
        }
    }

    fn sync_debounced_switches(&mut self) {
        let Some(due) = self.switch_sync_due else { return; };
        if Instant::now() < due { return; }
        if self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running) {
            return;
        }
        match self.sync_switches_now() {
            Ok(()) => self.switch_sync_due = None,
            Err(error) => {
                self.switch_sync_due = None;
                let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                state.error(format!("sense switch sync: {error}"));
            }
        }
    }

    fn desired_switches(&self) -> u16 {
        self.shared.lock().unwrap_or_else(|p| p.into_inner()).panel.switches
    }

    fn sync_switches_now(&mut self) -> BackendResult<()> {
        let desired = self.desired_switches();
        if desired == self.applied_switches || self.session.is_none() { return Ok(()); }
        if self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running) {
            // Never HALT/DEPOSIT/CONT for a mouse click. The final value is
            // guaranteed to be written before the next RUN command.
            return Ok(());
        }
        let session = self.session.as_mut().expect("checked session");
        match self.engine {
            EmulationEngine::SimhAltair => set_switch_register(session, desired)
                .map_err(|e| op_error("SIMH switch register", e.to_string()))?,
            EmulationEngine::SimhAltairZ80 => set_altairz80_switch_register_low(session, desired as u8)
                .map_err(|e| op_error("SIMH switch register", e.to_string()))?,
            _ => {}
        }
        self.applied_switches = desired;
        Ok(())
    }

    fn apply_sample(&mut self, sample: super::SimhLivePanelSample, running: bool) {
        if !sample.valid { return; }

        // CPU register telemetry is always allowed to advance, even while an
        // operator EXAMINE/DEPOSIT latch owns the stopped front-panel display.
        let switches = self.desired_switches();
        let show_sample_on_panel = running || !self.operator_panel_latched;
        if show_sample_on_panel {
            self.panel_address = sample.pc;
            self.panel_data = sample.a;
        }

        let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        match &mut state.cpu {
            CpuState::Intel8080(cpu) => {
                cpu.pc = sample.pc;
                cpu.sp = sample.sp;
                cpu.a = sample.a;
            }
            CpuState::Z80(cpu) => {
                cpu.pc = sample.pc;
                cpu.sp = sample.sp;
                cpu.a = sample.a;
            }
        }
        state.panel.powered = self.session.is_some();
        state.panel.running = running;
        state.panel.switches = switches;

        if show_sample_on_panel {
            state.panel.address = sample.pc;
            state.panel.data = sample.a;
            // While RUNning, accumulated PC/A bit activity provides a useful
            // backend-observed motion proxy. Once halted, history is invalid as
            // a static LED state: show the exact current binary register values.
            state.panel.lamps = if running {
                observed_sample_lamps(&sample)
            } else {
                observed_value_lamps(sample.pc, sample.a)
            };
        }
    }

    fn update_running_state(&mut self, running: bool) {
        let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        state.panel.powered = self.session.is_some();
        state.panel.running = running;
        // Do not manufacture a new lamp frame for a control acknowledgement.
        // The callback owns the LED image.
    }

    fn set_stopped_panel(&mut self, address: u16, data: u8) {
        self.operator_panel_latched = true;
        let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        state.panel.powered = self.session.is_some();
        state.panel.running = false;
        state.panel.address = address;
        state.panel.data = data;
        // EXAMINE/DEPOSIT has just produced these actual backend values. Only
        // ADDRESS/DATA are representable; unsupported S-100 status stays dark.
        state.panel.lamps = observed_value_lamps(address, data);
        match &mut state.cpu {
            CpuState::Intel8080(cpu) => cpu.pc = address,
            CpuState::Z80(cpu) => cpu.pc = address,
        }
    }
}

fn observed_sample_lamps(sample: &super::SimhLivePanelSample) -> PanelLampSnapshot {
    let mut lamps = PanelLampSnapshot::default();
    lamps.address = sample.address_activity;
    lamps.data = sample.data_activity;
    lamps
}

fn observed_value_lamps(address: u16, data: u8) -> PanelLampSnapshot {
    let mut lamps = PanelLampSnapshot::default();
    for bit in 0..16 {
        lamps.address[bit] = if address & (1u16 << bit) != 0 { 1.0 } else { 0.0 };
    }
    for bit in 0..8 {
        lamps.data[bit] = if data & (1u8 << bit) != 0 { 1.0 } else { 0.0 };
    }
    lamps
}

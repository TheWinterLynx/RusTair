use std::collections::{BTreeMap, VecDeque};
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
    AltairZ80CpuMode, AltairZ80Registers, ClassicAltairRegisters, SimhLaunchConfig,
    SimhOperationalState, SimhSession, embedded_altair_launch_config,
    embedded_altairz80_launch_config, set_altairz80_switch_register_low, set_switch_register,
};

const MEMORY_BYTES: usize = 64 * 1024;
const WORKER_TICK: Duration = Duration::from_millis(1);
const PANEL_TICK: Duration = Duration::from_millis(16);
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
    SetSwitches(u16),
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
            state.log("SIMH runtime preparation happens on the worker thread; the UI never waits for it.");
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

    /// Product controls are deliberately fire-and-forget. If the worker has
    /// failed, record the transport error in its console instead of converting a
    /// backend integration problem into an egui/main-thread panic.
    fn enqueue(&self, command: Command) {
        if let Err(error) = self.tx.send(command) {
            self.write_shared(|state| state.error(format!("could not queue command: {error}")));
        }
    }
}

impl Drop for SimhThreadedBackend {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        // Never join here: FrontPanel teardown is allowed to finish off-thread.
    }
}

impl MachineBackend for SimhThreadedBackend {
    fn engine(&self) -> EmulationEngine { self.engine }

    fn name(&self) -> &'static str {
        match self.engine {
            EmulationEngine::SimhAltair => "Open SIMH classic Altair — async worker",
            EmulationEngine::SimhAltairZ80 => "Open SIMH AltairZ80 — async worker",
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

    // Hot-path UI calls: local memory only.
    fn cpu_state(&mut self) -> BackendResult<CpuState> { Ok(self.read_shared(|s| s.cpu)) }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { Ok(self.read_shared(|s| s.panel)) }
    fn service_execution(&mut self, _t_state_budget: u32) -> BackendResult<()> { Ok(()) }
    fn commit_panel_activity(&mut self, _dt: Duration) -> BackendResult<()> { Ok(()) }

    fn configure_memory(&mut self, _size: RamSize, _init: RamInit) -> BackendResult<()> { Ok(()) }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        self.write_shared(|state| {
            state.panel.powered = on;
            if !on {
                state.panel.running = false;
                state.panel.lamps = PanelLampSnapshot::default();
                state.console_available = false;
            }
            state.busy = on;
            state.log(if on { "POWER ON requested" } else { "POWER OFF requested" });
        });
        self.enqueue(Command::Power(on));
        Ok(())
    }
    fn power_with_historical_run_latch(&mut self, on: bool, _historical: bool) -> BackendResult<()> { self.power(on) }

    fn run(&mut self) -> BackendResult<()> {
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
        // Physical UI switch position changes immediately. Synchronizing the
        // SIMH pseudo-register is worker work and may briefly HALT/CONT SIMH.
        self.write_shared(|state| state.panel.switches = value);
        self.enqueue(Command::SetSwitches(value));
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

struct Worker {
    engine: EmulationEngine,
    launch: SimhLaunchConfig,
    session: Option<SimhSession>,
    bridge: Option<SimhM2SioBridge>,
    _serial_config: Option<SimhM2SioRuntimeConfig>,
    shared: Arc<Mutex<SharedState>>,
    switches: u16,
    panel_address: u16,
    panel_data: u8,
    pending_memory: BTreeMap<u16, u8>,
    last_panel_copy: Instant,
    last_running: bool,
    serial_in_glow: f32,
    serial_out_glow: f32,
}

impl Worker {
    fn new(engine: EmulationEngine, shared: Arc<Mutex<SharedState>>) -> BackendResult<Self> {
        {
            let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = true;
            state.log("Preparing embedded Open-SIMH runtime…");
        }
        match engine {
            EmulationEngine::SimhAltair => {
                let launch = embedded_altair_launch_config()
                    .map_err(|e| op_error("prepare embedded SIMH Altair", e.to_string()))?;
                let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                state.busy = false;
                state.log("SIMH worker ready (classic Altair). POWER remains operator-controlled.");
                drop(state);
                Ok(Self {
                    engine,
                    launch,
                    session: None,
                    bridge: None,
                    _serial_config: None,
                    shared,
                    switches: 0,
                    panel_address: 0,
                    panel_data: 0,
                    pending_memory: BTreeMap::new(),
                    last_panel_copy: Instant::now(),
                    last_running: false,
                    serial_in_glow: 0.0,
                    serial_out_glow: 0.0,
                })
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
                let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                state.busy = false;
                state.log("SIMH worker ready (AltairZ80 + private 88-2SIO bridge). POWER remains operator-controlled.");
                drop(state);
                Ok(Self {
                    engine,
                    launch,
                    session: None,
                    bridge: Some(bridge),
                    _serial_config: Some(serial_config),
                    shared,
                    switches: 0,
                    panel_address: 0,
                    panel_data: 0,
                    pending_memory: BTreeMap::new(),
                    last_panel_copy: Instant::now(),
                    last_running: false,
                    serial_in_glow: 0.0,
                    serial_out_glow: 0.0,
                })
            }
            _ => Err(BackendError::Unsupported { operation: "SIMH worker engine", engine }),
        }
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
        match command {
            Command::Power(on) => {
                let result = self.power(on);
                self.finish(if on { "POWER ON" } else { "POWER OFF" }, result);
            }
            Command::Running(run) => {
                let result = self.set_running(run);
                self.finish(if run { "RUN" } else { "STOP" }, result);
            }
            Command::Step => { let result = self.step(); self.finish("STEP", result); }
            Command::Reset => { let result = self.reset(); self.finish("RESET", result); }
            Command::Examine(next) => { let result = self.examine(next); self.finish(if next { "EXAMINE NEXT" } else { "EXAMINE" }, result); }
            Command::Deposit(next) => { let result = self.deposit(next); self.finish(if next { "DEPOSIT NEXT" } else { "DEPOSIT" }, result); }
            Command::SetSwitches(value) => { let result = self.set_switches(value); self.finish("sense switches", result); }
            Command::ConfigureSerial(board) => {
                if self.engine == EmulationEngine::SimhAltairZ80 && board != SerialBoard::TwoSio88 {
                    self.record_error("serial configuration", "embedded AltairZ80 is fixed to MITS 88-2SIO");
                }
            }
            Command::Load(address, bytes) => {
                let result = self.load(address, &bytes);
                self.finish("memory load", result);
            }
            Command::Write(address, value) => {
                let result = self.write(address, value).map(|_| ());
                self.finish("memory write", result);
            }
            Command::SerialRx(port, byte) => self.serial_rx(port, byte),
            Command::ClearSerial => {
                if let Some(bridge) = self.bridge.as_mut() { bridge.clear_queues(); }
            }
            Command::Console(command) => self.console(command),
            Command::Shutdown => {}
        }
    }

    fn finish(&mut self, operation: &'static str, result: BackendResult<()>) {
        if let Err(error) = result {
            self.record_error(operation, error.to_string());
        }
    }

    fn record_error(&mut self, operation: &'static str, detail: impl Into<String>) {
        let detail = detail.into();
        let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        state.busy = false;
        state.error(format!("{operation}: {detail}"));
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        if on && self.session.is_none() {
            {
                let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                state.busy = true;
                state.log("Starting embedded SIMH process and FrontPanel connection…");
            }
            let mut session = SimhSession::start(
                self.launch.executable(),
                self.launch.simulator_config(),
                self.launch.device_panel_count,
            )
            .map_err(|e| op_error("SIMH power on", e.to_string()))?;

            for (&address, &value) in &self.pending_memory {
                session
                    .write_byte(address, value)
                    .map_err(|e| op_error("apply pending SIMH memory", e.to_string()))?;
            }
            self.pending_memory.clear();
            self.session = Some(session);
            self.set_switches(self.switches)?;
            self.refresh_halted()?;
            let console_available = self.session.as_ref().is_some_and(SimhSession::console_available);
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = false;
            state.panel.powered = true;
            state.console_available = console_available;
            state.last_error = None;
            state.log("POWER ON complete.");
            if !console_available {
                state.log("SIMH Console: current DLL has no RusTair command extension; rebuild the embedded bundle to enable interactive SCP commands.");
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
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = false;
            state.panel.powered = false;
            state.panel.running = false;
            state.panel.lamps = PanelLampSnapshot::default();
            state.console_available = false;
            state.log("POWER OFF complete.");
        }
        Ok(())
    }

    fn set_running(&mut self, running: bool) -> BackendResult<()> {
        if self.session.is_none() { return Err(op_error("SIMH RUN/STOP", "simulator is powered off or still starting")); }
        {
            let session = self.session.as_mut().expect("checked SIMH session");
            if running {
                session.run().map_err(|e| op_error("SIMH RUN", e.to_string()))?;
            } else {
                session.halt().map_err(|e| op_error("SIMH HALT", e.to_string()))?;
            }
        }
        if !running { self.refresh_halted()?; }
        self.last_running = running;
        self.update_panel_state(running, None);
        Ok(())
    }

    fn step(&mut self) -> BackendResult<()> {
        if self.session.is_none() { return Err(op_error("SIMH STEP", "simulator is powered off or still starting")); }
        {
            let session = self.session.as_mut().expect("checked SIMH session");
            session.step().map_err(|e| op_error("SIMH STEP", e.to_string()))?;
        }
        self.refresh_halted()?;
        Ok(())
    }

    fn reset(&mut self) -> BackendResult<()> {
        self.panel_address = 0;
        if self.session.is_none() { return Ok(()); }
        let was_running = self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running);
        {
            let session = self.session.as_mut().expect("checked SIMH session");
            if was_running { session.halt().map_err(|e| op_error("SIMH RESET halt", e.to_string()))?; }
            session.deposit_register_u32("PC", 0).map_err(|e| op_error("SIMH RESET PC", e.to_string()))?;
            if was_running { session.run().map_err(|e| op_error("SIMH RESET resume", e.to_string()))?; }
        }
        self.last_running = was_running;
        if !was_running { self.refresh_halted()?; }
        Ok(())
    }

    fn examine(&mut self, next: bool) -> BackendResult<()> {
        if self.session.is_none() { return Err(op_error("SIMH EXAMINE", "simulator is powered off or still starting")); }
        let address = if next { self.panel_address.wrapping_add(1) } else { self.switches };
        {
            let session = self.session.as_mut().expect("checked SIMH session");
            if session.state() == SimhOperationalState::Running {
                return Err(op_error("SIMH EXAMINE", "front-panel EXAMINE requires STOP"));
            }
            self.panel_data = session.read_byte(address).map_err(|e| op_error("SIMH EXAMINE", e.to_string()))?;
            session.deposit_register_u32("PC", address as u32).map_err(|e| op_error("SIMH EXAMINE PC", e.to_string()))?;
        }
        self.panel_address = address;
        self.update_panel_state(false, Some(stopped_lamps(self.panel_address, self.panel_data)));
        Ok(())
    }

    fn deposit(&mut self, next: bool) -> BackendResult<()> {
        let address = if next { self.panel_address.wrapping_add(1) } else { self.panel_address };
        let value = self.switches as u8;
        if !self.write(address, value)? { return Ok(()); }
        if let Some(session) = self.session.as_mut() {
            session.deposit_register_u32("PC", address as u32).map_err(|e| op_error("SIMH DEPOSIT PC", e.to_string()))?;
        }
        self.panel_address = address;
        self.panel_data = value;
        self.update_panel_state(false, Some(stopped_lamps(address, value)));
        Ok(())
    }

    fn set_switches(&mut self, value: u16) -> BackendResult<()> {
        self.switches = value;
        if self.session.is_some() {
            let running = self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running);
            {
                let session = self.session.as_mut().expect("checked SIMH session");
                if running { session.halt().map_err(|e| op_error("SIMH switch halt", e.to_string()))?; }
                match self.engine {
                    EmulationEngine::SimhAltair => set_switch_register(session, value)
                        .map_err(|e| op_error("SIMH switch register", e.to_string()))?,
                    EmulationEngine::SimhAltairZ80 => set_altairz80_switch_register_low(session, value as u8)
                        .map_err(|e| op_error("SIMH switch register", e.to_string()))?,
                    _ => {}
                }
                if running { session.run().map_err(|e| op_error("SIMH switch resume", e.to_string()))?; }
            }
        }
        let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        shared.panel.switches = value;
        Ok(())
    }

    fn load(&mut self, base: u16, bytes: &[u8]) -> BackendResult<()> {
        let was_running = self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running);
        if was_running {
            self.session
                .as_mut()
                .expect("checked SIMH session")
                .halt()
                .map_err(|e| op_error("SIMH memory load halt", e.to_string()))?;
            self.last_running = false;
            self.update_panel_state(false, None);
        }

        if self.session.is_some() {
            {
                let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                state.busy = true;
                state.log(format!("Loading {} bytes at {:04X}h into SIMH…", bytes.len(), base));
            }
            self.session
                .as_mut()
                .expect("checked SIMH session")
                .load_bytes(base, bytes)
                .map_err(|e| op_error("SIMH memory load", e.to_string()))?;
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.busy = false;
            state.log(format!("Loaded {} bytes at {:04X}h.", bytes.len(), base));
        } else {
            for (offset, byte) in bytes.iter().copied().enumerate() {
                let Some(address) = u16::try_from(offset).ok().and_then(|o| base.checked_add(o)) else { break; };
                self.pending_memory.insert(address, byte);
            }
        }
        Ok(())
    }

    fn write(&mut self, address: u16, value: u8) -> BackendResult<bool> {
        if self.session.is_some() {
            if self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running) {
                return Ok(false);
            }
            self.session
                .as_mut()
                .expect("checked SIMH session")
                .write_byte(address, value)
                .map_err(|e| op_error("SIMH memory write", e.to_string()))?;
        } else {
            self.pending_memory.insert(address, value);
        }
        Ok(true)
    }

    fn serial_rx(&mut self, port: BackendSerialPort, byte: u8) {
        let index = port_index(port);
        if let Some(bridge) = self.bridge.as_mut() {
            if let Err(error) = bridge.queue_to_simh(port, byte) {
                self.record_error("serial RX queue", error.to_string());
            } else {
                self.serial_in_glow = 1.0;
            }
        }
        let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        shared.to_simh[index] = shared.to_simh[index].saturating_sub(1);
    }

    fn console(&mut self, command: String) {
        let command = command.trim().to_owned();
        if command.is_empty() { return; }
        {
            let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            state.log(format!("sim> {command}"));
        }
        let Some(session) = self.session.as_mut() else {
            self.record_error("SIMH console", "simulator is powered off or still starting");
            return;
        };
        match session.console_command(&command) {
            Ok(response) => {
                let mut state = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                if response.trim().is_empty() {
                    state.log("(command completed with no output)");
                } else {
                    for line in response.lines() { state.log(line.to_owned()); }
                }
            }
            Err(error) => self.record_error("SIMH console", error.to_string()),
        }
    }

    fn background(&mut self) {
        if let Some(bridge) = self.bridge.as_mut() {
            if let Err(error) = bridge.poll() {
                self.record_error("M2SIO bridge poll", error.to_string());
            }
            for port in BackendSerialPort::ALL {
                while let Some(byte) = bridge.pop_from_simh(port) {
                    let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                    let queue = &mut shared.from_simh[port_index(port)];
                    if queue.len() < 64 * 1024 { queue.push_back(byte); }
                    self.serial_out_glow = 1.0;
                }
            }
        }

        if self.session.is_none() || self.last_panel_copy.elapsed() < PANEL_TICK { return; }
        self.last_panel_copy = Instant::now();

        // state() is an in-DLL volatile state read. live_panel_sample() is a
        // local mutex copy populated by FrontPanel's callback thread. Neither
        // operation sends a wire command to SIMH.
        let state = self.session.as_ref().map(|s| s.state()).unwrap_or(SimhOperationalState::Halted);
        let running = state == SimhOperationalState::Running;
        let sample = self.session.as_ref().expect("checked SIMH session").live_panel_sample();

        self.panel_address = sample.pc;
        self.panel_data = sample.a;
        let lamps = sampled_lamps(&sample, running, self.serial_in_glow, self.serial_out_glow);
        {
            let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            match &mut shared.cpu {
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
            shared.panel.powered = true;
            shared.panel.running = running;
            shared.panel.switches = self.switches;
            shared.panel.address = sample.pc;
            shared.panel.data = sample.a;
            shared.panel.lamps = lamps;
        }

        self.serial_in_glow *= 0.70;
        self.serial_out_glow *= 0.70;

        if !running && self.last_running { let _ = self.refresh_halted(); }
        self.last_running = running;
    }

    fn refresh_halted(&mut self) -> BackendResult<()> {
        if self.session.is_none() { return Ok(()); }
        if self.session.as_ref().is_some_and(|s| s.state() == SimhOperationalState::Running) { return Ok(()); }

        let cpu = {
            let session = self.session.as_ref().expect("checked SIMH session");
            match self.engine {
                EmulationEngine::SimhAltair => {
                    let r = ClassicAltairRegisters::read(session)
                        .map_err(|e| op_error("SIMH register snapshot", e.to_string()))?;
                    self.panel_address = r.pc;
                    self.switches = r.switch_register;
                    CpuState::Intel8080(Intel8080State {
                        a: r.a,
                        b: (r.bc >> 8) as u8,
                        c: r.bc as u8,
                        d: (r.de >> 8) as u8,
                        e: r.de as u8,
                        h: (r.hl >> 8) as u8,
                        l: r.hl as u8,
                        flags: r.flags_8080(),
                        pc: r.pc,
                        sp: r.sp,
                        inte: r.inte,
                        halted: None,
                        total_t_states: None,
                    })
                }
                EmulationEngine::SimhAltairZ80 => {
                    let r = AltairZ80Registers::read(session)
                        .map_err(|e| op_error("SIMH AltairZ80 register snapshot", e.to_string()))?;
                    self.panel_address = r.pc;
                    r.to_cpu_state(AltairZ80CpuMode::Intel8080)
                }
                _ => CpuState::default(),
            }
        };

        self.panel_data = self
            .session
            .as_ref()
            .expect("checked SIMH session")
            .read_byte(self.panel_address)
            .unwrap_or(0);

        let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        shared.cpu = cpu;
        shared.panel.powered = true;
        shared.panel.running = false;
        shared.panel.switches = self.switches;
        shared.panel.address = self.panel_address;
        shared.panel.data = self.panel_data;
        shared.panel.lamps = stopped_lamps(self.panel_address, self.panel_data);
        Ok(())
    }

    fn update_panel_state(&self, running: bool, lamps: Option<PanelLampSnapshot>) {
        let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        shared.panel.powered = self.session.is_some();
        shared.panel.running = running;
        shared.panel.switches = self.switches;
        shared.panel.address = self.panel_address;
        shared.panel.data = self.panel_data;
        if let Some(lamps) = lamps { shared.panel.lamps = lamps; }
    }
}

fn sampled_lamps(
    sample: &super::SimhLivePanelSample,
    running: bool,
    serial_in: f32,
    serial_out: f32,
) -> PanelLampSnapshot {
    if !running { return stopped_lamps(sample.pc, sample.a); }

    let mut lamps = PanelLampSnapshot::default();
    lamps.address = sample.address_activity;
    lamps.data = sample.data_activity;

    // Open-SIMH does not expose the physical S-100 status latch cycle by cycle.
    // PC/A are genuine asynchronous samples. These status intensities are
    // explicitly presentation estimates; INP/OUT are driven by real bridge I/O.
    lamps.memr = 0.82;
    lamps.m1 = 0.46;
    lamps.wo = 0.78;
    lamps.inp = serial_in.clamp(0.0, 1.0) * 0.8;
    lamps.out = serial_out.clamp(0.0, 1.0) * 0.8;
    lamps
}

fn stopped_lamps(address: u16, data: u8) -> PanelLampSnapshot {
    let mut lamps = PanelLampSnapshot::default();
    for bit in 0..16 {
        lamps.address[bit] = if address & (1u16 << bit) != 0 { 1.0 } else { 0.0 };
    }
    for bit in 0..8 {
        lamps.data[bit] = if data & (1u8 << bit) != 0 { 1.0 } else { 0.0 };
    }
    lamps.memr = 1.0;
    lamps.m1 = 1.0;
    lamps.wo = 1.0;
    lamps.wait = 1.0;
    lamps
}

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::{CpuDiagnosticResult, PanelLampSnapshot};

use super::serial_bridge::{SimhM2SioBridge, SimhM2SioRuntimeConfig};
use super::{
    AltairZ80CpuMode, AltairZ80Registers, ClassicAltairRegisters, SimhLaunchConfig,
    SimhOperationalState, SimhSession, SimhTarget, embedded_altair_launch_config,
    embedded_altairz80_launch_config, set_altairz80_switch_register_low, set_switch_register,
};
use crate::backend::{
    BackendCapabilities, BackendError, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, EmulationEngine, FrontPanelState, Intel8080State, IoPortActivity, IoTraceSnapshot,
    MachineBackend,
};

const WORKER_TICK: Duration = Duration::from_millis(1);
const PANEL_SAMPLE_INTERVAL: Duration = Duration::from_millis(16);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MEMORY_BYTES: usize = 64 * 1024;

#[derive(Clone)]
struct SharedSnapshot {
    cpu: CpuState,
    panel: FrontPanelState,
    memory: Vec<u8>,
    serial_to_simh: [usize; 2],
    serial_from_simh: [VecDeque<u8>; 2],
    io_trace_enabled: bool,
}

impl SharedSnapshot {
    fn new(engine: EmulationEngine) -> Self {
        let cpu = match engine {
            EmulationEngine::SimhAltairZ80 => CpuState::Intel8080(Intel8080State::default()),
            _ => CpuState::Intel8080(Intel8080State::default()),
        };
        Self {
            cpu,
            panel: FrontPanelState::default(),
            memory: vec![0; MEMORY_BYTES],
            serial_to_simh: [0, 0],
            serial_from_simh: [VecDeque::new(), VecDeque::new()],
            io_trace_enabled: false,
        }
    }
}

enum WorkerCommand {
    Power(bool, mpsc::Sender<BackendResult<()>>),
    Run(bool, mpsc::Sender<BackendResult<()>>),
    Step(mpsc::Sender<BackendResult<()>>),
    Reset(mpsc::Sender<BackendResult<()>>),
    Clear(mpsc::Sender<BackendResult<()>>),
    PanelExamine(bool, mpsc::Sender<BackendResult<()>>),
    PanelDeposit(bool, mpsc::Sender<BackendResult<()>>),
    SetSwitch(u16, mpsc::Sender<BackendResult<()>>),
    ConfigureSerial(SerialBoard, mpsc::Sender<BackendResult<()>>),
    LoadBytes(u16, Vec<u8>, mpsc::Sender<BackendResult<()>>),
    WriteMemory(u16, u8, bool, mpsc::Sender<BackendResult<bool>>),
    SerialReceive(BackendSerialPort, u8),
    ClearSerial,
    Shutdown,
}

pub struct SimhThreadedBackend {
    engine: EmulationEngine,
    tx: mpsc::Sender<WorkerCommand>,
    shared: Arc<Mutex<SharedSnapshot>>,
}

impl SimhThreadedBackend {
    pub fn new(engine: EmulationEngine) -> BackendResult<Self> {
        if !matches!(engine, EmulationEngine::SimhAltair | EmulationEngine::SimhAltairZ80) {
            return Err(BackendError::Unsupported {
                operation: "threaded SIMH backend creation",
                engine,
            });
        }

        let shared = Arc::new(Mutex::new(SharedSnapshot::new(engine)));
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name(format!("rustair-simh-{}", match engine {
                EmulationEngine::SimhAltair => "altair",
                _ => "altairz80",
            }))
            .spawn(move || {
                let machine = WorkerMachine::new(engine, worker_shared);
                match machine {
                    Ok(mut machine) => {
                        let _ = ready_tx.send(Ok(()));
                        machine.run_loop(rx);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            })
            .map_err(|error| BackendError::Operation {
                operation: "spawn SIMH worker",
                detail: error.to_string(),
            })?;

        ready_rx.recv_timeout(COMMAND_TIMEOUT).map_err(|error| BackendError::Operation {
            operation: "start SIMH worker",
            detail: error.to_string(),
        })??;

        Ok(Self { engine, tx, shared })
    }

    fn request<T>(&self, make: impl FnOnce(mpsc::Sender<BackendResult<T>>) -> WorkerCommand) -> BackendResult<T> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx.send(make(reply_tx)).map_err(|error| BackendError::Operation {
            operation: "send SIMH worker command",
            detail: error.to_string(),
        })?;
        reply_rx.recv_timeout(COMMAND_TIMEOUT).map_err(|error| BackendError::Operation {
            operation: "wait for SIMH worker command",
            detail: error.to_string(),
        })?
    }

    fn port_index(port: BackendSerialPort) -> usize {
        match port { BackendSerialPort::Port0 => 0, BackendSerialPort::Port1 => 1 }
    }

    fn with_shared<T>(&self, f: impl FnOnce(&SharedSnapshot) -> T) -> T {
        let guard = self.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&guard)
    }

    fn with_shared_mut<T>(&self, f: impl FnOnce(&mut SharedSnapshot) -> T) -> T {
        let mut guard = self.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut guard)
    }
}

impl Drop for SimhThreadedBackend {
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerCommand::Shutdown);
        // Deliberately do not join here. If Windows or FrontPanel is in a slow
        // teardown path, dropping/switching an engine must never freeze egui.
    }
}

impl MachineBackend for SimhThreadedBackend {
    fn engine(&self) -> EmulationEngine { self.engine }
    fn name(&self) -> &'static str {
        match self.engine {
            EmulationEngine::SimhAltair => "Open SIMH classic Altair — threaded",
            EmulationEngine::SimhAltairZ80 => "Open SIMH AltairZ80 — threaded",
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

    fn cpu_state(&mut self) -> BackendResult<CpuState> { Ok(self.with_shared(|s| s.cpu)) }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { Ok(self.with_shared(|s| s.panel)) }
    fn configure_memory(&mut self, _size: RamSize, _init: RamInit) -> BackendResult<()> { Ok(()) }

    fn power(&mut self, on: bool) -> BackendResult<()> { self.request(|reply| WorkerCommand::Power(on, reply)) }
    fn power_with_historical_run_latch(&mut self, on: bool, _historical: bool) -> BackendResult<()> { self.power(on) }
    fn run(&mut self) -> BackendResult<()> { self.request(|reply| WorkerCommand::Run(true, reply)) }
    fn halt(&mut self) -> BackendResult<()> { self.request(|reply| WorkerCommand::Run(false, reply)) }
    fn step(&mut self) -> BackendResult<()> { self.request(WorkerCommand::Step) }
    fn service_execution(&mut self, _t_state_budget: u32) -> BackendResult<()> { Ok(()) }
    fn commit_panel_activity(&mut self, _dt: Duration) -> BackendResult<()> { Ok(()) }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { if run { self.run() } else { self.halt() } }
    fn release_run_stop(&mut self, _run: bool) -> BackendResult<()> { Ok(()) }
    fn assert_reset(&mut self) -> BackendResult<()> { self.request(WorkerCommand::Reset) }
    fn release_reset(&mut self) -> BackendResult<()> { Ok(()) }
    fn assert_clear(&mut self) -> BackendResult<()> { self.request(WorkerCommand::Clear) }
    fn release_clear(&mut self) -> BackendResult<()> { Ok(()) }
    fn request_hold(&mut self, _hold: bool) -> BackendResult<()> { Ok(()) }
    fn panel_examine(&mut self, next: bool) -> BackendResult<()> { self.request(|reply| WorkerCommand::PanelExamine(next, reply)) }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> { self.request(|reply| WorkerCommand::PanelDeposit(next, reply)) }
    fn protect_current_board(&mut self, _protected: bool) -> BackendResult<()> { Ok(()) }

    fn switch_register(&mut self) -> BackendResult<u16> { Ok(self.with_shared(|s| s.panel.switches)) }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        self.with_shared_mut(|s| s.panel.switches = value);
        self.request(|reply| WorkerCommand::SetSwitch(value, reply))
    }
    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        self.request(|reply| WorkerCommand::ConfigureSerial(board, reply))
    }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> {
        Ok(if self.engine == EmulationEngine::SimhAltairZ80 { SerialBoard::TwoSio88 } else { SerialBoard::TwoSio88 })
    }

    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        if self.engine != EmulationEngine::SimhAltairZ80 { return Ok(()); }
        let index = Self::port_index(port);
        self.with_shared_mut(|s| s.serial_to_simh[index] = s.serial_to_simh[index].saturating_add(1));
        self.tx.send(WorkerCommand::SerialReceive(port, byte)).map_err(|error| BackendError::Operation {
            operation: "queue SIMH serial byte",
            detail: error.to_string(),
        })
    }
    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        Ok(self.with_shared(|s| s.serial_to_simh[Self::port_index(port)] == 0))
    }
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> {
        Ok(self.with_shared(|s| s.serial_to_simh[Self::port_index(port)]))
    }
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        Ok(self.with_shared(|s| !s.serial_from_simh[Self::port_index(port)].is_empty()))
    }
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        Ok(self.with_shared(|s| s.serial_from_simh[Self::port_index(port)].front().copied()))
    }
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        Ok(self.with_shared_mut(|s| s.serial_from_simh[Self::port_index(port)].pop_front()))
    }
    fn clear_serial(&mut self) -> BackendResult<()> {
        self.with_shared_mut(|s| {
            s.serial_to_simh = [0, 0];
            s.serial_from_simh[0].clear();
            s.serial_from_simh[1].clear();
        });
        let _ = self.tx.send(WorkerCommand::ClearSerial);
        Ok(())
    }

    fn installed_ram_bytes(&mut self) -> BackendResult<usize> { Ok(MEMORY_BYTES) }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        Ok(Some(self.with_shared(|s| s.memory[address as usize])))
    }
    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> BackendResult<bool> {
        let written = self.request(|reply| WorkerCommand::WriteMemory(address, value, respect_protection, reply))?;
        if written { self.with_shared_mut(|s| s.memory[address as usize] = value); }
        Ok(written)
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        let owned = bytes.to_vec();
        self.request(|reply| WorkerCommand::LoadBytes(address, owned, reply))?;
        self.with_shared_mut(|s| {
            for (offset, byte) in bytes.iter().copied().enumerate() {
                let pos = address as usize + offset;
                if pos >= s.memory.len() { break; }
                s.memory[pos] = byte;
            }
        });
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
    fn io_trace_enabled(&mut self) -> BackendResult<bool> { Ok(self.with_shared(|s| s.io_trace_enabled)) }
    fn set_io_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> {
        self.with_shared_mut(|s| s.io_trace_enabled = enabled);
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

struct WorkerMachine {
    engine: EmulationEngine,
    launch: SimhLaunchConfig,
    session: Option<SimhSession>,
    bridge: Option<SimhM2SioBridge>,
    _serial_config: Option<SimhM2SioRuntimeConfig>,
    shared: Arc<Mutex<SharedSnapshot>>,
    switches: u16,
    panel_address: u16,
    panel_data: u8,
    pending_memory: BTreeMap<u16, u8>,
    last_panel_sample: Instant,
    last_running: bool,
    recent_serial_input: f32,
    recent_serial_output: f32,
}

impl WorkerMachine {
    fn new(engine: EmulationEngine, shared: Arc<Mutex<SharedSnapshot>>) -> BackendResult<Self> {
        match engine {
            EmulationEngine::SimhAltair => {
                let launch = embedded_altair_launch_config().map_err(|error| BackendError::Operation {
                    operation: "prepare embedded SIMH classic Altair",
                    detail: error.to_string(),
                })?;
                Ok(Self {
                    engine, launch, session: None, bridge: None, _serial_config: None, shared,
                    switches: 0, panel_address: 0, panel_data: 0, pending_memory: BTreeMap::new(),
                    last_panel_sample: Instant::now(), last_running: false,
                    recent_serial_input: 0.0, recent_serial_output: 0.0,
                })
            }
            EmulationEngine::SimhAltairZ80 => {
                let mut launch = embedded_altairz80_launch_config(AltairZ80CpuMode::Intel8080)
                    .map_err(|error| BackendError::Operation {
                        operation: "prepare embedded SIMH AltairZ80",
                        detail: error.to_string(),
                    })?;
                let bridge = SimhM2SioBridge::bind_loopback().map_err(|error| BackendError::Operation {
                    operation: "bind SIMH M2SIO bridge",
                    detail: error.to_string(),
                })?;
                let (port0, port1) = bridge.listen_ports();
                let serial_config = SimhM2SioRuntimeConfig::create(launch.simulator_config(), port0, port1)
                    .map_err(|error| BackendError::Operation {
                        operation: "prepare SIMH M2SIO config",
                        detail: error.to_string(),
                    })?;
                launch.simulator_config = serial_config.path().to_path_buf();
                Ok(Self {
                    engine, launch, session: None, bridge: Some(bridge), _serial_config: Some(serial_config), shared,
                    switches: 0, panel_address: 0, panel_data: 0, pending_memory: BTreeMap::new(),
                    last_panel_sample: Instant::now(), last_running: false,
                    recent_serial_input: 0.0, recent_serial_output: 0.0,
                })
            }
            _ => Err(BackendError::Unsupported { operation: "SIMH worker engine", engine }),
        }
    }

    fn run_loop(&mut self, rx: mpsc::Receiver<WorkerCommand>) {
        loop {
            match rx.recv_timeout(WORKER_TICK) {
                Ok(WorkerCommand::Shutdown) => break,
                Ok(command) => self.handle(command),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            self.service_background();
        }
        self.session.take();
        if let Some(bridge) = self.bridge.as_mut() { bridge.disconnect(); }
    }

    fn handle(&mut self, command: WorkerCommand) {
        match command {
            WorkerCommand::Power(on, reply) => { let _ = reply.send(self.power(on)); }
            WorkerCommand::Run(run, reply) => { let _ = reply.send(self.set_running(run)); }
            WorkerCommand::Step(reply) => { let _ = reply.send(self.step()); }
            WorkerCommand::Reset(reply) => { let _ = reply.send(self.reset()); }
            WorkerCommand::Clear(reply) => { if let Some(bridge) = self.bridge.as_mut() { bridge.clear_queues(); } let _ = reply.send(Ok(())); }
            WorkerCommand::PanelExamine(next, reply) => { let _ = reply.send(self.panel_examine(next)); }
            WorkerCommand::PanelDeposit(next, reply) => { let _ = reply.send(self.panel_deposit(next)); }
            WorkerCommand::SetSwitch(value, reply) => { let _ = reply.send(self.set_switches(value)); }
            WorkerCommand::ConfigureSerial(board, reply) => {
                let result = if self.engine == EmulationEngine::SimhAltairZ80 && board != SerialBoard::TwoSio88 {
                    Err(BackendError::Unsupported { operation: "MITS 88-SIO; embedded AltairZ80 uses 88-2SIO", engine: self.engine })
                } else { Ok(()) };
                let _ = reply.send(result);
            }
            WorkerCommand::LoadBytes(address, bytes, reply) => { let _ = reply.send(self.load_bytes(address, &bytes)); }
            WorkerCommand::WriteMemory(address, value, respect, reply) => { let _ = reply.send(self.write_memory(address, value, respect)); }
            WorkerCommand::SerialReceive(port, byte) => {
                let index = SimhThreadedBackend::port_index(port);
                if let Some(bridge) = self.bridge.as_mut() {
                    let _ = bridge.queue_to_simh(port, byte);
                    self.recent_serial_input = 1.0;
                }
                let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                shared.serial_to_simh[index] = shared.serial_to_simh[index].saturating_sub(1);
            }
            WorkerCommand::ClearSerial => {
                if let Some(bridge) = self.bridge.as_mut() { bridge.clear_queues(); }
            }
            WorkerCommand::Shutdown => {}
        }
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        match (on, self.session.is_some()) {
            (true, false) => {
                let mut session = SimhSession::start(self.launch.executable(), self.launch.simulator_config(), self.launch.device_panel_count)
                    .map_err(|error| self.error("SIMH power on", error.to_string()))?;
                for (&address, &value) in &self.pending_memory {
                    session.write_byte(address, value).map_err(|error| self.error("SIMH pending memory", error.to_string()))?;
                }
                self.pending_memory.clear();
                self.session = Some(session);
                self.set_switches(self.switches)?;
                self.refresh_stopped_registers()?;
                self.update_shared(false, None);
            }
            (false, true) => {
                self.session.take();
                if let Some(bridge) = self.bridge.as_mut() { bridge.disconnect(); }
                self.last_running = false;
                let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                shared.panel.powered = false;
                shared.panel.running = false;
                shared.panel.lamps = PanelLampSnapshot::default();
            }
            _ => {}
        }
        Ok(())
    }

    fn set_running(&mut self, run: bool) -> BackendResult<()> {
        let session = self.session.as_mut().ok_or_else(|| self.error("SIMH RUN/STOP", "simulator is powered off"))?;
        if run {
            session.run().map_err(|error| self.error("SIMH RUN", error.to_string()))?;
        } else {
            session.halt().map_err(|error| self.error("SIMH HALT", error.to_string()))?;
            self.refresh_stopped_registers()?;
        }
        self.last_running = run;
        self.update_shared(run, None);
        Ok(())
    }

    fn step(&mut self) -> BackendResult<()> {
        let session = self.session.as_mut().ok_or_else(|| self.error("SIMH STEP", "simulator is powered off"))?;
        session.step().map_err(|error| self.error("SIMH STEP", error.to_string()))?;
        self.refresh_stopped_registers()?;
        self.update_shared(false, None);
        Ok(())
    }

    fn reset(&mut self) -> BackendResult<()> {
        if let Some(session) = self.session.as_mut() {
            let was_running = session.state() == SimhOperationalState::Running;
            if was_running { session.halt().map_err(|e| self.error("SIMH RESET halt", e.to_string()))?; }
            session.deposit_register_u32("PC", 0).map_err(|e| self.error("SIMH RESET PC", e.to_string()))?;
            self.panel_address = 0;
            if was_running { session.run().map_err(|e| self.error("SIMH RESET resume", e.to_string()))?; }
            self.last_running = was_running;
        } else {
            self.panel_address = 0;
        }
        Ok(())
    }

    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        let session = self.session.as_mut().ok_or_else(|| self.error("SIMH EXAMINE", "simulator is powered off"))?;
        if session.state() == SimhOperationalState::Running {
            return Err(self.error("SIMH EXAMINE", "front-panel EXAMINE requires STOP"));
        }
        let address = if next { self.panel_address.wrapping_add(1) } else { self.switches };
        self.panel_data = session.read_byte(address).map_err(|e| self.error("SIMH EXAMINE", e.to_string()))?;
        session.deposit_register_u32("PC", address as u32).map_err(|e| self.error("SIMH EXAMINE PC", e.to_string()))?;
        self.panel_address = address;
        self.update_shared(false, None);
        Ok(())
    }

    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        let address = if next { self.panel_address.wrapping_add(1) } else { self.panel_address };
        let value = self.switches as u8;
        self.write_memory(address, value, false)?;
        if let Some(session) = self.session.as_mut() {
            session.deposit_register_u32("PC", address as u32).map_err(|e| self.error("SIMH DEPOSIT PC", e.to_string()))?;
        }
        self.panel_address = address;
        self.panel_data = value;
        self.update_shared(false, None);
        Ok(())
    }

    fn set_switches(&mut self, value: u16) -> BackendResult<()> {
        self.switches = value;
        if let Some(session) = self.session.as_mut() {
            let running = session.state() == SimhOperationalState::Running;
            if running { session.halt().map_err(|e| self.error("SIMH switch halt", e.to_string()))?; }
            match self.engine {
                EmulationEngine::SimhAltair => set_switch_register(session, value)
                    .map_err(|e| self.error("SIMH switch register", e.to_string()))?,
                EmulationEngine::SimhAltairZ80 => set_altairz80_switch_register_low(session, value as u8)
                    .map_err(|e| self.error("SIMH switch register", e.to_string()))?,
                _ => {}
            }
            if running { session.run().map_err(|e| self.error("SIMH switch resume", e.to_string()))?; }
        }
        let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        shared.panel.switches = value;
        Ok(())
    }

    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        if let Some(session) = self.session.as_mut() {
            if session.state() == SimhOperationalState::Running {
                return Err(self.error("SIMH memory load", "halt the simulator before loading memory"));
            }
            session.load_bytes(address, bytes).map_err(|e| self.error("SIMH memory load", e.to_string()))?;
        } else {
            for (offset, byte) in bytes.iter().copied().enumerate() {
                let Some(addr) = u16::try_from(offset).ok().and_then(|o| address.checked_add(o)) else { break; };
                self.pending_memory.insert(addr, byte);
            }
        }
        Ok(())
    }

    fn write_memory(&mut self, address: u16, value: u8, _respect: bool) -> BackendResult<bool> {
        if let Some(session) = self.session.as_mut() {
            if session.state() == SimhOperationalState::Running { return Ok(false); }
            session.write_byte(address, value).map_err(|e| self.error("SIMH memory write", e.to_string()))?;
        } else {
            self.pending_memory.insert(address, value);
        }
        Ok(true)
    }

    fn service_background(&mut self) {
        if let Some(bridge) = self.bridge.as_mut() {
            let _ = bridge.poll();
            for port in BackendSerialPort::ALL {
                while let Some(byte) = bridge.pop_from_simh(port) {
                    let index = SimhThreadedBackend::port_index(port);
                    let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                    if shared.serial_from_simh[index].len() < 64 * 1024 {
                        shared.serial_from_simh[index].push_back(byte);
                    }
                    self.recent_serial_output = 1.0;
                }
            }
        }

        if self.session.is_none() { return; }
        if self.last_panel_sample.elapsed() < PANEL_SAMPLE_INTERVAL { return; }
        self.last_panel_sample = Instant::now();

        let state = self.session.as_ref().map(SimhSession::state).unwrap_or(SimhOperationalState::Halted);
        let running = state == SimhOperationalState::Running;
        let sample = self.session.as_mut().and_then(|session| session.live_panel_sample().ok());
        if let Some(sample) = sample {
            self.panel_address = sample.pc;
            self.panel_data = sample.a;
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
            shared.panel.address = sample.pc;
            shared.panel.data = sample.a;
            shared.panel.switches = self.switches;
            shared.panel.lamps = sampled_lamps(&sample, running, self.recent_serial_input, self.recent_serial_output);
        }
        self.recent_serial_input *= 0.70;
        self.recent_serial_output *= 0.70;

        if !running && self.last_running {
            let _ = self.refresh_stopped_registers();
        }
        self.last_running = running;
    }

    fn refresh_stopped_registers(&mut self) -> BackendResult<()> {
        let Some(session) = self.session.as_ref() else { return Ok(()); };
        if session.state() == SimhOperationalState::Running { return Ok(()); }
        let cpu = match self.engine {
            EmulationEngine::SimhAltair => {
                let r = ClassicAltairRegisters::read(session).map_err(|e| self.error("SIMH register snapshot", e.to_string()))?;
                self.panel_address = r.pc;
                self.switches = r.switch_register;
                CpuState::Intel8080(Intel8080State {
                    a: r.a, b: (r.bc >> 8) as u8, c: r.bc as u8,
                    d: (r.de >> 8) as u8, e: r.de as u8,
                    h: (r.hl >> 8) as u8, l: r.hl as u8,
                    flags: r.flags_8080(), pc: r.pc, sp: r.sp, inte: r.inte,
                    halted: None, total_t_states: None,
                })
            }
            EmulationEngine::SimhAltairZ80 => {
                let r = AltairZ80Registers::read(session).map_err(|e| self.error("SIMH AltairZ80 register snapshot", e.to_string()))?;
                self.panel_address = r.pc;
                r.to_cpu_state(AltairZ80CpuMode::Intel8080)
            }
            _ => CpuState::default(),
        };
        self.panel_data = session.read_byte(self.panel_address).unwrap_or(0);
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

    fn update_shared(&self, running: bool, lamps: Option<PanelLampSnapshot>) {
        let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        shared.panel.powered = self.session.is_some();
        shared.panel.running = running;
        shared.panel.switches = self.switches;
        shared.panel.address = self.panel_address;
        shared.panel.data = self.panel_data;
        if let Some(lamps) = lamps { shared.panel.lamps = lamps; }
    }

    fn error(&self, operation: &'static str, detail: impl Into<String>) -> BackendError {
        BackendError::Operation { operation, detail: detail.into() }
    }
}

fn sampled_lamps(sample: &super::SimhLivePanelSample, running: bool, serial_in: f32, serial_out: f32) -> PanelLampSnapshot {
    if !running { return stopped_lamps(sample.pc, sample.a); }
    let mut lamps = PanelLampSnapshot::default();
    lamps.address = sample.address_activity;
    lamps.data = sample.data_activity;
    // Open-SIMH exposes architectural registers rather than the physical S-100
    // bus-status latch. Address/data brightness therefore comes from FrontPanel's
    // real accumulated register sampler; status lamps below are presentation
    // estimates for a running 8080 fetch/read mix, with real serial activity
    // contributing to INP/OUT. They are intentionally not advertised as exact
    // bus-cycle telemetry.
    lamps.memr = 0.82;
    lamps.m1 = 0.46;
    lamps.wo = 0.78;
    lamps.inp = serial_in.clamp(0.0, 1.0) * 0.8;
    lamps.out = serial_out.clamp(0.0, 1.0) * 0.8;
    lamps.wait = 0.0;
    lamps
}

fn stopped_lamps(address: u16, data: u8) -> PanelLampSnapshot {
    let mut lamps = PanelLampSnapshot::default();
    for bit in 0..16 { lamps.address[bit] = if address & (1 << bit) != 0 { 1.0 } else { 0.0 }; }
    for bit in 0..8 { lamps.data[bit] = if data & (1 << bit) != 0 { 1.0 } else { 0.0 }; }
    lamps.memr = 1.0;
    lamps.m1 = 1.0;
    lamps.wo = 1.0;
    lamps.wait = 1.0;
    lamps
}

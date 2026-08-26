use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::PanelLampSnapshot;

use super::serial_bridge::{
    SimhM2SioBridge, SimhM2SioRuntimeConfig, SimhSerialBridgeError,
};
use super::{
    AltairZ80CpuMode, AltairZ80Registers, SimhLaunchConfig, SimhOperationalState, SimhSession,
    SimhSessionError, SimhTarget, set_altairz80_switch_register_low,
};
use crate::backend::{
    BackendCapabilities, BackendError, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, EmulationEngine, FrontPanelState, Intel8080State, IoPortActivity, IoTraceSnapshot,
    MachineBackend, Z80State,
};

struct SimhM2SioRuntime {
    bridge: SimhM2SioBridge,
    config: SimhM2SioRuntimeConfig,
}

/// Open-SIMH `altairz80.exe` backend.
pub struct SimhAltairZ80Backend {
    launch: SimhLaunchConfig,
    cpu_mode: AltairZ80CpuMode,
    session: Option<SimhSession>,
    serial_runtime: Option<SimhM2SioRuntime>,
    panel_address_latch: u16,
    panel_data_latch: u8,
    switch_register_latch: u16,
    cpu_latch: CpuState,
    pending_memory: BTreeMap<u16, u8>,
    io_trace_enabled: bool,
}

impl SimhAltairZ80Backend {
    pub fn new_unpowered(
        launch: SimhLaunchConfig,
        cpu_mode: AltairZ80CpuMode,
    ) -> BackendResult<Self> {
        Self::validate_launch_target(&launch)?;
        Ok(Self::base(launch, cpu_mode, None))
    }

    pub fn new_unpowered_with_serial(
        launch: SimhLaunchConfig,
        cpu_mode: AltairZ80CpuMode,
    ) -> BackendResult<Self> {
        Self::validate_launch_target(&launch)?;
        let bridge = SimhM2SioBridge::bind_loopback()
            .map_err(|error| serial_backend_error("SIMH M2SIO bridge bind", error))?;
        let (port0, port1) = bridge.listen_ports();
        let runtime_config = SimhM2SioRuntimeConfig::create(
            launch.simulator_config(),
            port0,
            port1,
        )
        .map_err(|error| serial_backend_error("SIMH M2SIO runtime config", error))?;
        Ok(Self::base(
            launch,
            cpu_mode,
            Some(SimhM2SioRuntime {
                bridge,
                config: runtime_config,
            }),
        ))
    }

    fn base(
        launch: SimhLaunchConfig,
        cpu_mode: AltairZ80CpuMode,
        serial_runtime: Option<SimhM2SioRuntime>,
    ) -> Self {
        let cpu_latch = match cpu_mode {
            AltairZ80CpuMode::Intel8080 => CpuState::Intel8080(Intel8080State::default()),
            AltairZ80CpuMode::Z80 => CpuState::Z80(Z80State::default()),
        };
        Self {
            launch,
            cpu_mode,
            session: None,
            serial_runtime,
            panel_address_latch: 0,
            panel_data_latch: 0,
            switch_register_latch: 0,
            cpu_latch,
            pending_memory: BTreeMap::new(),
            io_trace_enabled: false,
        }
    }

    /// Immediate-start constructor retained for regression tests.
    pub fn launch(launch: SimhLaunchConfig, cpu_mode: AltairZ80CpuMode) -> BackendResult<Self> {
        let mut backend = Self::new_unpowered(launch, cpu_mode)?;
        backend.power(true)?;
        Ok(backend)
    }

    /// Immediate-start M2SIO constructor retained for the full serial smoke test.
    pub fn launch_with_serial(
        launch: SimhLaunchConfig,
        cpu_mode: AltairZ80CpuMode,
    ) -> BackendResult<Self> {
        let mut backend = Self::new_unpowered_with_serial(launch, cpu_mode)?;
        backend.power(true)?;
        Ok(backend)
    }

    fn validate_launch_target(launch: &SimhLaunchConfig) -> BackendResult<()> {
        if launch.target == SimhTarget::AltairZ80 {
            Ok(())
        } else {
            Err(BackendError::Unsupported {
                operation: "AltairZ80 backend creation for this SIMH target",
                engine: launch.target.engine(),
            })
        }
    }

    pub fn launch_config(&self) -> &SimhLaunchConfig { &self.launch }
    pub const fn cpu_mode(&self) -> AltairZ80CpuMode { self.cpu_mode }

    pub fn serial_connected(&self, port: BackendSerialPort) -> bool {
        self.session.is_some()
            && self.serial_runtime.as_ref()
                .is_some_and(|runtime| runtime.bridge.connected(port))
    }

    pub fn mount(&mut self, device: &str, switches: &str, path: &Path) -> BackendResult<()> {
        self.session_mut()?.mount(device, switches, path)
            .map_err(|error| backend_error("SIMH AltairZ80 media mount", error))
    }

    pub fn dismount(&mut self, device: &str) -> BackendResult<()> {
        self.session_mut()?.dismount(device)
            .map_err(|error| backend_error("SIMH AltairZ80 media dismount", error))
    }

    fn session(&self) -> BackendResult<&SimhSession> {
        self.session.as_ref().ok_or_else(|| BackendError::Operation {
            operation: "SIMH AltairZ80 access",
            detail: "simulator is powered off".into(),
        })
    }

    fn session_mut(&mut self) -> BackendResult<&mut SimhSession> {
        self.session.as_mut().ok_or_else(|| BackendError::Operation {
            operation: "SIMH AltairZ80 access",
            detail: "simulator is powered off".into(),
        })
    }

    fn serial_bridge_mut(&mut self) -> BackendResult<&mut SimhM2SioBridge> {
        self.serial_runtime
            .as_mut()
            .map(|runtime| &mut runtime.bridge)
            .ok_or_else(|| BackendError::Operation {
                operation: "SIMH M2SIO serial access",
                detail: "backend has no M2SIO raw TCP bridge".into(),
            })
    }

    fn active_simulator_config(&self) -> &Path {
        self.serial_runtime
            .as_ref()
            .map(|runtime| runtime.config.path())
            .unwrap_or_else(|| self.launch.simulator_config())
    }

    fn poll_serial(&mut self) -> BackendResult<()> {
        if self.session.is_none() { return Ok(()); }
        if let Some(runtime) = self.serial_runtime.as_mut() {
            runtime.bridge.poll()
                .map_err(|error| serial_backend_error("SIMH M2SIO poll", error))?;
        }
        Ok(())
    }

    fn registers(&self) -> BackendResult<AltairZ80Registers> {
        AltairZ80Registers::read(self.session()?)
            .map_err(|error| backend_error("SIMH AltairZ80 register snapshot", error))
    }

    fn operational_state(&self) -> BackendResult<SimhOperationalState> {
        let Some(session) = self.session.as_ref() else {
            return Ok(SimhOperationalState::Halted);
        };
        let state = session.state();
        if state == SimhOperationalState::Error {
            Err(BackendError::Operation {
                operation: "SIMH AltairZ80 state",
                detail: "FrontPanel connection entered the Error state".into(),
            })
        } else {
            Ok(state)
        }
    }

    fn require_stopped(&self, operation: &'static str) -> BackendResult<()> {
        if self.operational_state()? == SimhOperationalState::Running {
            Err(BackendError::Operation {
                operation,
                detail: "front-panel operation requires the simulator to be halted".into(),
            })
        } else { Ok(()) }
    }

    fn refresh_stopped_panel_latch(&mut self) -> BackendResult<()> {
        if self.session.is_none() { return Ok(()); }
        self.require_stopped("SIMH AltairZ80 panel refresh")?;
        let registers = self.registers()?;
        let data = self.session()?.read_byte(registers.pc)
            .map_err(|error| backend_error("SIMH AltairZ80 panel memory examine", error))?;
        self.panel_address_latch = registers.pc;
        self.panel_data_latch = data;
        self.switch_register_latch =
            (self.switch_register_latch & 0xff00) | u16::from(registers.switch_register_low);
        self.cpu_latch = registers.to_cpu_state(self.cpu_mode);
        Ok(())
    }

    fn set_pc_latch(&mut self, pc: u16) {
        match &mut self.cpu_latch {
            CpuState::Intel8080(state) => state.pc = pc,
            CpuState::Z80(state) => state.pc = pc,
        }
    }

    fn set_pc(&mut self, pc: u16) -> BackendResult<()> {
        if let Some(session) = self.session.as_mut() {
            session.deposit_register_u32("PC", u32::from(pc))
                .map_err(|error| backend_error("SIMH AltairZ80 PC deposit", error))?;
        }
        self.set_pc_latch(pc);
        Ok(())
    }

    fn apply_pending_memory(&mut self) -> BackendResult<()> {
        if self.pending_memory.is_empty() { return Ok(()); }
        let pending: Vec<(u16, u8)> = self.pending_memory.iter().map(|(&a, &v)| (a, v)).collect();
        for (address, value) in pending {
            self.session_mut()?.write_byte(address, value)
                .map_err(|error| backend_error("SIMH AltairZ80 pending memory deposit", error))?;
        }
        self.pending_memory.clear();
        Ok(())
    }

    fn set_switches_live(&mut self, value: u16) -> BackendResult<()> {
        self.switch_register_latch = value;
        if self.session.is_none() { return Ok(()); }
        let was_running = self.operational_state()? == SimhOperationalState::Running;
        if was_running {
            self.session_mut()?.halt()
                .map_err(|error| backend_error("SIMH AltairZ80 switch update halt", error))?;
        }
        set_altairz80_switch_register_low(self.session_mut()?, value as u8)
            .map_err(|error| backend_error("SIMH AltairZ80 switch register deposit", error))?;
        if was_running {
            self.session_mut()?.run()
                .map_err(|error| backend_error("SIMH AltairZ80 switch update resume", error))?;
        }
        Ok(())
    }
}

impl MachineBackend for SimhAltairZ80Backend {
    fn engine(&self) -> EmulationEngine { EmulationEngine::SimhAltairZ80 }

    fn name(&self) -> &'static str {
        match self.cpu_mode {
            AltairZ80CpuMode::Intel8080 => "Open SIMH AltairZ80 — 8080",
            AltairZ80CpuMode::Z80 => "Open SIMH AltairZ80 — Z80",
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
            serial_routing: self.serial_runtime.is_some(),
            disk_mount: true,
        }
    }

    fn execution_model(&self) -> BackendExecutionModel { BackendExecutionModel::ExternalProcess }

    fn cpu_state(&mut self) -> BackendResult<CpuState> {
        if self.session.is_some() && self.operational_state()? != SimhOperationalState::Running {
            self.cpu_latch = self.registers()?.to_cpu_state(self.cpu_mode);
        }
        Ok(self.cpu_latch)
    }

    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> {
        let running = self.session.is_some()
            && self.operational_state()? == SimhOperationalState::Running;
        Ok(FrontPanelState {
            powered: self.session.is_some(),
            running,
            switches: self.switch_register_latch,
            address: self.panel_address_latch,
            data: self.panel_data_latch,
            lamps: PanelLampSnapshot::default(),
            current_board_protected: false,
            ext_clear_asserted: false,
        })
    }

    fn configure_memory(&mut self, _size: RamSize, _init: RamInit) -> BackendResult<()> {
        // The embedded AltairZ80 production profile is fixed to 64 KiB.
        Ok(())
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        match (on, self.session.is_some()) {
            (false, true) => {
                self.session.take();
                if let Some(runtime) = self.serial_runtime.as_mut() {
                    runtime.bridge.disconnect();
                    runtime.bridge.clear_queues();
                }
                self.panel_address_latch = 0;
                self.panel_data_latch = 0;
                self.cpu_latch = match self.cpu_mode {
                    AltairZ80CpuMode::Intel8080 => CpuState::Intel8080(Intel8080State::default()),
                    AltairZ80CpuMode::Z80 => CpuState::Z80(Z80State::default()),
                };
                self.pending_memory.clear();
            }
            (true, false) => {
                let config = self.active_simulator_config().to_path_buf();
                let session = SimhSession::start(
                    self.launch.executable(), &config, self.launch.device_panel_count,
                ).map_err(|error| backend_error("SIMH AltairZ80 power on", error))?;
                self.session = Some(session);
                self.apply_pending_memory()?;
                self.set_switches_live(self.switch_register_latch)?;
                self.refresh_stopped_panel_latch()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn power_with_historical_run_latch(
        &mut self,
        on: bool,
        _historical: bool,
    ) -> BackendResult<()> {
        self.power(on)
    }

    fn run(&mut self) -> BackendResult<()> {
        self.session_mut()?.run().map_err(|error| backend_error("SIMH AltairZ80 RUN", error))
    }

    fn halt(&mut self) -> BackendResult<()> {
        self.session_mut()?.halt().map_err(|error| backend_error("SIMH AltairZ80 HALT", error))?;
        self.poll_serial()?;
        self.refresh_stopped_panel_latch()
    }

    fn step(&mut self) -> BackendResult<()> {
        self.require_stopped("SIMH AltairZ80 STEP")?;
        self.session_mut()?.step().map_err(|error| backend_error("SIMH AltairZ80 STEP", error))?;
        self.poll_serial()?;
        self.refresh_stopped_panel_latch()
    }

    fn service_execution(&mut self, _t_state_budget: u32) -> BackendResult<()> { self.poll_serial() }
    fn commit_panel_activity(&mut self, _dt: Duration) -> BackendResult<()> { Ok(()) }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { if run { self.run() } else { self.halt() } }
    fn release_run_stop(&mut self, _run: bool) -> BackendResult<()> { Ok(()) }

    fn assert_reset(&mut self) -> BackendResult<()> {
        if self.session.is_none() {
            self.set_pc_latch(0);
            self.panel_address_latch = 0;
            return Ok(());
        }
        let was_running = self.operational_state()? == SimhOperationalState::Running;
        if was_running {
            self.session_mut()?.halt().map_err(|error| backend_error("SIMH AltairZ80 RESET halt", error))?;
        }
        self.set_pc(0)?;
        self.refresh_stopped_panel_latch()?;
        Ok(())
    }
    fn release_reset(&mut self) -> BackendResult<()> { Ok(()) }
    fn assert_clear(&mut self) -> BackendResult<()> { self.clear_serial() }
    fn release_clear(&mut self) -> BackendResult<()> { Ok(()) }
    fn request_hold(&mut self, _hold: bool) -> BackendResult<()> { Ok(()) }

    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.require_stopped("front-panel EXAMINE")?;
        if self.session.is_none() { return Ok(()); }
        let address = if next { self.panel_address_latch.wrapping_add(1) } else { self.switch_register_latch };
        let data = self.session()?.read_byte(address)
            .map_err(|error| backend_error("SIMH AltairZ80 EXAMINE", error))?;
        self.set_pc(address)?;
        self.panel_address_latch = address;
        self.panel_data_latch = data;
        Ok(())
    }

    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        self.require_stopped("front-panel DEPOSIT")?;
        let address = if next { self.panel_address_latch.wrapping_add(1) } else { self.panel_address_latch };
        let value = self.switch_register_latch as u8;
        if let Some(session) = self.session.as_mut() {
            session.write_byte(address, value)
                .map_err(|error| backend_error("SIMH AltairZ80 DEPOSIT", error))?;
        } else {
            self.pending_memory.insert(address, value);
        }
        if next { self.set_pc(address)?; }
        self.panel_address_latch = address;
        self.panel_data_latch = value;
        Ok(())
    }

    fn protect_current_board(&mut self, _protected: bool) -> BackendResult<()> { Ok(()) }
    fn switch_register(&mut self) -> BackendResult<u16> { Ok(self.switch_register_latch) }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> { self.set_switches_live(value) }

    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        if board == SerialBoard::TwoSio88 {
            Ok(())
        } else {
            Err(BackendError::Unsupported {
                operation: "MITS 88-SIO; embedded AltairZ80 uses 88-2SIO ports",
                engine: EmulationEngine::SimhAltairZ80,
            })
        }
    }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> { Ok(SerialBoard::TwoSio88) }

    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        if self.session.is_none() { return Ok(()); }
        self.serial_bridge_mut()?.queue_to_simh(port, byte)
            .map_err(|error| serial_backend_error("SIMH M2SIO receive queue", error))?;
        self.poll_serial()
    }
    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        if self.session.is_none() { return Ok(true); }
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.to_simh_len(port) == 0)
    }
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> {
        if self.session.is_none() { return Ok(0); }
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.to_simh_len(port))
    }
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        if self.session.is_none() { return Ok(false); }
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.from_simh_len(port) != 0)
    }
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        if self.session.is_none() { return Ok(None); }
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.from_simh_front(port))
    }
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        if self.session.is_none() { return Ok(None); }
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.pop_from_simh(port))
    }
    fn clear_serial(&mut self) -> BackendResult<()> {
        if let Some(runtime) = self.serial_runtime.as_mut() { runtime.bridge.clear_queues(); }
        Ok(())
    }

    fn installed_ram_bytes(&mut self) -> BackendResult<usize> { Ok(64 * 1024) }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        if let Some(session) = self.session.as_ref() {
            if self.operational_state()? == SimhOperationalState::Running { return Ok(None); }
            return session.read_byte(address).map(Some)
                .map_err(|error| backend_error("SIMH AltairZ80 memory examine", error));
        }
        Ok(Some(*self.pending_memory.get(&address).unwrap_or(&0)))
    }
    fn write_memory(&mut self, address: u16, value: u8, _respect_protection: bool) -> BackendResult<bool> {
        if let Some(session) = self.session.as_mut() {
            if session.state() == SimhOperationalState::Running { return Ok(false); }
            session.write_byte(address, value)
                .map_err(|error| backend_error("SIMH AltairZ80 memory deposit", error))?;
        } else {
            self.pending_memory.insert(address, value);
        }
        Ok(true)
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        if let Some(session) = self.session.as_mut() {
            if session.state() == SimhOperationalState::Running {
                return Err(BackendError::Operation {
                    operation: "SIMH AltairZ80 memory load",
                    detail: "halt the simulator before loading memory".into(),
                });
            }
            return session.load_bytes(address, bytes)
                .map_err(|error| backend_error("SIMH AltairZ80 memory load", error));
        }
        for (offset, byte) in bytes.iter().copied().enumerate() {
            let Some(addr) = u16::try_from(offset).ok().and_then(|o| address.checked_add(o)) else { break; };
            self.pending_memory.insert(addr, byte);
        }
        Ok(())
    }
    fn memory_is_protected(&mut self, _address: u16) -> BackendResult<bool> { Ok(false) }
    fn clear_memory_protection(&mut self) -> BackendResult<()> { Ok(()) }
    fn clear_transient_memory_guards(&mut self) -> BackendResult<()> { Ok(()) }
    fn arm_basic32_full_memory_probe_guard(&mut self) -> BackendResult<bool> { Ok(false) }

    // The product UI polls diagnostic completion unconditionally. SIMH does not
    // currently implement RusTair's instruction/T-state meter, so passive poll
    // and cancellation must be harmless rather than propagating Unsupported.
    fn cancel_cpu_diagnostic_meter(&mut self) -> BackendResult<()> { Ok(()) }
    fn take_cpu_diagnostic_result(&mut self) -> BackendResult<Option<crate::machine::CpuDiagnosticResult>> { Ok(None) }

    fn peek_io_port(&mut self, _port: u8) -> BackendResult<u8> { Ok(0) }
    fn io_port_activity(&mut self, _port: u8) -> BackendResult<IoPortActivity> { Ok((None, None, 0, 0)) }
    fn io_trace_snapshot(&mut self) -> BackendResult<IoTraceSnapshot> { Ok(Vec::new()) }
    fn io_trace_enabled(&mut self) -> BackendResult<bool> { Ok(self.io_trace_enabled) }
    fn set_io_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> { self.io_trace_enabled = enabled; Ok(()) }
    fn clear_io_trace(&mut self) -> BackendResult<()> { Ok(()) }
    fn debugger_input_port(&mut self, _port: u8) -> BackendResult<u8> { Ok(0) }
    fn debugger_output_port(&mut self, _port: u8, _value: u8) -> BackendResult<()> { Ok(()) }
    fn debugger_inject_serial_rx(&mut self, _port: u8, _byte: u8) -> BackendResult<bool> { Ok(false) }
    fn debugger_clear_serial_rx(&mut self, _port: u8) -> BackendResult<bool> { Ok(false) }
    fn debugger_clear_serial_tx(&mut self, _port: u8) -> BackendResult<bool> { Ok(false) }
    fn debugger_complete_serial_tx(&mut self, _port: u8) -> BackendResult<Option<u8>> { Ok(None) }
}

fn backend_error(operation: &'static str, error: SimhSessionError) -> BackendError {
    BackendError::Operation { operation, detail: error.to_string() }
}

fn serial_backend_error(operation: &'static str, error: SimhSerialBridgeError) -> BackendError {
    BackendError::Operation { operation, detail: error.to_string() }
}

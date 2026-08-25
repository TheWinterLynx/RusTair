use std::path::Path;
use std::time::Duration;

use crate::config::SerialBoard;
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
    CpuState, EmulationEngine, FrontPanelState, MachineBackend,
};

struct SimhM2SioRuntime {
    bridge: SimhM2SioBridge,
    config: SimhM2SioRuntimeConfig,
}

/// Open-SIMH `altairz80.exe` backend.
///
/// The selected CPU personality is explicit because FrontPanel API v12 does
/// not expose a generic monitor-command function. The simulator configuration
/// supplied in `SimhLaunchConfig` must therefore contain the matching
/// `SET CPU 8080` or `SET CPU Z80` command before FrontPanel starts the process.
pub struct SimhAltairZ80Backend {
    launch: SimhLaunchConfig,
    cpu_mode: AltairZ80CpuMode,
    session: Option<SimhSession>,
    serial_runtime: Option<SimhM2SioRuntime>,
    panel_address_latch: u16,
    panel_data_latch: u8,
    /// AltairZ80 exports only an 8-bit SR register. RusTair keeps the physical
    /// Altair front-panel switch register as 16 bits locally and mirrors only
    /// the low byte into SIMH.
    switch_register_latch: u16,
}

impl SimhAltairZ80Backend {
    pub fn launch(launch: SimhLaunchConfig, cpu_mode: AltairZ80CpuMode) -> BackendResult<Self> {
        Self::validate_launch_target(&launch)?;
        let session = SimhSession::start(
            launch.executable(),
            launch.simulator_config(),
            launch.device_panel_count,
        )
        .map_err(|error| backend_error("SIMH AltairZ80 launch", error))?;

        let mut backend = Self {
            launch,
            cpu_mode,
            session: Some(session),
            serial_runtime: None,
            panel_address_latch: 0,
            panel_data_latch: 0,
            switch_register_latch: 0,
        };
        backend.refresh_stopped_panel_latch()?;
        Ok(backend)
    }

    /// Launch AltairZ80 with both MITS 88-2SIO channels bridged to RusTair over
    /// two private loopback-only raw TCP sockets. The caller's config file is
    /// never modified: a temporary overlay enables M2SIO0/1 and tells SIMH to
    /// connect outward to the two listeners using TMXR `;notelnet` mode.
    ///
    /// TMXR validates each outgoing destination with a short-lived connection
    /// while parsing ATTACH, but the persistent M2SIO connection is established
    /// later from `m2sio_svc()`/`tmxr_poll_conn()` while the simulator executes.
    /// Therefore launch must not require a persistent serial connection while
    /// the CPU is halted.
    pub fn launch_with_serial(
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

        let session = SimhSession::start(
            launch.executable(),
            runtime_config.path(),
            launch.device_panel_count,
        )
        .map_err(|error| backend_error("SIMH AltairZ80 serial launch", error))?;

        let mut backend = Self {
            launch,
            cpu_mode,
            session: Some(session),
            serial_runtime: Some(SimhM2SioRuntime {
                bridge,
                config: runtime_config,
            }),
            panel_address_latch: 0,
            panel_data_latch: 0,
            switch_register_latch: 0,
        };
        backend.refresh_stopped_panel_latch()?;
        Ok(backend)
    }

    fn validate_launch_target(launch: &SimhLaunchConfig) -> BackendResult<()> {
        if launch.target == SimhTarget::AltairZ80 {
            Ok(())
        } else {
            Err(BackendError::Unsupported {
                operation: "AltairZ80 backend launch for this SIMH target",
                engine: launch.target.engine(),
            })
        }
    }

    pub fn launch_config(&self) -> &SimhLaunchConfig { &self.launch }
    pub const fn cpu_mode(&self) -> AltairZ80CpuMode { self.cpu_mode }

    pub fn serial_connected(&self, port: BackendSerialPort) -> bool {
        match self.serial_runtime.as_ref() {
            Some(runtime) => runtime.bridge.connected(port),
            None => false,
        }
    }

    pub fn mount(
        &mut self,
        device: &str,
        switches: &str,
        path: &Path,
    ) -> BackendResult<()> {
        self.session_mut()?
            .mount(device, switches, path)
            .map_err(|error| backend_error("SIMH AltairZ80 media mount", error))
    }

    pub fn dismount(&mut self, device: &str) -> BackendResult<()> {
        self.session_mut()?
            .dismount(device)
            .map_err(|error| backend_error("SIMH AltairZ80 media dismount", error))
    }

    fn unsupported<T>(&self, operation: &'static str) -> BackendResult<T> {
        Err(BackendError::Unsupported {
            operation,
            engine: EmulationEngine::SimhAltairZ80,
        })
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
                detail: "backend was launched without the M2SIO raw TCP bridge".into(),
            })
    }

    fn active_simulator_config(&self) -> &Path {
        self.serial_runtime
            .as_ref()
            .map(|runtime| runtime.config.path())
            .unwrap_or_else(|| self.launch.simulator_config())
    }

    fn poll_serial(&mut self) -> BackendResult<()> {
        if let Some(runtime) = self.serial_runtime.as_mut() {
            runtime
                .bridge
                .poll()
                .map_err(|error| serial_backend_error("SIMH M2SIO poll", error))?;
        }
        Ok(())
    }

    fn registers(&self) -> BackendResult<AltairZ80Registers> {
        AltairZ80Registers::read(self.session()?)
            .map_err(|error| backend_error("SIMH AltairZ80 register snapshot", error))
    }

    fn operational_state(&self) -> BackendResult<SimhOperationalState> {
        let state = self.session()?.state();
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
        } else {
            Ok(())
        }
    }

    fn refresh_stopped_panel_latch(&mut self) -> BackendResult<()> {
        self.require_stopped("SIMH AltairZ80 panel refresh")?;
        let registers = self.registers()?;
        let data = self
            .session()?
            .read_byte(registers.pc)
            .map_err(|error| backend_error("SIMH AltairZ80 panel memory examine", error))?;
        self.panel_address_latch = registers.pc;
        self.panel_data_latch = data;
        self.switch_register_latch =
            (self.switch_register_latch & 0xff00) | u16::from(registers.switch_register_low);
        Ok(())
    }

    fn set_pc(&mut self, pc: u16) -> BackendResult<()> {
        self.session_mut()?
            .deposit_register_u32("PC", u32::from(pc))
            .map_err(|error| backend_error("SIMH AltairZ80 PC deposit", error))
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
        self.require_stopped("SIMH AltairZ80 CPU snapshot")?;
        Ok(self.registers()?.to_cpu_state(self.cpu_mode))
    }

    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> {
        let running = self.operational_state()? == SimhOperationalState::Running;
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

    fn power(&mut self, on: bool) -> BackendResult<()> {
        match (on, self.session.is_some()) {
            (false, true) => {
                self.session.take();
                if let Some(runtime) = self.serial_runtime.as_mut() {
                    runtime.bridge.disconnect();
                }
                self.panel_address_latch = 0;
                self.panel_data_latch = 0;
                self.switch_register_latch = 0;
            }
            (true, false) => {
                let config = self.active_simulator_config().to_path_buf();
                let session = SimhSession::start(
                    self.launch.executable(),
                    &config,
                    self.launch.device_panel_count,
                )
                .map_err(|error| backend_error("SIMH AltairZ80 power on", error))?;

                self.session = Some(session);
                self.refresh_stopped_panel_latch()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn power_with_historical_run_latch(
        &mut self,
        on: bool,
        historical: bool,
    ) -> BackendResult<()> {
        if historical {
            return self.unsupported("historical undefined RUN/STOP power-on latch");
        }
        self.power(on)
    }

    fn run(&mut self) -> BackendResult<()> {
        self.session_mut()?
            .run()
            .map_err(|error| backend_error("SIMH AltairZ80 RUN", error))
    }

    fn halt(&mut self) -> BackendResult<()> {
        self.session_mut()?
            .halt()
            .map_err(|error| backend_error("SIMH AltairZ80 HALT", error))?;
        self.poll_serial()?;
        self.refresh_stopped_panel_latch()
    }

    fn step(&mut self) -> BackendResult<()> {
        self.require_stopped("SIMH AltairZ80 STEP")?;
        self.session_mut()?
            .step()
            .map_err(|error| backend_error("SIMH AltairZ80 STEP", error))?;
        self.poll_serial()?;
        self.refresh_stopped_panel_latch()
    }

    fn service_execution(&mut self, _t_state_budget: u32) -> BackendResult<()> {
        let _ = self.operational_state()?;
        self.poll_serial()
    }

    fn commit_panel_activity(&mut self, _dt: Duration) -> BackendResult<()> { Ok(()) }

    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> {
        if run { self.run() } else { self.halt() }
    }

    fn release_run_stop(&mut self, _run: bool) -> BackendResult<()> { Ok(()) }

    fn assert_reset(&mut self) -> BackendResult<()> {
        self.unsupported("physical RESET without unintended instruction execution")
    }
    fn release_reset(&mut self) -> BackendResult<()> { self.unsupported("physical RESET release") }
    fn assert_clear(&mut self) -> BackendResult<()> { self.unsupported("S-100 EXT CLR") }
    fn release_clear(&mut self) -> BackendResult<()> { self.unsupported("S-100 EXT CLR") }
    fn request_hold(&mut self, _hold: bool) -> BackendResult<()> { self.unsupported("HOLD/HLDA") }

    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.require_stopped("front-panel EXAMINE")?;
        let address = if next {
            self.panel_address_latch.wrapping_add(1)
        } else {
            self.switch_register_latch
        };
        let data = self
            .session()?
            .read_byte(address)
            .map_err(|error| backend_error("SIMH AltairZ80 EXAMINE", error))?;
        self.set_pc(address)?;
        self.panel_address_latch = address;
        self.panel_data_latch = data;
        Ok(())
    }

    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        self.require_stopped("front-panel DEPOSIT")?;
        let address = if next {
            self.panel_address_latch.wrapping_add(1)
        } else {
            self.panel_address_latch
        };
        let value = self.switch_register_latch as u8;
        self.session_mut()?
            .write_byte(address, value)
            .map_err(|error| backend_error("SIMH AltairZ80 DEPOSIT", error))?;
        if next {
            self.set_pc(address)?;
        }
        self.panel_address_latch = address;
        self.panel_data_latch = value;
        Ok(())
    }

    fn protect_current_board(&mut self, _protected: bool) -> BackendResult<()> {
        self.unsupported("front-panel memory protection")
    }

    fn switch_register(&mut self) -> BackendResult<u16> { Ok(self.switch_register_latch) }

    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        self.require_stopped("SIMH AltairZ80 switch register deposit")?;
        set_altairz80_switch_register_low(self.session_mut()?, value as u8)
            .map_err(|error| backend_error("SIMH AltairZ80 switch register deposit", error))?;
        self.switch_register_latch = value;
        Ok(())
    }

    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        if board == SerialBoard::TwoSio88 {
            Ok(())
        } else {
            self.unsupported("MITS 88-SIO; AltairZ80 backend is routed through 88-2SIO ports")
        }
    }

    fn serial_board(&mut self) -> BackendResult<SerialBoard> { Ok(SerialBoard::TwoSio88) }

    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        self.serial_bridge_mut()?
            .queue_to_simh(port, byte)
            .map_err(|error| serial_backend_error("SIMH M2SIO receive queue", error))?;
        self.poll_serial()
    }

    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.to_simh_len(port) == 0)
    }

    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> {
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.to_simh_len(port))
    }

    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.from_simh_len(port) != 0)
    }

    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.from_simh_front(port))
    }

    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        self.poll_serial()?;
        Ok(self.serial_bridge_mut()?.pop_from_simh(port))
    }

    fn clear_serial(&mut self) -> BackendResult<()> {
        self.serial_bridge_mut()?.clear_queues();
        Ok(())
    }

    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        self.require_stopped("SIMH AltairZ80 memory examine")?;
        self.session()?
            .read_byte(address)
            .map(Some)
            .map_err(|error| backend_error("SIMH AltairZ80 memory examine", error))
    }

    fn write_memory(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> BackendResult<bool> {
        self.require_stopped("SIMH AltairZ80 memory deposit")?;
        if respect_protection {
            return self.unsupported("debugger write respecting front-panel protection");
        }
        self.session_mut()?
            .write_byte(address, value)
            .map_err(|error| backend_error("SIMH AltairZ80 memory deposit", error))?;
        Ok(true)
    }

    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.require_stopped("SIMH AltairZ80 memory load")?;
        self.session_mut()?
            .load_bytes(address, bytes)
            .map_err(|error| backend_error("SIMH AltairZ80 memory load", error))
    }
}

fn backend_error(operation: &'static str, error: SimhSessionError) -> BackendError {
    BackendError::Operation {
        operation,
        detail: error.to_string(),
    }
}

fn serial_backend_error(operation: &'static str, error: SimhSerialBridgeError) -> BackendError {
    BackendError::Operation {
        operation,
        detail: error.to_string(),
    }
}

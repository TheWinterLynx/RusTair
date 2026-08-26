use std::collections::BTreeMap;
use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::PanelLampSnapshot;

use super::{
    ClassicAltairRegisters, SimhLaunchConfig, SimhOperationalState, SimhSession, SimhSessionError,
    SimhTarget, set_switch_register,
};
use crate::backend::{
    BackendCapabilities, BackendError, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, EmulationEngine, FrontPanelState, Intel8080State, IoPortActivity, IoTraceSnapshot,
    MachineBackend,
};

pub struct SimhAltairBackend {
    launch: SimhLaunchConfig,
    session: Option<SimhSession>,
    panel_address_latch: u16,
    panel_data_latch: u8,
    switch_register_latch: u16,
    cpu_latch: Intel8080State,
    serial_board_latch: SerialBoard,
    pending_memory: BTreeMap<u16, u8>,
    io_trace_enabled: bool,
}

impl SimhAltairBackend {
    pub fn new_unpowered(launch: SimhLaunchConfig) -> BackendResult<Self> {
        if launch.target != SimhTarget::Altair {
            return Err(BackendError::Unsupported {
                operation: "classic Altair backend creation for this SIMH target",
                engine: launch.target.engine(),
            });
        }
        Ok(Self {
            launch,
            session: None,
            panel_address_latch: 0,
            panel_data_latch: 0,
            switch_register_latch: 0,
            cpu_latch: Intel8080State::default(),
            serial_board_latch: SerialBoard::TwoSio88,
            pending_memory: BTreeMap::new(),
            io_trace_enabled: false,
        })
    }

    /// Immediate-start constructor retained for regression tests and explicit
    /// developer use. Product engine selection uses `new_unpowered` instead.
    pub fn launch(launch: SimhLaunchConfig) -> BackendResult<Self> {
        let mut backend = Self::new_unpowered(launch)?;
        backend.power(true)?;
        Ok(backend)
    }

    pub fn launch_config(&self) -> &SimhLaunchConfig { &self.launch }

    pub fn mount(&mut self, device: &str, switches: &str, path: &std::path::Path) -> BackendResult<()> {
        self.session_mut()?.mount(device, switches, path)
            .map_err(|error| backend_error("SIMH media mount", error))
    }

    pub fn dismount(&mut self, device: &str) -> BackendResult<()> {
        self.session_mut()?.dismount(device)
            .map_err(|error| backend_error("SIMH media dismount", error))
    }

    fn session(&self) -> BackendResult<&SimhSession> {
        self.session.as_ref().ok_or_else(|| BackendError::Operation {
            operation: "SIMH access", detail: "simulator is powered off".into(),
        })
    }

    fn session_mut(&mut self) -> BackendResult<&mut SimhSession> {
        self.session.as_mut().ok_or_else(|| BackendError::Operation {
            operation: "SIMH access", detail: "simulator is powered off".into(),
        })
    }

    fn registers(&self) -> BackendResult<ClassicAltairRegisters> {
        ClassicAltairRegisters::read(self.session()?)
            .map_err(|error| backend_error("SIMH register snapshot", error))
    }

    fn operational_state(&self) -> BackendResult<SimhOperationalState> {
        let Some(session) = self.session.as_ref() else {
            return Ok(SimhOperationalState::Halted);
        };
        let state = session.state();
        if state == SimhOperationalState::Error {
            Err(BackendError::Operation {
                operation: "SIMH state",
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

    fn cpu_from_registers(r: ClassicAltairRegisters) -> Intel8080State {
        Intel8080State {
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
        }
    }

    fn refresh_stopped_panel_latch(&mut self) -> BackendResult<()> {
        if self.session.is_none() { return Ok(()); }
        self.require_stopped("SIMH classic Altair panel refresh")?;
        let registers = self.registers()?;
        let data = self.session()?.read_byte(registers.pc)
            .map_err(|error| backend_error("SIMH panel memory examine", error))?;
        self.panel_address_latch = registers.pc;
        self.panel_data_latch = data;
        self.switch_register_latch = registers.switch_register;
        self.cpu_latch = Self::cpu_from_registers(registers);
        Ok(())
    }

    fn set_pc(&mut self, pc: u16) -> BackendResult<()> {
        if let Some(session) = self.session.as_mut() {
            session.deposit_register_u32(super::altair_registers::PC, u32::from(pc))
                .map_err(|error| backend_error("SIMH PC deposit", error))?;
        }
        self.cpu_latch.pc = pc;
        Ok(())
    }

    fn apply_pending_memory(&mut self) -> BackendResult<()> {
        if self.pending_memory.is_empty() { return Ok(()); }
        let pending: Vec<(u16, u8)> = self.pending_memory.iter().map(|(&a, &v)| (a, v)).collect();
        for (address, value) in pending {
            self.session_mut()?.write_byte(address, value)
                .map_err(|error| backend_error("SIMH pending memory deposit", error))?;
        }
        self.pending_memory.clear();
        Ok(())
    }

    fn set_switches_live(&mut self, value: u16) -> BackendResult<()> {
        self.switch_register_latch = value;
        let Some(_) = self.session else { return Ok(()); };
        let was_running = self.operational_state()? == SimhOperationalState::Running;
        if was_running {
            self.session_mut()?.halt().map_err(|error| backend_error("SIMH switch update halt", error))?;
        }
        set_switch_register(self.session_mut()?, value)
            .map_err(|error| backend_error("SIMH switch register deposit", error))?;
        if was_running {
            self.session_mut()?.run().map_err(|error| backend_error("SIMH switch update resume", error))?;
        }
        Ok(())
    }
}

impl MachineBackend for SimhAltairBackend {
    fn engine(&self) -> EmulationEngine { EmulationEngine::SimhAltair }
    fn name(&self) -> &'static str { "Open SIMH classic Altair" }
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            front_panel: true,
            exact_bus_activity: false,
            exact_t_state_timing: false,
            memory_protection: false,
            hold_hlda: false,
            direct_memory_access: true,
            serial_routing: false,
            disk_mount: true,
        }
    }
    fn execution_model(&self) -> BackendExecutionModel { BackendExecutionModel::ExternalProcess }

    fn cpu_state(&mut self) -> BackendResult<CpuState> {
        if self.session.is_some() && self.operational_state()? != SimhOperationalState::Running {
            let registers = self.registers()?;
            self.cpu_latch = Self::cpu_from_registers(registers);
        }
        Ok(CpuState::Intel8080(self.cpu_latch))
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
        // The validated embedded classic simulator is built/configured with 64 KiB.
        // Product UI locks SIMH memory configuration to that value.
        Ok(())
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        match (on, self.session.is_some()) {
            (false, true) => {
                self.session.take();
                self.panel_address_latch = 0;
                self.panel_data_latch = 0;
                self.cpu_latch = Intel8080State::default();
                self.pending_memory.clear();
            }
            (true, false) => {
                let session = SimhSession::start(
                    self.launch.executable(), self.launch.simulator_config(), self.launch.device_panel_count,
                ).map_err(|error| backend_error("SIMH power on", error))?;
                self.session = Some(session);
                self.apply_pending_memory()?;
                self.set_switches_live(self.switch_register_latch)?;
                self.refresh_stopped_panel_latch()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn power_with_historical_run_latch(&mut self, on: bool, _historical: bool) -> BackendResult<()> {
        self.power(on)
    }

    fn run(&mut self) -> BackendResult<()> {
        self.session_mut()?.run().map_err(|error| backend_error("SIMH RUN", error))
    }
    fn halt(&mut self) -> BackendResult<()> {
        self.session_mut()?.halt().map_err(|error| backend_error("SIMH HALT", error))?;
        self.refresh_stopped_panel_latch()
    }
    fn step(&mut self) -> BackendResult<()> {
        self.require_stopped("SIMH STEP")?;
        self.session_mut()?.step().map_err(|error| backend_error("SIMH STEP", error))?;
        self.refresh_stopped_panel_latch()
    }
    fn service_execution(&mut self, _t_state_budget: u32) -> BackendResult<()> { Ok(()) }
    fn commit_panel_activity(&mut self, _dt: Duration) -> BackendResult<()> { Ok(()) }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { if run { self.run() } else { self.halt() } }
    fn release_run_stop(&mut self, _run: bool) -> BackendResult<()> { Ok(()) }

    fn assert_reset(&mut self) -> BackendResult<()> {
        if self.session.is_none() {
            self.cpu_latch.pc = 0;
            self.panel_address_latch = 0;
            return Ok(());
        }
        let was_running = self.operational_state()? == SimhOperationalState::Running;
        if was_running {
            self.session_mut()?.halt().map_err(|error| backend_error("SIMH RESET halt", error))?;
        }
        self.set_pc(0)?;
        self.refresh_stopped_panel_latch()?;
        Ok(())
    }
    fn release_reset(&mut self) -> BackendResult<()> { Ok(()) }
    fn assert_clear(&mut self) -> BackendResult<()> { Ok(()) }
    fn release_clear(&mut self) -> BackendResult<()> { Ok(()) }
    fn request_hold(&mut self, _hold: bool) -> BackendResult<()> { Ok(()) }

    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.require_stopped("front-panel EXAMINE")?;
        if self.session.is_none() { return Ok(()); }
        let address = if next { self.panel_address_latch.wrapping_add(1) } else { self.switch_register_latch };
        let data = self.session()?.read_byte(address).map_err(|error| backend_error("SIMH EXAMINE", error))?;
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
            session.write_byte(address, value).map_err(|error| backend_error("SIMH DEPOSIT", error))?;
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
        self.serial_board_latch = board;
        Ok(())
    }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> { Ok(self.serial_board_latch) }
    fn serial_receive(&mut self, _port: BackendSerialPort, _byte: u8) -> BackendResult<()> { Ok(()) }
    fn serial_rx_empty(&mut self, _port: BackendSerialPort) -> BackendResult<bool> { Ok(true) }
    fn serial_rx_len(&mut self, _port: BackendSerialPort) -> BackendResult<usize> { Ok(0) }
    fn serial_tx_busy(&mut self, _port: BackendSerialPort) -> BackendResult<bool> { Ok(false) }
    fn serial_tx_front(&mut self, _port: BackendSerialPort) -> BackendResult<Option<u8>> { Ok(None) }
    fn serial_tx_complete(&mut self, _port: BackendSerialPort) -> BackendResult<Option<u8>> { Ok(None) }
    fn clear_serial(&mut self) -> BackendResult<()> { Ok(()) }

    fn installed_ram_bytes(&mut self) -> BackendResult<usize> { Ok(64 * 1024) }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        if let Some(session) = self.session.as_ref() {
            if self.operational_state()? == SimhOperationalState::Running {
                return Ok(None);
            }
            return session.read_byte(address).map(Some).map_err(|error| backend_error("SIMH memory examine", error));
        }
        Ok(Some(*self.pending_memory.get(&address).unwrap_or(&0)))
    }
    fn write_memory(&mut self, address: u16, value: u8, _respect_protection: bool) -> BackendResult<bool> {
        if let Some(session) = self.session.as_mut() {
            if session.state() == SimhOperationalState::Running { return Ok(false); }
            session.write_byte(address, value).map_err(|error| backend_error("SIMH memory deposit", error))?;
        } else {
            self.pending_memory.insert(address, value);
        }
        Ok(true)
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        if let Some(session) = self.session.as_mut() {
            if session.state() == SimhOperationalState::Running {
                return Err(BackendError::Operation { operation: "SIMH memory load", detail: "halt the simulator before loading memory".into() });
            }
            return session.load_bytes(address, bytes).map_err(|error| backend_error("SIMH memory load", error));
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

    // CPU diagnostics are implemented by the native Rust cores. The application
    // polls this result every frame even when no diagnostic is active, so SIMH
    // must answer the passive query harmlessly instead of inheriting Unsupported.
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

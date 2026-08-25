use std::time::Duration;

use crate::config::SerialBoard;
use crate::machine::PanelLampSnapshot;

use super::{
    ClassicAltairRegisters, SimhLaunchConfig, SimhOperationalState, SimhSession, SimhSessionError,
    SimhTarget, set_switch_register,
};
use crate::backend::{
    BackendCapabilities, BackendError, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, EmulationEngine, FrontPanelState, Intel8080State, MachineBackend,
};

pub struct SimhAltairBackend {
    launch: SimhLaunchConfig,
    session: Option<SimhSession>,
    panel_address_latch: u16,
    panel_data_latch: u8,
    switch_register_latch: u16,
}

impl SimhAltairBackend {
    pub fn launch(launch: SimhLaunchConfig) -> BackendResult<Self> {
        if launch.target != SimhTarget::Altair {
            return Err(BackendError::Unsupported {
                operation: "classic Altair backend launch for this SIMH target",
                engine: launch.target.engine(),
            });
        }
        let session = SimhSession::start(
            launch.executable(), launch.simulator_config(), launch.device_panel_count,
        ).map_err(|error| backend_error("SIMH launch", error))?;
        let mut backend = Self {
            launch,
            session: Some(session),
            panel_address_latch: 0,
            panel_data_latch: 0,
            switch_register_latch: 0,
        };
        backend.refresh_stopped_panel_latch()?;
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

    fn unsupported<T>(&self, operation: &'static str) -> BackendResult<T> {
        Err(BackendError::Unsupported { operation, engine: EmulationEngine::SimhAltair })
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
        let state = self.session()?.state();
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

    fn refresh_stopped_panel_latch(&mut self) -> BackendResult<()> {
        let registers = self.registers()?;
        let data = self.session()?.read_byte(registers.pc)
            .map_err(|error| backend_error("SIMH panel memory examine", error))?;
        self.panel_address_latch = registers.pc;
        self.panel_data_latch = data;
        self.switch_register_latch = registers.switch_register;
        Ok(())
    }

    fn set_pc(&mut self, pc: u16) -> BackendResult<()> {
        self.session_mut()?.deposit_register_u32(super::altair_registers::PC, u32::from(pc))
            .map_err(|error| backend_error("SIMH PC deposit", error))
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
        let r = self.registers()?;
        Ok(CpuState::Intel8080(Intel8080State {
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
        }))
    }

    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> {
        let running = self.operational_state()? == SimhOperationalState::Running;

        // FrontPanel's generic EXAMINE operations are deliberately rejected
        // while the simulator is running.  The classic SIMH backend also has
        // no exact bus-activity feed, so do not fabricate live PC/RAM samples
        // by attempting register or memory reads here.  While running, expose
        // the last stopped front-panel latches; HALT and STEP refresh them from
        // the simulator before returning.
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
                self.panel_address_latch = 0;
                self.panel_data_latch = 0;
                self.switch_register_latch = 0;
            }
            (true, false) => {
                let session = SimhSession::start(
                    self.launch.executable(), self.launch.simulator_config(), self.launch.device_panel_count,
                ).map_err(|error| backend_error("SIMH power on", error))?;
                self.session = Some(session);
                self.refresh_stopped_panel_latch()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> {
        if historical { return self.unsupported("historical undefined RUN/STOP power-on latch"); }
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
    fn service_execution(&mut self, _t_state_budget: u32) -> BackendResult<()> {
        let _ = self.operational_state()?;
        Ok(())
    }
    fn commit_panel_activity(&mut self, _dt: Duration) -> BackendResult<()> { Ok(()) }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { if run { self.run() } else { self.halt() } }
    fn release_run_stop(&mut self, _run: bool) -> BackendResult<()> { Ok(()) }
    fn assert_reset(&mut self) -> BackendResult<()> { self.unsupported("physical RESET without unintended instruction execution") }
    fn release_reset(&mut self) -> BackendResult<()> { self.unsupported("physical RESET release") }
    fn assert_clear(&mut self) -> BackendResult<()> { self.unsupported("S-100 EXT CLR") }
    fn release_clear(&mut self) -> BackendResult<()> { self.unsupported("S-100 EXT CLR") }
    fn request_hold(&mut self, _hold: bool) -> BackendResult<()> { self.unsupported("HOLD/HLDA") }

    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.require_stopped("front-panel EXAMINE")?;
        let address = if next { self.panel_address_latch.wrapping_add(1) } else { self.registers()?.switch_register };
        let data = self.session()?.read_byte(address).map_err(|error| backend_error("SIMH EXAMINE", error))?;
        self.set_pc(address)?;
        self.panel_address_latch = address;
        self.panel_data_latch = data;
        Ok(())
    }

    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        self.require_stopped("front-panel DEPOSIT")?;
        let switches = self.registers()?.switch_register;
        let address = if next { self.panel_address_latch.wrapping_add(1) } else { self.panel_address_latch };
        let value = switches as u8;
        self.session_mut()?.write_byte(address, value).map_err(|error| backend_error("SIMH DEPOSIT", error))?;
        if next { self.set_pc(address)?; }
        self.panel_address_latch = address;
        self.panel_data_latch = value;
        Ok(())
    }

    fn protect_current_board(&mut self, _protected: bool) -> BackendResult<()> { self.unsupported("front-panel memory protection") }
    fn switch_register(&mut self) -> BackendResult<u16> {
        if self.operational_state()? == SimhOperationalState::Running {
            Ok(self.switch_register_latch)
        } else {
            let switches = self.registers()?.switch_register;
            self.switch_register_latch = switches;
            Ok(switches)
        }
    }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        self.require_stopped("SIMH switch register deposit")?;
        set_switch_register(self.session_mut()?, value)
            .map_err(|error| backend_error("SIMH switch register deposit", error))?;
        self.switch_register_latch = value;
        Ok(())
    }
    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        if board == SerialBoard::TwoSio88 { Ok(()) } else { self.unsupported("MITS 88-SIO; classic SIMH ALTAIR is fixed to 88-2SIO") }
    }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> { Ok(SerialBoard::TwoSio88) }
    fn serial_receive(&mut self, _port: BackendSerialPort, _byte: u8) -> BackendResult<()> { self.unsupported("RusTair serial endpoint routing to SIMH console/PTR") }
    fn serial_rx_empty(&mut self, _port: BackendSerialPort) -> BackendResult<bool> { self.unsupported("RusTair serial endpoint routing to SIMH console/PTR") }
    fn serial_rx_len(&mut self, _port: BackendSerialPort) -> BackendResult<usize> { self.unsupported("RusTair serial endpoint routing to SIMH console/PTR") }
    fn serial_tx_busy(&mut self, _port: BackendSerialPort) -> BackendResult<bool> { self.unsupported("RusTair serial endpoint routing to SIMH console/PTR") }
    fn serial_tx_front(&mut self, _port: BackendSerialPort) -> BackendResult<Option<u8>> { self.unsupported("RusTair serial endpoint routing to SIMH console/PTR") }
    fn serial_tx_complete(&mut self, _port: BackendSerialPort) -> BackendResult<Option<u8>> { self.unsupported("RusTair serial endpoint routing to SIMH console/PTR") }
    fn clear_serial(&mut self) -> BackendResult<()> { self.unsupported("RusTair serial endpoint routing to SIMH console/PTR") }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        self.session()?.read_byte(address).map(Some).map_err(|error| backend_error("SIMH memory examine", error))
    }
    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> BackendResult<bool> {
        if respect_protection { return self.unsupported("debugger write respecting front-panel protection"); }
        self.session_mut()?.write_byte(address, value).map_err(|error| backend_error("SIMH memory deposit", error))?;
        Ok(true)
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.session_mut()?.load_bytes(address, bytes).map_err(|error| backend_error("SIMH memory load", error))
    }
}

fn backend_error(operation: &'static str, error: SimhSessionError) -> BackendError {
    BackendError::Operation { operation, detail: error.to_string() }
}

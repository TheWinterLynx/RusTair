use std::ffi::{CStr, CString, NulError, c_void};
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;

use super::{ffi, runtime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimhOperationalState {
    Halted,
    Running,
    Error,
}

#[derive(Debug)]
pub enum SimhSessionError {
    InteriorNul(NulError),
    Runtime(runtime::SimhRuntimeError),
    FrontPanelLoad(ffi::FrontPanelLoadError),
    StartFailed(String),
    Api {
        operation: &'static str,
        detail: String,
    },
}

impl fmt::Display for SimhSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InteriorNul(error) => write!(f, "SIMH string contains an interior NUL: {error}"),
            Self::Runtime(error) => write!(f, "SIMH runtime preparation failed: {error}"),
            Self::FrontPanelLoad(error) => write!(f, "SIMH FrontPanel load failed: {error}"),
            Self::StartFailed(detail) => write!(f, "failed to start SIMH: {detail}"),
            Self::Api { operation, detail } => write!(f, "SIMH {operation} failed: {detail}"),
        }
    }
}

impl std::error::Error for SimhSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InteriorNul(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::FrontPanelLoad(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NulError> for SimhSessionError {
    fn from(value: NulError) -> Self { Self::InteriorNul(value) }
}

impl From<runtime::SimhRuntimeError> for SimhSessionError {
    fn from(value: runtime::SimhRuntimeError) -> Self { Self::Runtime(value) }
}

impl From<ffi::FrontPanelLoadError> for SimhSessionError {
    fn from(value: ffi::FrontPanelLoadError) -> Self { Self::FrontPanelLoad(value) }
}

/// RAII owner for one Open-SIMH FrontPanel connection.
///
/// The C API creates a simulator process and internal communication threads.
/// The FrontPanel DLL itself is resolved dynamically from RusTair's embedded
/// runtime bundle, so no SIMH import library or external installation is needed
/// at compile time. The handle remains deliberately !Send + !Sync.
pub struct SimhSession {
    panel: NonNull<ffi::PANEL>,
    api: ffi::FrontPanelApi,
    _not_send_sync: PhantomData<Rc<()>>,
}

impl SimhSession {
    pub fn start(
        simulator: &Path,
        config: &Path,
        device_panel_count: usize,
    ) -> Result<Self, SimhSessionError> {
        Self::start_impl(simulator, config, device_panel_count, None)
    }

    /// Start a session with Open-SIMH FrontPanel's own wire/debug logging enabled.
    /// Intended for integration diagnostics; normal product operation uses `start`.
    pub fn start_debug(
        simulator: &Path,
        config: &Path,
        device_panel_count: usize,
        debug_file: &Path,
    ) -> Result<Self, SimhSessionError> {
        Self::start_impl(simulator, config, device_panel_count, Some(debug_file))
    }

    fn start_impl(
        simulator: &Path,
        config: &Path,
        device_panel_count: usize,
        debug_file: Option<&Path>,
    ) -> Result<Self, SimhSessionError> {
        let frontpanel_path = runtime::frontpanel_dll_path()?;
        let api = ffi::FrontPanelApi::load(&frontpanel_path)?;
        let simulator = path_cstring(simulator)?;
        let config = path_cstring(config)?;
        let debug = debug_file.map(path_cstring).transpose()?;
        let raw = unsafe {
            match debug.as_ref() {
                Some(debug) => api.start_simulator_debug(
                    simulator.as_ptr(),
                    config.as_ptr(),
                    device_panel_count,
                    debug.as_ptr(),
                ),
                None => api.start_simulator(
                    simulator.as_ptr(),
                    config.as_ptr(),
                    device_panel_count,
                ),
            }
        };
        let panel = NonNull::new(raw)
            .ok_or_else(|| SimhSessionError::StartFailed(last_error_text(&api)))?;
        Ok(Self {
            panel,
            api,
            _not_send_sync: PhantomData,
        })
    }

    #[inline]
    fn raw(&self) -> *mut ffi::PANEL { self.panel.as_ptr() }

    fn check(&self, operation: &'static str, status: i32) -> Result<(), SimhSessionError> {
        if status == 0 {
            Ok(())
        } else {
            Err(SimhSessionError::Api {
                operation,
                detail: last_error_text(&self.api),
            })
        }
    }

    pub fn state(&self) -> SimhOperationalState {
        match unsafe { self.api.get_state(self.raw()) } {
            ffi::OperationalState::Halt => SimhOperationalState::Halted,
            ffi::OperationalState::Run => SimhOperationalState::Running,
            ffi::OperationalState::Error => SimhOperationalState::Error,
        }
    }

    pub fn halt(&mut self) -> Result<(), SimhSessionError> {
        let status = unsafe { self.api.exec_halt(self.raw()) };
        self.check("halt", status)
    }

    pub fn run(&mut self) -> Result<(), SimhSessionError> {
        let status = unsafe { self.api.exec_run(self.raw()) };
        self.check("run", status)
    }

    /// Reset all SIMH devices and start instruction execution.
    pub fn start_from_reset(&mut self) -> Result<(), SimhSessionError> {
        let status = unsafe { self.api.exec_start(self.raw()) };
        self.check("start", status)
    }

    pub fn step(&mut self) -> Result<(), SimhSessionError> {
        let status = unsafe { self.api.exec_step(self.raw()) };
        self.check("step", status)
    }

    pub fn boot(&mut self, device: &str) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let status = unsafe { self.api.exec_boot(self.raw(), device.as_ptr()) };
        self.check("boot", status)
    }

    /// Generic FrontPanel EXAMINE by register name or simulator-formatted address.
    pub fn examine_u32(&self, name_or_addr: &str) -> Result<u32, SimhSessionError> {
        let name_or_addr = CString::new(name_or_addr)?;
        let mut value = 0u32;
        let status = unsafe {
            self.api.gen_examine(
                self.raw(),
                name_or_addr.as_ptr(),
                size_of::<u32>(),
                (&mut value as *mut u32).cast::<c_void>(),
            )
        };
        self.check("generic examine", status)?;
        Ok(value)
    }

    /// Generic FrontPanel DEPOSIT by register name or simulator-formatted address.
    pub fn deposit_u32(
        &mut self,
        name_or_addr: &str,
        value: u32,
    ) -> Result<(), SimhSessionError> {
        let name_or_addr = CString::new(name_or_addr)?;
        let status = unsafe {
            self.api.gen_deposit(
                self.raw(),
                name_or_addr.as_ptr(),
                size_of::<u32>(),
                (&value as *const u32).cast::<c_void>(),
            )
        };
        self.check("generic deposit", status)
    }

    pub fn examine_register_u32(&self, name: &str) -> Result<u32, SimhSessionError> {
        self.examine_u32(name)
    }

    pub fn deposit_register_u32(
        &mut self,
        name: &str,
        value: u32,
    ) -> Result<(), SimhSessionError> {
        self.deposit_u32(name, value)
    }

    pub fn read_byte(&self, address: u16) -> Result<u8, SimhSessionError> {
        let address = u32::from(address);
        let mut value = 0u8;
        let status = unsafe {
            self.api.mem_examine(
                self.raw(),
                size_of::<u32>(),
                (&address as *const u32).cast::<c_void>(),
                size_of::<u8>(),
                (&mut value as *mut u8).cast::<c_void>(),
            )
        };
        self.check("memory examine", status)?;
        Ok(value)
    }

    pub fn write_byte(&mut self, address: u16, value: u8) -> Result<(), SimhSessionError> {
        let address = u32::from(address);
        let status = unsafe {
            self.api.mem_deposit(
                self.raw(),
                size_of::<u32>(),
                (&address as *const u32).cast::<c_void>(),
                size_of::<u8>(),
                (&value as *const u8).cast::<c_void>(),
            )
        };
        self.check("memory deposit", status)
    }

    /// Deliberately byte-wise. FrontPanel's value size describes one addressed
    /// host value rather than a contiguous transfer length.
    pub fn load_bytes(&mut self, base: u16, bytes: &[u8]) -> Result<(), SimhSessionError> {
        for (offset, byte) in bytes.iter().copied().enumerate() {
            let Some(address) = u16::try_from(offset)
                .ok()
                .and_then(|offset| base.checked_add(offset))
            else {
                break;
            };
            self.write_byte(address, byte)?;
        }
        Ok(())
    }

    pub fn set_device_debug_mode(
        &mut self,
        device: &str,
        enabled: bool,
        mode_bits: &str,
    ) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let mode_bits = CString::new(mode_bits)?;
        let set_unset = if enabled { 1 } else { 0 };
        let status = unsafe {
            self.api.device_debug_mode(
                self.raw(),
                device.as_ptr(),
                set_unset,
                mode_bits.as_ptr(),
            )
        };
        self.check("device debug mode", status)
    }

    pub fn mount(
        &mut self,
        device: &str,
        switches: &str,
        path: &Path,
    ) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let switches = CString::new(switches)?;
        let path = path_cstring(path)?;
        let status = unsafe {
            self.api.mount(
                self.raw(),
                device.as_ptr(),
                switches.as_ptr(),
                path.as_ptr(),
            )
        };
        self.check("mount", status)
    }

    pub fn dismount(&mut self, device: &str) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let status = unsafe { self.api.dismount(self.raw(), device.as_ptr()) };
        self.check("dismount", status)
    }

    pub fn halt_text(&self) -> String {
        let text = unsafe { self.api.halt_text(self.raw()) };
        c_text(text)
    }
}

impl Drop for SimhSession {
    fn drop(&mut self) {
        unsafe {
            let _ = self.api.destroy(self.raw());
        }
    }
}

fn path_cstring(path: &Path) -> Result<CString, SimhSessionError> {
    Ok(CString::new(path.to_string_lossy().as_bytes())?)
}

fn last_error_text(api: &ffi::FrontPanelApi) -> String {
    let ptr = unsafe { api.get_error() };
    let text = c_text(ptr);
    if text.is_empty() {
        "unknown FrontPanel error".to_owned()
    } else {
        text
    }
}

fn c_text(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_state_is_backend_neutral() {
        assert_ne!(SimhOperationalState::Halted, SimhOperationalState::Running);
        assert_ne!(SimhOperationalState::Running, SimhOperationalState::Error);
    }

    #[test]
    fn path_conversion_rejects_embedded_nul() {
        let path = Path::new("bad\0path");
        assert!(matches!(
            path_cstring(path),
            Err(SimhSessionError::InteriorNul(_))
        ));
    }
}

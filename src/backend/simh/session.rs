use std::ffi::{CStr, CString, NulError, c_void};
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;

use super::ffi;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimhOperationalState {
    Halted,
    Running,
    Error,
}

#[derive(Debug)]
pub enum SimhSessionError {
    InteriorNul(NulError),
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
            Self::StartFailed(detail) => write!(f, "failed to start SIMH: {detail}"),
            Self::Api { operation, detail } => write!(f, "SIMH {operation} failed: {detail}"),
        }
    }
}

impl std::error::Error for SimhSessionError {}

impl From<NulError> for SimhSessionError {
    fn from(value: NulError) -> Self { Self::InteriorNul(value) }
}

/// RAII owner for one Open SIMH FrontPanel connection.
///
/// The C API creates a simulator process and internal communication threads.
/// The handle is deliberately !Send + !Sync until the thread-safety guarantees
/// required by RusTair's UI/runtime have been audited explicitly.
pub struct SimhSession {
    panel: NonNull<ffi::PANEL>,
    _not_send_sync: PhantomData<Rc<()>>,
}

impl SimhSession {
    pub fn start(
        simulator: &Path,
        config: &Path,
        device_panel_count: usize,
    ) -> Result<Self, SimhSessionError> {
        let simulator = path_cstring(simulator)?;
        let config = path_cstring(config)?;
        let raw = unsafe {
            ffi::sim_panel_start_simulator(
                simulator.as_ptr(),
                config.as_ptr(),
                device_panel_count,
            )
        };
        let panel = NonNull::new(raw).ok_or_else(|| {
            SimhSessionError::StartFailed(last_error_text())
        })?;
        Ok(Self {
            panel,
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
                detail: last_error_text(),
            })
        }
    }

    pub fn state(&self) -> SimhOperationalState {
        match unsafe { ffi::sim_panel_get_state(self.raw()) } {
            ffi::OperationalState::Halt => SimhOperationalState::Halted,
            ffi::OperationalState::Run => SimhOperationalState::Running,
            ffi::OperationalState::Error => SimhOperationalState::Error,
        }
    }

    pub fn halt(&mut self) -> Result<(), SimhSessionError> {
        let status = unsafe { ffi::sim_panel_exec_halt(self.raw()) };
        self.check("halt", status)
    }

    pub fn run(&mut self) -> Result<(), SimhSessionError> {
        let status = unsafe { ffi::sim_panel_exec_run(self.raw()) };
        self.check("run", status)
    }

    /// Reset all SIMH devices and start instruction execution.
    pub fn start_from_reset(&mut self) -> Result<(), SimhSessionError> {
        let status = unsafe { ffi::sim_panel_exec_start(self.raw()) };
        self.check("start", status)
    }

    pub fn step(&mut self) -> Result<(), SimhSessionError> {
        let status = unsafe { ffi::sim_panel_exec_step(self.raw()) };
        self.check("step", status)
    }

    pub fn boot(&mut self, device: &str) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let status = unsafe { ffi::sim_panel_exec_boot(self.raw(), device.as_ptr()) };
        self.check("boot", status)
    }

    /// Examine a SIMH register into the API's conventional 32-bit host buffer.
    /// This is sufficient for the 8/16-bit programmer-visible Altair registers
    /// and matches the official FrontPanel sample application's usage.
    pub fn examine_register_u32(&self, name: &str) -> Result<u32, SimhSessionError> {
        let name = CString::new(name)?;
        let mut value = 0u32;
        let status = unsafe {
            ffi::sim_panel_gen_examine(
                self.raw(),
                name.as_ptr(),
                size_of::<u32>(),
                (&mut value as *mut u32).cast::<c_void>(),
            )
        };
        self.check("register examine", status)?;
        Ok(value)
    }

    pub fn deposit_register_u32(
        &mut self,
        name: &str,
        value: u32,
    ) -> Result<(), SimhSessionError> {
        let name = CString::new(name)?;
        let status = unsafe {
            ffi::sim_panel_gen_deposit(
                self.raw(),
                name.as_ptr(),
                size_of::<u32>(),
                (&value as *const u32).cast::<c_void>(),
            )
        };
        self.check("register deposit", status)
    }

    pub fn read_byte(&self, address: u16) -> Result<u8, SimhSessionError> {
        let address = u32::from(address);
        let mut value = 0u8;
        let status = unsafe {
            ffi::sim_panel_mem_examine(
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
            ffi::sim_panel_mem_deposit(
                self.raw(),
                size_of::<u32>(),
                (&address as *const u32).cast::<c_void>(),
                size_of::<u8>(),
                (&value as *const u8).cast::<c_void>(),
            )
        };
        self.check("memory deposit", status)
    }

    /// Deliberately byte-wise for the first implementation. The FrontPanel API
    /// describes `value_size` as the host representation of one addressed
    /// value, not a bulk-transfer length, so this avoids relying on undocumented
    /// contiguous-block semantics.
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
            ffi::sim_panel_mount(
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
        let status = unsafe { ffi::sim_panel_dismount(self.raw(), device.as_ptr()) };
        self.check("dismount", status)
    }

    pub fn halt_text(&self) -> String {
        let text = unsafe { ffi::sim_panel_halt_text(self.raw()) };
        c_text(text)
    }
}

impl Drop for SimhSession {
    fn drop(&mut self) {
        unsafe {
            let _ = ffi::sim_panel_destroy(self.raw());
        }
    }
}

fn path_cstring(path: &Path) -> Result<CString, SimhSessionError> {
    Ok(CString::new(path.to_string_lossy().as_bytes())?)
}

fn last_error_text() -> String {
    let ptr = unsafe { ffi::sim_panel_get_error() };
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

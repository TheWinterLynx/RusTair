use std::ffi::{CStr, CString, NulError, c_int, c_void};
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::{ffi, runtime};

// 30 Hz is sufficient for a human front panel and keeps the FrontPanel control
// channel comfortably below the rate at which it can compete with user commands.
const LIVE_CALLBACK_INTERVAL_US: c_int = 33_333;
const CONSOLE_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimhOperationalState {
    Halted,
    Running,
    Error,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SimhLivePanelSample {
    pub pc: u16,
    pub a: u8,
    pub sp: u16,
    pub address_activity: [f32; 16],
    pub data_activity: [f32; 8],
    pub simulation_time: u64,
}

struct LivePanelRegisters {
    pc: Box<u32>,
    data: Box<u32>,
    sp: Box<u32>,
    data_shift: usize,
}

impl LivePanelRegisters {
    fn new(data_shift: usize) -> Self {
        Self {
            pc: Box::new(0),
            data: Box::new(0),
            sp: Box::new(0),
            data_shift,
        }
    }
}

/// Context owned by `SimhSession` and passed verbatim to Open-SIMH's callback
/// thread. All pointed-to buffers are stable heap allocations.
struct LivePanelCallbackContext {
    pc: *const u32,
    data: *const u32,
    sp: *const u32,
    data_shift: usize,
    latest: Arc<Mutex<SimhLivePanelSample>>,
}

unsafe impl Send for LivePanelCallbackContext {}
unsafe impl Sync for LivePanelCallbackContext {}

unsafe extern "C" fn live_panel_callback(
    _panel: *mut ffi::PANEL,
    simulation_time: u64,
    context: *mut c_void,
) {
    if context.is_null() {
        return;
    }
    let context = unsafe { &*(context.cast::<LivePanelCallbackContext>()) };
    let pc = unsafe { *context.pc } as u16;
    let data_raw = unsafe { *context.data };
    let a = (data_raw >> context.data_shift) as u8;
    let sp = unsafe { *context.sp } as u16;

    let mut address_activity = [0.0f32; 16];
    for (bit, dst) in address_activity.iter_mut().enumerate() {
        *dst = if pc & (1u16 << bit) != 0 { 1.0 } else { 0.0 };
    }
    let mut data_activity = [0.0f32; 8];
    for (bit, dst) in data_activity.iter_mut().enumerate() {
        *dst = if a & (1u8 << bit) != 0 { 1.0 } else { 0.0 };
    }

    let sample = SimhLivePanelSample {
        pc,
        a,
        sp,
        address_activity,
        data_activity,
        simulation_time,
    };
    let mut latest = context.latest.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *latest = sample;
}

#[derive(Debug)]
pub enum SimhSessionError {
    InteriorNul(NulError),
    Runtime(runtime::SimhRuntimeError),
    FrontPanelLoad(ffi::FrontPanelLoadError),
    StartFailed(String),
    Api { operation: &'static str, detail: String },
    ConsoleUnavailable,
}

impl fmt::Display for SimhSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InteriorNul(error) => write!(f, "SIMH string contains an interior NUL: {error}"),
            Self::Runtime(error) => write!(f, "SIMH runtime preparation failed: {error}"),
            Self::FrontPanelLoad(error) => write!(f, "SIMH FrontPanel load failed: {error}"),
            Self::StartFailed(detail) => write!(f, "failed to start SIMH: {detail}"),
            Self::Api { operation, detail } => write!(f, "SIMH {operation} failed: {detail}"),
            Self::ConsoleUnavailable => write!(
                f,
                "the embedded simh_frontpanel.dll predates RusTair console support; rebuild the SIMH bundle"
            ),
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
impl From<NulError> for SimhSessionError { fn from(value: NulError) -> Self { Self::InteriorNul(value) } }
impl From<runtime::SimhRuntimeError> for SimhSessionError { fn from(value: runtime::SimhRuntimeError) -> Self { Self::Runtime(value) } }
impl From<ffi::FrontPanelLoadError> for SimhSessionError { fn from(value: ffi::FrontPanelLoadError) -> Self { Self::FrontPanelLoad(value) } }

/// RAII owner for one Open-SIMH FrontPanel connection.
/// Product code creates and owns this object only on the dedicated SIMH worker.
pub struct SimhSession {
    panel: NonNull<ffi::PANEL>,
    api: ffi::FrontPanelApi,
    live: LivePanelRegisters,
    latest_live: Arc<Mutex<SimhLivePanelSample>>,
    callback_context: Option<Box<LivePanelCallbackContext>>,
    _not_send_sync: PhantomData<Rc<()>>,
}

impl SimhSession {
    pub fn start(simulator: &Path, config: &Path, device_panel_count: usize) -> Result<Self, SimhSessionError> {
        Self::start_impl(simulator, config, device_panel_count, None)
    }

    pub fn start_debug(simulator: &Path, config: &Path, device_panel_count: usize, debug_file: &Path) -> Result<Self, SimhSessionError> {
        Self::start_impl(simulator, config, device_panel_count, Some(debug_file))
    }

    fn start_impl(
        simulator: &Path,
        config: &Path,
        device_panel_count: usize,
        debug_file: Option<&Path>,
    ) -> Result<Self, SimhSessionError> {
        let is_altairz80 = simulator
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case("altairz80"));
        let data_register = if is_altairz80 { "AF" } else { "A" };
        let data_shift = if is_altairz80 { 8 } else { 0 };

        let frontpanel_path = runtime::frontpanel_dll_path()?;
        let api = ffi::FrontPanelApi::load(&frontpanel_path)?;
        let simulator_c = path_cstring(simulator)?;
        let config_c = path_cstring(config)?;
        let debug = debug_file.map(path_cstring).transpose()?;
        let raw = unsafe {
            match debug.as_ref() {
                Some(debug) => api.start_simulator_debug(simulator_c.as_ptr(), config_c.as_ptr(), device_panel_count, debug.as_ptr()),
                None => api.start_simulator(simulator_c.as_ptr(), config_c.as_ptr(), device_panel_count),
            }
        };
        let panel = NonNull::new(raw).ok_or_else(|| SimhSessionError::StartFailed(last_error_text(&api)))?;
        let latest_live = Arc::new(Mutex::new(SimhLivePanelSample::default()));
        let mut session = Self {
            panel,
            api,
            live: LivePanelRegisters::new(data_shift),
            latest_live,
            callback_context: None,
            _not_send_sync: PhantomData,
        };
        session.configure_live_panel_callback(data_register)?;
        Ok(session)
    }

    #[inline]
    fn raw(&self) -> *mut ffi::PANEL { self.panel.as_ptr() }

    fn check(&self, operation: &'static str, status: i32) -> Result<(), SimhSessionError> {
        if status == 0 { Ok(()) } else { Err(SimhSessionError::Api { operation, detail: last_error_text(&self.api) }) }
    }

    fn configure_live_panel_callback(&mut self, data_register: &str) -> Result<(), SimhSessionError> {
        let pc = CString::new("PC")?;
        let data = CString::new(data_register)?;
        let sp = CString::new("SP")?;
        let device = std::ptr::null();
        let raw = self.raw();

        // Only three ordinary register subscriptions are required. Unlike the
        // *_bits sampling mode, this does not establish a separate SIMH sample
        // collector or block startup on rolling-average setup.
        let status = unsafe { self.api.add_register(raw, pc.as_ptr(), device, size_of::<u32>(), (&mut *self.live.pc as *mut u32).cast()) };
        self.check("register live PC", status)?;
        let status = unsafe { self.api.add_register(raw, data.as_ptr(), device, size_of::<u32>(), (&mut *self.live.data as *mut u32).cast()) };
        self.check("register live data", status)?;
        let status = unsafe { self.api.add_register(raw, sp.as_ptr(), device, size_of::<u32>(), (&mut *self.live.sp as *mut u32).cast()) };
        self.check("register live SP", status)?;

        let mut context = Box::new(LivePanelCallbackContext {
            pc: (&*self.live.pc) as *const u32,
            data: (&*self.live.data) as *const u32,
            sp: (&*self.live.sp) as *const u32,
            data_shift: self.live.data_shift,
            latest: Arc::clone(&self.latest_live),
        });
        let context_ptr = (&mut *context as *mut LivePanelCallbackContext).cast::<c_void>();
        let status = unsafe {
            self.api.set_display_callback_interval(
                raw,
                Some(live_panel_callback),
                context_ptr,
                LIVE_CALLBACK_INTERVAL_US,
            )
        };
        self.check("start live front-panel callback", status)?;
        self.callback_context = Some(context);
        Ok(())
    }

    /// Local copy only. No command is sent to SIMH.
    pub fn live_panel_sample(&self) -> SimhLivePanelSample {
        *self.latest_live.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn state(&self) -> SimhOperationalState {
        match unsafe { self.api.get_state(self.raw()) } {
            ffi::OperationalState::Halt => SimhOperationalState::Halted,
            ffi::OperationalState::Run => SimhOperationalState::Running,
            ffi::OperationalState::Error => SimhOperationalState::Error,
        }
    }

    pub fn halt(&mut self) -> Result<(), SimhSessionError> { let status = unsafe { self.api.exec_halt(self.raw()) }; self.check("halt", status) }
    pub fn run(&mut self) -> Result<(), SimhSessionError> { let status = unsafe { self.api.exec_run(self.raw()) }; self.check("run", status) }
    pub fn start_from_reset(&mut self) -> Result<(), SimhSessionError> { let status = unsafe { self.api.exec_start(self.raw()) }; self.check("start", status) }
    pub fn step(&mut self) -> Result<(), SimhSessionError> { let status = unsafe { self.api.exec_step(self.raw()) }; self.check("step", status) }
    pub fn boot(&mut self, device: &str) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let status = unsafe { self.api.exec_boot(self.raw(), device.as_ptr()) };
        self.check("boot", status)
    }

    pub fn examine_u32(&self, name_or_addr: &str) -> Result<u32, SimhSessionError> {
        let name_or_addr = CString::new(name_or_addr)?;
        let mut value = 0u32;
        let status = unsafe { self.api.gen_examine(self.raw(), name_or_addr.as_ptr(), size_of::<u32>(), (&mut value as *mut u32).cast::<c_void>()) };
        self.check("generic examine", status)?;
        Ok(value)
    }
    pub fn deposit_u32(&mut self, name_or_addr: &str, value: u32) -> Result<(), SimhSessionError> {
        let name_or_addr = CString::new(name_or_addr)?;
        let status = unsafe { self.api.gen_deposit(self.raw(), name_or_addr.as_ptr(), size_of::<u32>(), (&value as *const u32).cast::<c_void>()) };
        self.check("generic deposit", status)
    }
    pub fn examine_register_u32(&self, name: &str) -> Result<u32, SimhSessionError> { self.examine_u32(name) }
    pub fn deposit_register_u32(&mut self, name: &str, value: u32) -> Result<(), SimhSessionError> { self.deposit_u32(name, value) }

    pub fn read_byte(&self, address: u16) -> Result<u8, SimhSessionError> {
        let address = u32::from(address);
        let mut value = 0u8;
        let status = unsafe {
            self.api.mem_examine(self.raw(), size_of::<u32>(), (&address as *const u32).cast::<c_void>(), size_of::<u8>(), (&mut value as *mut u8).cast::<c_void>())
        };
        self.check("memory examine", status)?;
        Ok(value)
    }
    pub fn write_byte(&mut self, address: u16, value: u8) -> Result<(), SimhSessionError> {
        let address = u32::from(address);
        let status = unsafe {
            self.api.mem_deposit(self.raw(), size_of::<u32>(), (&address as *const u32).cast::<c_void>(), size_of::<u8>(), (&value as *const u8).cast::<c_void>())
        };
        self.check("memory deposit", status)
    }
    pub fn load_bytes(&mut self, base: u16, bytes: &[u8]) -> Result<(), SimhSessionError> {
        for (offset, byte) in bytes.iter().copied().enumerate() {
            let Some(address) = u16::try_from(offset).ok().and_then(|offset| base.checked_add(offset)) else { break; };
            self.write_byte(address, byte)?;
        }
        Ok(())
    }

    pub fn set_device_debug_mode(&mut self, device: &str, enabled: bool, mode_bits: &str) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let mode_bits = CString::new(mode_bits)?;
        let set_unset = if enabled { 1 } else { 0 };
        let status = unsafe { self.api.device_debug_mode(self.raw(), device.as_ptr(), set_unset, mode_bits.as_ptr()) };
        self.check("device debug mode", status)
    }
    pub fn mount(&mut self, device: &str, switches: &str, path: &Path) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let switches = CString::new(switches)?;
        let path = path_cstring(path)?;
        let status = unsafe { self.api.mount(self.raw(), device.as_ptr(), switches.as_ptr(), path.as_ptr()) };
        self.check("mount", status)
    }
    pub fn dismount(&mut self, device: &str) -> Result<(), SimhSessionError> {
        let device = CString::new(device)?;
        let status = unsafe { self.api.dismount(self.raw(), device.as_ptr()) };
        self.check("dismount", status)
    }

    pub fn console_available(&self) -> bool { self.api.has_rustair_exec_command() }

    pub fn console_command(&mut self, command: &str) -> Result<String, SimhSessionError> {
        if !self.api.has_rustair_exec_command() {
            return Err(SimhSessionError::ConsoleUnavailable);
        }
        let command = CString::new(command)?;
        let mut response = vec![0 as std::ffi::c_char; CONSOLE_RESPONSE_BYTES];
        let Some(status) = (unsafe {
            self.api.rustair_exec_command(
                self.raw(),
                command.as_ptr(),
                response.as_mut_ptr(),
                response.len(),
            )
        }) else {
            return Err(SimhSessionError::ConsoleUnavailable);
        };
        self.check("console command", status)?;
        Ok(unsafe { CStr::from_ptr(response.as_ptr()) }.to_string_lossy().into_owned())
    }

    pub fn halt_text(&self) -> String { let text = unsafe { self.api.halt_text(self.raw()) }; c_text(text) }
}

impl Drop for SimhSession {
    fn drop(&mut self) {
        unsafe { let _ = self.api.destroy(self.raw()); }
    }
}

fn path_cstring(path: &Path) -> Result<CString, SimhSessionError> { Ok(CString::new(path.to_string_lossy().as_bytes())?) }
fn last_error_text(api: &ffi::FrontPanelApi) -> String {
    let ptr = unsafe { api.get_error() };
    let text = c_text(ptr);
    if text.is_empty() { "unknown FrontPanel error".to_owned() } else { text }
}
fn c_text(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() { return String::new(); }
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
        assert!(matches!(path_cstring(path), Err(SimhSessionError::InteriorNul(_))));
    }
}

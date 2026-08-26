//! Dynamically loaded Open-SIMH FrontPanel ABI.
//!
//! The function signatures mirror API version 12 from the pinned Open-SIMH
//! revision bundled with RusTair. Nothing in this module is linked against
//! `simh_frontpanel.lib`: the embedded DLL is extracted and resolved with the
//! Windows loader at runtime.

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_void};
use std::fmt;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

pub const SIM_FRONTPANEL_VERSION: c_int = 12;

#[repr(C)]
pub struct PANEL {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationalState {
    Halt = 0,
    Run = 1,
    Error = 2,
}

#[derive(Debug)]
pub enum FrontPanelLoadError {
    UnsupportedPlatform,
    LoadLibrary { path: PathBuf, code: u32 },
    MissingSymbol { symbol: &'static str, code: u32 },
}

impl fmt::Display for FrontPanelLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => write!(f, "the embedded SIMH FrontPanel backend is currently supported only on Windows"),
            Self::LoadLibrary { path, code } => write!(
                f,
                "unable to load {} (Windows error {code})",
                path.display()
            ),
            Self::MissingSymbol { symbol, code } => write!(
                f,
                "SIMH FrontPanel DLL does not export {symbol} (Windows error {code})"
            ),
        }
    }
}

impl std::error::Error for FrontPanelLoadError {}

type StartSimulatorFn = unsafe extern "C" fn(*const c_char, *const c_char, usize) -> *mut PANEL;
type StartSimulatorDebugFn =
    unsafe extern "C" fn(*const c_char, *const c_char, usize, *const c_char) -> *mut PANEL;
type DestroyFn = unsafe extern "C" fn(*mut PANEL) -> c_int;
type ExecHaltFn = unsafe extern "C" fn(*mut PANEL) -> c_int;
type ExecBootFn = unsafe extern "C" fn(*mut PANEL, *const c_char) -> c_int;
type ExecStartFn = unsafe extern "C" fn(*mut PANEL) -> c_int;
type ExecRunFn = unsafe extern "C" fn(*mut PANEL) -> c_int;
type ExecStepFn = unsafe extern "C" fn(*mut PANEL) -> c_int;
type HaltTextFn = unsafe extern "C" fn(*mut PANEL) -> *const c_char;
type AddRegisterFn = unsafe extern "C" fn(
    *mut PANEL,
    *const c_char,
    *const c_char,
    usize,
    *mut c_void,
) -> c_int;
type AddRegisterBitsFn = unsafe extern "C" fn(
    *mut PANEL,
    *const c_char,
    *const c_char,
    usize,
    *mut c_int,
) -> c_int;
type GetRegistersFn = unsafe extern "C" fn(*mut PANEL, *mut u64) -> c_int;
type SetSamplingParametersFn = unsafe extern "C" fn(*mut PANEL, u32, u32) -> c_int;
type GenExamineFn = unsafe extern "C" fn(*mut PANEL, *const c_char, usize, *mut c_void) -> c_int;
type GenDepositFn = unsafe extern "C" fn(*mut PANEL, *const c_char, usize, *const c_void) -> c_int;
type MemExamineFn = unsafe extern "C" fn(
    *mut PANEL,
    usize,
    *const c_void,
    usize,
    *mut c_void,
) -> c_int;
type MemDepositFn = unsafe extern "C" fn(
    *mut PANEL,
    usize,
    *const c_void,
    usize,
    *const c_void,
) -> c_int;
type DeviceDebugModeFn =
    unsafe extern "C" fn(*mut PANEL, *const c_char, c_int, *const c_char) -> c_int;
type MountFn = unsafe extern "C" fn(
    *mut PANEL,
    *const c_char,
    *const c_char,
    *const c_char,
) -> c_int;
type DismountFn = unsafe extern "C" fn(*mut PANEL, *const c_char) -> c_int;
type GetStateFn = unsafe extern "C" fn(*mut PANEL) -> OperationalState;
type GetErrorFn = unsafe extern "C" fn() -> *const c_char;

pub struct FrontPanelApi {
    #[cfg(windows)]
    module: *mut c_void,
    start_simulator: StartSimulatorFn,
    start_simulator_debug: StartSimulatorDebugFn,
    destroy: DestroyFn,
    exec_halt: ExecHaltFn,
    exec_boot: ExecBootFn,
    exec_start: ExecStartFn,
    exec_run: ExecRunFn,
    exec_step: ExecStepFn,
    halt_text: HaltTextFn,
    add_register: AddRegisterFn,
    add_register_bits: AddRegisterBitsFn,
    get_registers: GetRegistersFn,
    set_sampling_parameters: SetSamplingParametersFn,
    gen_examine: GenExamineFn,
    gen_deposit: GenDepositFn,
    mem_examine: MemExamineFn,
    mem_deposit: MemDepositFn,
    device_debug_mode: DeviceDebugModeFn,
    mount: MountFn,
    dismount: DismountFn,
    get_state: GetStateFn,
    get_error: GetErrorFn,
}

impl FrontPanelApi {
    #[cfg(windows)]
    pub fn load(path: &Path) -> Result<Self, FrontPanelLoadError> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(FrontPanelLoadError::LoadLibrary {
                path: path.to_path_buf(),
                code: unsafe { GetLastError() },
            });
        }

        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let ptr = unsafe {
                    GetProcAddress(module, concat!($name, "\0").as_ptr())
                };
                if ptr.is_null() {
                    let code = unsafe { GetLastError() };
                    unsafe { FreeLibrary(module); }
                    return Err(FrontPanelLoadError::MissingSymbol { symbol: $name, code });
                }
                unsafe { std::mem::transmute::<*mut c_void, $ty>(ptr) }
            }};
        }

        Ok(Self {
            module,
            start_simulator: symbol!("sim_panel_start_simulator", StartSimulatorFn),
            start_simulator_debug: symbol!("sim_panel_start_simulator_debug", StartSimulatorDebugFn),
            destroy: symbol!("sim_panel_destroy", DestroyFn),
            exec_halt: symbol!("sim_panel_exec_halt", ExecHaltFn),
            exec_boot: symbol!("sim_panel_exec_boot", ExecBootFn),
            exec_start: symbol!("sim_panel_exec_start", ExecStartFn),
            exec_run: symbol!("sim_panel_exec_run", ExecRunFn),
            exec_step: symbol!("sim_panel_exec_step", ExecStepFn),
            halt_text: symbol!("sim_panel_halt_text", HaltTextFn),
            add_register: symbol!("sim_panel_add_register", AddRegisterFn),
            add_register_bits: symbol!("sim_panel_add_register_bits", AddRegisterBitsFn),
            get_registers: symbol!("sim_panel_get_registers", GetRegistersFn),
            set_sampling_parameters: symbol!("sim_panel_set_sampling_parameters", SetSamplingParametersFn),
            gen_examine: symbol!("sim_panel_gen_examine", GenExamineFn),
            gen_deposit: symbol!("sim_panel_gen_deposit", GenDepositFn),
            mem_examine: symbol!("sim_panel_mem_examine", MemExamineFn),
            mem_deposit: symbol!("sim_panel_mem_deposit", MemDepositFn),
            device_debug_mode: symbol!("sim_panel_device_debug_mode", DeviceDebugModeFn),
            mount: symbol!("sim_panel_mount", MountFn),
            dismount: symbol!("sim_panel_dismount", DismountFn),
            get_state: symbol!("sim_panel_get_state", GetStateFn),
            get_error: symbol!("sim_panel_get_error", GetErrorFn),
        })
    }

    #[cfg(not(windows))]
    pub fn load(_path: &Path) -> Result<Self, FrontPanelLoadError> {
        Err(FrontPanelLoadError::UnsupportedPlatform)
    }

    pub unsafe fn start_simulator(&self, sim_path: *const c_char, sim_config: *const c_char, device_panel_count: usize) -> *mut PANEL {
        unsafe { (self.start_simulator)(sim_path, sim_config, device_panel_count) }
    }
    pub unsafe fn start_simulator_debug(&self, sim_path: *const c_char, sim_config: *const c_char, device_panel_count: usize, debug_file: *const c_char) -> *mut PANEL {
        unsafe { (self.start_simulator_debug)(sim_path, sim_config, device_panel_count, debug_file) }
    }
    pub unsafe fn destroy(&self, panel: *mut PANEL) -> c_int { unsafe { (self.destroy)(panel) } }
    pub unsafe fn exec_halt(&self, panel: *mut PANEL) -> c_int { unsafe { (self.exec_halt)(panel) } }
    pub unsafe fn exec_boot(&self, panel: *mut PANEL, device: *const c_char) -> c_int { unsafe { (self.exec_boot)(panel, device) } }
    pub unsafe fn exec_start(&self, panel: *mut PANEL) -> c_int { unsafe { (self.exec_start)(panel) } }
    pub unsafe fn exec_run(&self, panel: *mut PANEL) -> c_int { unsafe { (self.exec_run)(panel) } }
    pub unsafe fn exec_step(&self, panel: *mut PANEL) -> c_int { unsafe { (self.exec_step)(panel) } }
    pub unsafe fn halt_text(&self, panel: *mut PANEL) -> *const c_char { unsafe { (self.halt_text)(panel) } }

    pub unsafe fn add_register(&self, panel: *mut PANEL, name: *const c_char, device: *const c_char, size: usize, addr: *mut c_void) -> c_int {
        unsafe { (self.add_register)(panel, name, device, size, addr) }
    }
    pub unsafe fn add_register_bits(&self, panel: *mut PANEL, name: *const c_char, device: *const c_char, bit_width: usize, bits: *mut c_int) -> c_int {
        unsafe { (self.add_register_bits)(panel, name, device, bit_width, bits) }
    }
    pub unsafe fn get_registers(&self, panel: *mut PANEL, simulation_time: *mut u64) -> c_int {
        unsafe { (self.get_registers)(panel, simulation_time) }
    }
    pub unsafe fn set_sampling_parameters(&self, panel: *mut PANEL, sample_frequency: u32, sample_depth: u32) -> c_int {
        unsafe { (self.set_sampling_parameters)(panel, sample_frequency, sample_depth) }
    }

    pub unsafe fn gen_examine(&self, panel: *mut PANEL, name_or_addr: *const c_char, size: usize, value: *mut c_void) -> c_int {
        unsafe { (self.gen_examine)(panel, name_or_addr, size, value) }
    }
    pub unsafe fn gen_deposit(&self, panel: *mut PANEL, name_or_addr: *const c_char, size: usize, value: *const c_void) -> c_int {
        unsafe { (self.gen_deposit)(panel, name_or_addr, size, value) }
    }
    pub unsafe fn mem_examine(&self, panel: *mut PANEL, addr_size: usize, addr: *const c_void, value_size: usize, value: *mut c_void) -> c_int {
        unsafe { (self.mem_examine)(panel, addr_size, addr, value_size, value) }
    }
    pub unsafe fn mem_deposit(&self, panel: *mut PANEL, addr_size: usize, addr: *const c_void, value_size: usize, value: *const c_void) -> c_int {
        unsafe { (self.mem_deposit)(panel, addr_size, addr, value_size, value) }
    }
    pub unsafe fn device_debug_mode(&self, panel: *mut PANEL, device: *const c_char, set_unset: c_int, mode_bits: *const c_char) -> c_int {
        unsafe { (self.device_debug_mode)(panel, device, set_unset, mode_bits) }
    }
    pub unsafe fn mount(&self, panel: *mut PANEL, device: *const c_char, switches: *const c_char, path: *const c_char) -> c_int {
        unsafe { (self.mount)(panel, device, switches, path) }
    }
    pub unsafe fn dismount(&self, panel: *mut PANEL, device: *const c_char) -> c_int { unsafe { (self.dismount)(panel, device) } }
    pub unsafe fn get_state(&self, panel: *mut PANEL) -> OperationalState { unsafe { (self.get_state)(panel) } }
    pub unsafe fn get_error(&self) -> *const c_char { unsafe { (self.get_error)() } }
}

#[cfg(windows)]
impl Drop for FrontPanelApi {
    fn drop(&mut self) {
        if !self.module.is_null() {
            unsafe { FreeLibrary(self.module); }
            self.module = std::ptr::null_mut();
        }
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(lp_lib_file_name: *const u16) -> *mut c_void;
    fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const u8) -> *mut c_void;
    fn FreeLibrary(h_lib_module: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

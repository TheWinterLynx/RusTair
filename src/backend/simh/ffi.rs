//! Raw Open SIMH FrontPanel ABI.
//!
//! Signatures are mirrored from `sim_frontpanel.h` API version 12 in the
//! Open-SIMH tree used while this backend was introduced. Keep this module
//! private: safe Rust code must go through `session.rs`.

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_uint, c_void};

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

unsafe extern "C" {
    pub fn sim_panel_start_simulator(
        sim_path: *const c_char,
        sim_config: *const c_char,
        device_panel_count: usize,
    ) -> *mut PANEL;

    pub fn sim_panel_start_simulator_debug(
        sim_path: *const c_char,
        sim_config: *const c_char,
        device_panel_count: usize,
        debug_file: *const c_char,
    ) -> *mut PANEL;

    pub fn sim_panel_add_device_panel(
        simulator_panel: *mut PANEL,
        device_name: *const c_char,
    ) -> *mut PANEL;

    pub fn sim_panel_destroy(panel: *mut PANEL) -> c_int;

    pub fn sim_panel_add_register(
        panel: *mut PANEL,
        name: *const c_char,
        device_name: *const c_char,
        size: usize,
        addr: *mut c_void,
    ) -> c_int;

    pub fn sim_panel_add_register_bits(
        panel: *mut PANEL,
        name: *const c_char,
        device_name: *const c_char,
        bit_width: usize,
        bits: *mut c_int,
    ) -> c_int;

    pub fn sim_panel_get_registers(
        panel: *mut PANEL,
        simulation_time: *mut u64,
    ) -> c_int;

    pub fn sim_panel_set_sampling_parameters_ex(
        panel: *mut PANEL,
        sample_frequency: c_uint,
        sample_dither_pct: c_uint,
        sample_depth: c_uint,
    ) -> c_int;

    pub fn sim_panel_set_sampling_parameters(
        panel: *mut PANEL,
        sample_frequency: c_uint,
        sample_depth: c_uint,
    ) -> c_int;

    pub fn sim_panel_exec_halt(panel: *mut PANEL) -> c_int;
    pub fn sim_panel_exec_boot(panel: *mut PANEL, device: *const c_char) -> c_int;
    pub fn sim_panel_exec_start(panel: *mut PANEL) -> c_int;
    pub fn sim_panel_exec_run(panel: *mut PANEL) -> c_int;
    pub fn sim_panel_exec_step(panel: *mut PANEL) -> c_int;

    pub fn sim_panel_halt_text(panel: *mut PANEL) -> *const c_char;

    pub fn sim_panel_gen_examine(
        panel: *mut PANEL,
        name_or_addr: *const c_char,
        size: usize,
        value: *mut c_void,
    ) -> c_int;

    pub fn sim_panel_gen_deposit(
        panel: *mut PANEL,
        name_or_addr: *const c_char,
        size: usize,
        value: *const c_void,
    ) -> c_int;

    pub fn sim_panel_mem_examine(
        panel: *mut PANEL,
        addr_size: usize,
        addr: *const c_void,
        value_size: usize,
        value: *mut c_void,
    ) -> c_int;

    pub fn sim_panel_mem_deposit(
        panel: *mut PANEL,
        addr_size: usize,
        addr: *const c_void,
        value_size: usize,
        value: *const c_void,
    ) -> c_int;

    pub fn sim_panel_mem_deposit_instruction(
        panel: *mut PANEL,
        addr_size: usize,
        addr: *const c_void,
        instruction: *const c_char,
    ) -> c_int;

    pub fn sim_panel_set_register_value(
        panel: *mut PANEL,
        name: *const c_char,
        value: *const c_char,
    ) -> c_int;

    pub fn sim_panel_mount(
        panel: *mut PANEL,
        device: *const c_char,
        switches: *const c_char,
        path: *const c_char,
    ) -> c_int;

    pub fn sim_panel_dismount(panel: *mut PANEL, device: *const c_char) -> c_int;

    pub fn sim_panel_get_state(panel: *mut PANEL) -> OperationalState;

    pub fn sim_panel_get_error() -> *const c_char;
    pub fn sim_panel_clear_error();
}

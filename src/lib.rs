pub mod app;
pub mod audio;
pub mod backend;
pub mod callstack8080;
pub mod config;
pub mod cpu8080;
pub mod cpu8080_cycle;
pub mod debugger8080;
pub mod debugger_control;
pub mod decoder8080;
pub mod explain8080;
pub mod io;
pub mod machine;
pub mod memory_activity8080;
pub mod peripherals;
pub mod s100;
pub mod s100_backplane;
pub mod s100_chassis;
pub mod s100_cpu;
pub mod s100_interface;
pub mod s100_io;
pub mod s100_io_card;
pub mod s100_memory;
pub mod s100_runtime;
pub mod s100_runtime_ram;
pub mod trace8080;
pub(crate) mod embedded_assets;
pub(crate) mod mc6850;

#[cfg(test)]
mod s100_cycle_integration_tests;

// Keep the original public module path working while the implementation now
// lives under peripherals/asr33.
pub use peripherals::asr33 as teletype;

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
pub mod peripherals;
pub mod trace8080;
pub(crate) mod embedded_assets;

// Keep the original public module path working while the implementation now
// lives under peripherals/asr33.
pub use peripherals::asr33 as teletype;

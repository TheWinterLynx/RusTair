pub mod app;
pub mod audio;
pub mod config;
pub mod cpu8080;
pub mod io;
pub mod machine;
pub mod peripherals;

// Keep the original public module path working while the implementation now
// lives under peripherals/asr33.
pub use peripherals::asr33 as teletype;

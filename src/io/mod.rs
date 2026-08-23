//! Host-side I/O adapters.
//!
//! Physical and virtual serial transports live here, outside the Altair
//! machine model. The router owns virtual cabling while TCP and COM adapters
//! move host bytes without changing the emulated MITS interfaces.

pub(crate) mod com_serial;
pub(crate) mod serial_router;
pub(crate) mod tcp_serial;

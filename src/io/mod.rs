//! Host-side I/O adapters.
//!
//! Physical/virtual serial integration belongs here rather than in the Altair
//! machine or a UI endpoint. Endpoint selection already lives here; the future
//! HostSerial transport can plug into the same boundary.

pub(crate) mod serial_router;

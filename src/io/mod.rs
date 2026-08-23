//! Host-side I/O adapters.
//!
//! Physical/virtual serial integration belongs here rather than in the Altair
//! machine or a UI endpoint. Endpoint selection and host transports share this
//! boundary so future COM/PTY adapters can reuse the same routing model.

pub(crate) mod serial_router;
pub(crate) mod tcp_serial;

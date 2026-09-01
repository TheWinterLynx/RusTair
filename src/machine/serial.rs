// Board-specific serial hardware lives below this module so the S-100 I/O
// wrapper remains the only route from the machine to a UART implementation.
#[path = "sio_interface.rs"]
pub(super) mod sio_interface;
#[path = "sio.rs"]
pub(super) mod sio;

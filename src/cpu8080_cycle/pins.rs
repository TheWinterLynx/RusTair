#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cpu8080Inputs {
    /// Value presented to the 8080 data bus while DBIN is active.
    pub data_in: u8,
    /// READY sampled by the processor. Low inserts TW states.
    pub ready: bool,
    /// Maskable interrupt request input.
    pub interrupt: bool,
    /// DMA/bus-hold request input.
    pub hold: bool,
    /// Asynchronous RESET input.
    pub reset: bool,
}

impl Default for Cpu8080Inputs {
    fn default() -> Self {
        Self {
            data_in: 0,
            ready: true,
            interrupt: false,
            hold: false,
            reset: false,
        }
    }
}

/// Externally visible Intel 8080 signals at T-state granularity.
///
/// `address` and `data_out` use `Option` so HOLD/tri-state behaviour can be
/// represented explicitly once those machine states are implemented.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cpu8080Pins {
    pub address: Option<u16>,
    pub data_out: Option<u8>,
    pub sync: bool,
    pub dbin: bool,
    /// Physical /WR output. `false` means write asserted.
    pub wr_n: bool,
    pub inte: bool,
    pub wait: bool,
    pub hlda: bool,
}

impl Default for Cpu8080Pins {
    fn default() -> Self {
        Self {
            address: None,
            data_out: None,
            sync: false,
            dbin: false,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        }
    }
}

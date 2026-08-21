#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialEndpoint {
    InternalAsr33,
    TextTerminal,
}

/// Selects which host endpoint owns the single physical connection of a
/// MITS 88-SIO. A fully populated 88-2SIO does not use this as a multiplexer:
/// RusTair wires its Port 0 to the ASR-33 and Port 1 to the Text Terminal so
/// both serial links can operate simultaneously.
pub(crate) struct SerialRouter {
    endpoint: SerialEndpoint,
}

impl Default for SerialRouter {
    fn default() -> Self {
        Self {
            endpoint: SerialEndpoint::InternalAsr33,
        }
    }
}

impl SerialRouter {
    pub(crate) fn endpoint(&self) -> SerialEndpoint {
        self.endpoint
    }

    pub(crate) fn select(&mut self, endpoint: SerialEndpoint) {
        self.endpoint = endpoint;
    }
}

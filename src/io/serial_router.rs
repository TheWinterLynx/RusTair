#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialEndpoint {
    InternalAsr33,
    TextTerminal,
}

/// Owns the selection of the host-side endpoint attached to the Altair serial
/// interface. Endpoint windows may control this selection, but the runtime does
/// not infer serial ownership from UI visibility.
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

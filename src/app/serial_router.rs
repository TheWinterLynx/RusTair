#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SerialEndpoint {
    InternalAsr33,
    TextTerminal,
}

/// Owns the selection of the host-side endpoint attached to the Altair serial
/// interface. Endpoint windows may control this selection, but the runtime no
/// longer infers serial ownership from UI visibility.
pub(super) struct SerialRouter {
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
    pub(super) fn endpoint(&self) -> SerialEndpoint {
        self.endpoint
    }

    pub(super) fn select(&mut self, endpoint: SerialEndpoint) {
        self.endpoint = endpoint;
    }
}

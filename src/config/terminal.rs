/// How an interactive terminal displays characters typed by its operator.
///
/// In full duplex the keyboard transmits only; the operator sees the character
/// when the host echoes it back. In half duplex the terminal makes a local
/// copy while transmitting, so host echo must normally be disabled to avoid a
/// duplicate character.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalDuplex {
    FullDuplexRemoteEcho,
    HalfDuplexLocalEcho,
}

impl TerminalDuplex {
    pub const ALL: [Self; 2] = [
        Self::FullDuplexRemoteEcho,
        Self::HalfDuplexLocalEcho,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FullDuplexRemoteEcho => "Full duplex / remote echo",
            Self::HalfDuplexLocalEcho => "Half duplex / local echo",
        }
    }

    pub const fn local_echo(self) -> bool {
        matches!(self, Self::HalfDuplexLocalEcho)
    }
}

impl Default for TerminalDuplex {
    fn default() -> Self {
        Self::FullDuplexRemoteEcho
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_duplex_is_the_default_for_echoing_hosts() {
        assert_eq!(TerminalDuplex::default(), TerminalDuplex::FullDuplexRemoteEcho);
        assert!(!TerminalDuplex::default().local_echo());
        assert!(TerminalDuplex::HalfDuplexLocalEcho.local_echo());
    }
}

use super::{CpuBoard, MachineConfig, S100HardwareConfig, S100InstalledCardConfig};

impl S100HardwareConfig {
    /// The one CPU board physically installed in the fitted S-100 connectors.
    ///
    /// A valid hardware assembly contains exactly one CPU card. Returning
    /// `Option` keeps this helper safe while a POWER-OFF editor is temporarily
    /// assembling an invalid configuration before validation/commit.
    pub fn active_cpu_board_slot(self) -> Option<(usize, CpuBoard)> {
        let mut boards = self.installed_cards().filter_map(|(slot, card)| match card {
            S100InstalledCardConfig::Mits8080Cpu => Some((slot, CpuBoard::Mits8080)),
            _ => None,
        });
        let board = boards.next()?;
        boards.next().is_none().then_some(board)
    }

    pub fn active_cpu_board(self) -> Option<CpuBoard> {
        self.active_cpu_board_slot().map(|(_, board)| board)
    }
}

impl MachineConfig {
    /// Runtime/UI CPU identity. The S-100 card inventory is the only authority.
    /// `cpu_model` remains persisted solely so older config.ini files can migrate.
    pub fn active_cpu_board(self) -> CpuBoard {
        self.s100_hardware
            .active_cpu_board()
            .expect("validated MachineConfig must contain exactly one S-100 CPU board")
    }

    pub fn active_cpu_board_slot(self) -> (usize, CpuBoard) {
        self.s100_hardware
            .active_cpu_board_slot()
            .expect("validated MachineConfig must contain exactly one S-100 CPU board")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FastRamCompatibilityConfig, S100InstalledCardConfig};
    use crate::s100_chassis::S100ChassisConfig;

    #[test]
    fn active_cpu_board_identity_and_slot_come_from_the_installed_s100_card() {
        let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
        hardware
            .set_slot(
                2,
                Some(S100InstalledCardConfig::FastRamCompatibility(
                    FastRamCompatibilityConfig::no_wait(0, 0x1000),
                )),
            )
            .unwrap();
        hardware.set_slot(5, Some(S100InstalledCardConfig::Mits8080Cpu)).unwrap();
        let hardware = hardware.validate().unwrap();

        assert_eq!(hardware.active_cpu_board_slot(), Some((5, CpuBoard::Mits8080)));
        assert_eq!(hardware.active_cpu_board(), Some(CpuBoard::Mits8080));

        let mut machine = MachineConfig::default();
        machine.s100_hardware = hardware;
        assert_eq!(machine.active_cpu_board_slot(), (5, CpuBoard::Mits8080));
        assert_eq!(machine.active_cpu_board(), CpuBoard::Mits8080);
    }
}

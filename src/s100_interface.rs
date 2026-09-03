//! Canonical software view of one physical S-100 card connector.
//!
//! Every slotted card in RusTair uses this exact bus-facing contract regardless
//! of card family. A CPU, RAM board, serial interface, interrupt controller or
//! future DMA board differs only in the contacts it declares and the logic it
//! applies when observing/driving those contacts.
//!
//! Host-side inspection handles (debugger, RAM viewer, teacher, diagnostics)
//! are intentionally *not* part of this interface. They may inspect the same
//! physical card state directly, but guest execution can reach another device
//! only through `S100BusCard` and the resolved S-100 backplane.

pub use crate::s100::{
    S100Card, S100CardClass, S100CardContact, S100CardDescriptor, S100ContactRole,
    S100Signal,
};
pub use crate::s100_backplane::{
    S100BusSample, S100CardDrive, S100ElectricalCard as S100BusCard, S100PinDrive,
    S100ResolvedPin,
};

#[cfg(test)]
mod tests {
    use super::S100BusCard;
    use crate::s100_cpu::Mits8080CpuBoard;
    use crate::s100_runtime_ram::RuntimeRamCard;

    fn assert_bus_card<T: S100BusCard>() {}

    #[test]
    fn live_cpu_and_ram_use_the_same_s100_bus_card_interface() {
        assert_bus_card::<Mits8080CpuBoard>();
        assert_bus_card::<RuntimeRamCard>();
    }
}

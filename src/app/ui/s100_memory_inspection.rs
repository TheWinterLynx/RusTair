use crate::config::S100InstalledCardConfig;
use crate::s100_runtime::{RuntimeMemoryInspection, RuntimeRamDriver, S100_OPEN_BUS_VALUE};
use crate::s100_runtime_ram::RuntimeRamConfig;

pub(super) fn ram_config_label(config: RuntimeRamConfig) -> &'static str {
    config
        .historical_model()
        .map(|model| model.label())
        .unwrap_or("Fast RAM compatibility (non-historical)")
}

pub(super) fn ram_config_window(config: RuntimeRamConfig) -> (u16, u16) {
    let start = config.base_address();
    let end = (u32::from(start) + config.populated_bytes() as u32 - 1) as u16;
    (start, end)
}

pub(super) fn ram_driver_line(driver: &RuntimeRamDriver) -> String {
    let (start, end) = ram_config_window(driver.config);
    format!(
        "Slot {:02} · {} · {:04X}h-{:04X}h · {:02X}h · {}",
        driver.slot,
        ram_config_label(driver.config),
        start,
        end,
        driver.value,
        if driver.protected { "PROTECTED" } else { "writable" },
    )
}

pub(super) fn mapping_summary(inspection: &RuntimeMemoryInspection) -> String {
    match inspection.drivers.as_slice() {
        [] => format!("UNMAPPED · open bus {:02X}h", S100_OPEN_BUS_VALUE),
        [driver] => format!("Slot {:02} · {}", driver.slot, ram_config_label(driver.config)),
        drivers => format!(
            "OVERLAP · slots {}{}",
            drivers
                .iter()
                .map(|driver| format!("{:02}", driver.slot))
                .collect::<Vec<_>>()
                .join(" + "),
            if inspection.electrically_contended() { " · CONTENTION" } else { "" },
        ),
    }
}

pub(super) fn mapping_detail(address: u16, inspection: &RuntimeMemoryInspection) -> String {
    let mut text = format!("S-100 mapping at {address:04X}h: {}", mapping_summary(inspection));
    if inspection.drivers.is_empty() {
        text.push_str("\nNo RAM card decodes this address. A guest memory read sees the S-100 open-bus value.");
        return text;
    }
    for driver in &inspection.drivers {
        text.push_str("\n");
        text.push_str(&ram_driver_line(driver));
    }
    if inspection.is_overlap() {
        if inspection.electrically_contended() {
            text.push_str("\nDifferent cards are driving different DI bits/values: this is real electrical bus contention, not a debugger ambiguity.");
        } else {
            text.push_str("\nMultiple cards decode the address and currently drive the same byte. The mapping is still physically overlapped even though DI is not contended at this instant.");
        }
    }
    text
}

#[cfg(test)]
fn single_driver(inspection: &RuntimeMemoryInspection) -> Option<&RuntimeRamDriver> {
    match inspection.drivers.as_slice() {
        [driver] => Some(driver),
        _ => None,
    }
}

pub(super) fn card_window(card: S100InstalledCardConfig) -> Option<(u16, u16, &'static str)> {
    match card {
        S100InstalledCardConfig::Ram(config) => {
            let start = config.base_address;
            let end = (u32::from(start) + config.populated_bytes as u32 - 1) as u16;
            Some((start, end, config.model.label()))
        }
        S100InstalledCardConfig::FastRamCompatibility(config) => {
            let start = config.base_address;
            let end = (u32::from(start) + config.populated_bytes as u32 - 1) as u16;
            Some((start, end, "Fast RAM compatibility (non-historical)"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FastRamCompatibilityConfig, RamInit, S100HardwareConfig};
    use crate::s100_chassis::S100ChassisConfig;
    use crate::s100_runtime::S100RuntimeFabric;

    #[test]
    fn presentation_reports_unmapped_unique_and_overlap_from_runtime_inspection() {
        let chassis = S100ChassisConfig::altair_8800b(6);
        let mut hardware = S100HardwareConfig::empty(chassis).unwrap();
        hardware.set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu)).unwrap();
        hardware.set_slot(2, Some(S100InstalledCardConfig::FastRamCompatibility(
            FastRamCompatibilityConfig::no_wait(0x0000, 0x1000),
        ))).unwrap();
        hardware.set_slot(3, Some(S100InstalledCardConfig::FastRamCompatibility(
            FastRamCompatibilityConfig::no_wait(0x0800, 0x1000),
        ))).unwrap();
        let fabric = S100RuntimeFabric::new(hardware, RamInit::Zeroed).unwrap();

        assert!(mapping_summary(&fabric.inspect_memory(0x2000)).contains("UNMAPPED"));
        assert!(mapping_summary(&fabric.inspect_memory(0x0400)).contains("Slot 02"));
        assert!(mapping_summary(&fabric.inspect_memory(0x0900)).contains("OVERLAP"));
        assert!(!fabric.inspect_memory(0x0900).electrically_contended());

        assert!(fabric.write_unique_memory(0x0400, 0x12, false));
        let inspection = fabric.inspect_memory(0x0400);
        let driver = single_driver(&inspection).unwrap();
        assert_eq!(driver.value, 0x12);
    }
}

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

/// Byte visible on DI when host instrumentation examines the physical RAM
/// responders without fabricating a CPU cycle.
///
/// Unmapped space intentionally returns `None` here even though a guest read
/// sees the chassis open-bus bias (FFh): there is no RAM cell to edit. An
/// overlap returns a byte only when every responding card currently drives the
/// same value. Different values are real electrical contention and therefore do
/// not have one truthful RAM byte for a debugger to present.
pub(super) fn visible_ram_value(inspection: &RuntimeMemoryInspection) -> Option<u8> {
    let first = inspection.drivers.first()?.value;
    (!inspection.electrically_contended()).then_some(first)
}

pub(super) fn mapping_cell_text(inspection: &RuntimeMemoryInspection) -> String {
    if inspection.is_unmapped() {
        "--".into()
    } else if inspection.electrically_contended() {
        "!!".into()
    } else {
        format!("{:02X}", visible_ram_value(inspection).expect("non-contended RAM drivers"))
    }
}

pub(super) fn mapping_detail(address: u16, inspection: &RuntimeMemoryInspection) -> String {
    let mut text = format!("S-100 mapping at {address:04X}h: {}", mapping_summary(inspection));
    if inspection.drivers.is_empty() {
        text.push_str("\nNo RAM card decodes this address. A guest memory read sees the S-100 open-bus value FFh.");
        return text;
    }
    for driver in &inspection.drivers {
        text.push_str("\n");
        text.push_str(&ram_driver_line(driver));
    }
    if inspection.is_overlap() {
        if inspection.electrically_contended() {
            text.push_str("\nDifferent cards are driving different DI values: this is real electrical bus contention, not a debugger ambiguity.");
        } else {
            text.push_str("\nMultiple cards decode the address and currently drive the same byte. The mapping is still physically overlapped even though DI is not contended at this instant.");
        }
    }
    text
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

        let unmapped = fabric.inspect_memory(0x2000);
        assert!(mapping_summary(&unmapped).contains("UNMAPPED"));
        assert_eq!(mapping_cell_text(&unmapped), "--");
        assert_eq!(visible_ram_value(&unmapped), None);

        let unique = fabric.inspect_memory(0x0400);
        assert!(mapping_summary(&unique).contains("Slot 02"));
        assert_eq!(mapping_cell_text(&unique), "00");
        assert_eq!(visible_ram_value(&unique), Some(0));

        let overlap = fabric.inspect_memory(0x0900);
        assert!(mapping_summary(&overlap).contains("OVERLAP"));
        assert!(!overlap.electrically_contended());
        assert_eq!(mapping_cell_text(&overlap), "00");
        assert_eq!(visible_ram_value(&overlap), Some(0));
    }

    #[test]
    fn presentation_never_invents_one_byte_for_electrical_contention() {
        let config = RuntimeRamConfig::Compatibility(FastRamCompatibilityConfig::no_wait(
            0x2000,
            0x1000,
        ));
        let inspection = RuntimeMemoryInspection {
            drivers: vec![
                RuntimeRamDriver {
                    slot: 2,
                    value: 0x00,
                    protected: false,
                    config,
                },
                RuntimeRamDriver {
                    slot: 3,
                    value: 0xff,
                    protected: false,
                    config,
                },
            ],
        };
        assert!(inspection.electrically_contended());
        assert_eq!(mapping_cell_text(&inspection), "!!");
        assert_eq!(visible_ram_value(&inspection), None);
        assert!(mapping_summary(&inspection).contains("CONTENTION"));
    }
}

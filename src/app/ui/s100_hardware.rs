use super::*;
use crate::config::{
    fitted_connector_choices, S100HardwareConfig, S100InstalledCardConfig,
    S100InstalledCardKind, SioAddressPair, SioBaudRate, SioDataBits, SioInterface,
    SioInterruptTarget, SioParity, SioRevision, SioStopBits, TwoSioAddressBlock,
    TwoSioBaudTap, TwoSioInterruptTarget, TwoSioSignalInterface,
};
use crate::s100_chassis::{AltairChassisModel, S100ChassisConfig};
use crate::s100_memory::{S100RamBoardModel, S100RamCardConfig};

pub(in crate::app) fn draw_s100_hardware_menu(app: &mut RusTairApp, ui: &mut egui::Ui) {
    let hardware = app.config.machine.s100_hardware;
    let powered = app.machine.powered();

    ui.label(format!("Chassis: {}", hardware.chassis.model.label()));
    ui.small(format!(
        "{} fitted S-100 connectors / {} physical positions",
        hardware.chassis.fitted_connectors,
        hardware.chassis.physical_slot_positions()
    ));
    ui.small(format!(
        "Installed RAM across cards: {} KiB",
        hardware.installed_ram_bytes() / 1024
    ));
    ui.separator();

    ui.add_enabled_ui(!powered, |ui| {
        ui.menu_button(
            format!("Chassis model: {}", hardware.chassis.model.label()),
            |ui| {
                for model in AltairChassisModel::ALL {
                    if ui
                        .selectable_label(hardware.chassis.model == model, model.label())
                        .clicked()
                    {
                        change_chassis_model(app, hardware, model);
                        ui.close();
                    }
                }
            },
        );

        ui.menu_button(
            format!("Fitted connectors: {}", hardware.chassis.fitted_connectors),
            |ui| {
                for &connectors in fitted_connector_choices(hardware.chassis.model) {
                    if ui
                        .selectable_label(
                            hardware.chassis.fitted_connectors == connectors,
                            connectors.to_string(),
                        )
                        .clicked()
                    {
                        let mut candidate = hardware;
                        match candidate.set_chassis(S100ChassisConfig {
                            model: hardware.chassis.model,
                            fitted_connectors: connectors,
                        }) {
                            Ok(()) => commit_hardware(app, candidate, "S-100 connector population changed"),
                            Err(error) => {
                                app.status = format!(
                                    "Cannot reduce S-100 connectors without removing cards first: {error:?}"
                                );
                            }
                        }
                        ui.close();
                    }
                }
            },
        );

        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(520.0)
            .show(ui, |ui| {
                for slot in 1..=hardware.fitted_connectors() {
                    let card = hardware.slot(slot);
                    ui.menu_button(
                        format!("Slot {slot:02} — {}", card_summary(card)),
                        |ui| draw_slot_menu(app, hardware, slot, card, ui),
                    );
                }
            });
    });

    if powered {
        ui.separator();
        ui.small("POWER OFF required to move cards, change chassis connectors, or alter physical card straps.");
    }
    ui.separator();
    ui.small("CPU and RAM entries are the live execution topology used by both Fast and Cycle. 88-SIO/88-2SIO entries are already persisted per slot but still use the transitional serial runtime until those boards are connected as live S-100 bus cards.");
}

fn change_chassis_model(
    app: &mut RusTairApp,
    hardware: S100HardwareConfig,
    model: AltairChassisModel,
) {
    let highest_used = hardware
        .installed_cards()
        .map(|(slot, _)| slot)
        .max()
        .unwrap_or(1);
    let choices = fitted_connector_choices(model);
    let connectors = choices
        .iter()
        .copied()
        .find(|&count| count >= highest_used)
        .unwrap_or_else(|| *choices.last().expect("every chassis has connector choices"));
    let mut candidate = hardware;
    match candidate.set_chassis(S100ChassisConfig {
        model,
        fitted_connectors: connectors,
    }) {
        Ok(()) => commit_hardware(app, candidate, "S-100 chassis model changed"),
        Err(error) => {
            app.status = format!(
                "Cannot select {} while cards occupy unsupported slots: {error:?}",
                model.label()
            );
        }
    }
}

fn draw_slot_menu(
    app: &mut RusTairApp,
    hardware: S100HardwareConfig,
    slot: usize,
    card: Option<S100InstalledCardConfig>,
    ui: &mut egui::Ui,
) {
    if ui.selectable_label(card.is_none(), "Empty").clicked() {
        let mut candidate = hardware;
        if candidate.set_slot(slot, None).is_ok() {
            commit_hardware(app, candidate, &format!("S-100 slot {slot} emptied"));
        }
        ui.close();
        return;
    }

    ui.separator();
    ui.label("Installed card");
    for kind in S100InstalledCardKind::ALL {
        let selected = card.is_some_and(|card| card.kind() == kind);
        if ui.selectable_label(selected, kind.label()).clicked() {
            let mut candidate = hardware;
            if kind == S100InstalledCardKind::Mits8080Cpu {
                for cpu_slot in hardware.cpu_slots().collect::<Vec<_>>() {
                    if cpu_slot != slot {
                        let _ = candidate.set_slot(cpu_slot, None);
                    }
                }
            }
            let next = default_card_for_kind(hardware, kind);
            match candidate.set_slot(slot, Some(next)) {
                Ok(()) => commit_hardware(
                    app,
                    candidate,
                    &format!("S-100 slot {slot}: {}", kind.label()),
                ),
                Err(error) => app.status = format!("Invalid S-100 card configuration: {error:?}"),
            }
            ui.close();
            return;
        }
    }

    if let Some(card) = card {
        ui.separator();
        match card {
            S100InstalledCardConfig::Ram(config) => {
                draw_ram_card_configuration(app, hardware, slot, config, ui)
            }
            S100InstalledCardConfig::Mits88Sio(config) => {
                draw_sio_card_configuration(app, hardware, slot, config, ui)
            }
            S100InstalledCardConfig::Mits88TwoSio {
                straps,
                interrupt_wiring,
            } => draw_two_sio_card_configuration(
                app,
                hardware,
                slot,
                straps,
                interrupt_wiring,
                ui,
            ),
            S100InstalledCardConfig::FastRamCompatibility(config) => {
                ui.small(format!(
                    "Compatibility RAM: {:04X}h + {} bytes · {} read wait(s)",
                    config.base_address, config.populated_bytes, config.read_wait_states
                ));
                ui.small("Non-historical migration card. Replace it with real MITS RAM boards for hardware-fidelity configurations.");
            }
            S100InstalledCardConfig::Mits8080Cpu => {
                ui.small("Intel 8080 at the MITS board's authentic 2 MHz hardware clock. Fast/Cycle are execution engines, not different cards.");
            }
        }
    }
}

fn draw_ram_card_configuration(
    app: &mut RusTairApp,
    hardware: S100HardwareConfig,
    slot: usize,
    config: S100RamCardConfig,
    ui: &mut egui::Ui,
) {
    ui.label("RAM card straps");
    ui.menu_button(
        format!("Base address: {:04X}h", config.base_address),
        |ui| {
            let quantum = config.model.address_granularity();
            let last = 0x1_0000usize - config.model.capacity_bytes();
            for base in (0..=last).step_by(quantum) {
                let base = base as u16;
                if ui
                    .selectable_label(config.base_address == base, format!("{base:04X}h"))
                    .clicked()
                {
                    let next = S100RamCardConfig {
                        base_address: base,
                        ..config
                    };
                    replace_slot(app, hardware, slot, S100InstalledCardConfig::Ram(next));
                    ui.close();
                }
            }
        },
    );

    if config.model == S100RamBoardModel::Mits1KStatic88Mcs {
        ui.menu_button(
            format!("Populated: {} bytes", config.populated_bytes),
            |ui| {
                for bytes in [256usize, 512, 768, 1024] {
                    if ui
                        .selectable_label(config.populated_bytes == bytes, format!("{bytes} bytes"))
                        .clicked()
                    {
                        let next = S100RamCardConfig {
                            populated_bytes: bytes,
                            ..config
                        };
                        replace_slot(app, hardware, slot, S100InstalledCardConfig::Ram(next));
                        ui.close();
                    }
                }
            },
        );
    } else {
        ui.small(format!("Populated: {} KiB", config.populated_bytes / 1024));
    }

    ui.small(format!("Timing: {:?}", config.model.timing_model()));
    if !config.model.timing_fully_implemented() {
        ui.small("Dynamic refresh behavior is intentionally not yet marked fidelity-PASS.");
    }
}

fn draw_sio_card_configuration(
    app: &mut RusTairApp,
    hardware: S100HardwareConfig,
    slot: usize,
    config: crate::config::SioHardwareConfig,
    ui: &mut egui::Ui,
) {
    ui.label("88-SIO physical configuration");

    ui.menu_button(format!("Revision: {}", config.revision.label()), |ui| {
        for revision in SioRevision::ALL {
            if ui.selectable_label(config.revision == revision, revision.label()).clicked() {
                let mut next = config;
                next.revision = revision;
                replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                ui.close();
            }
        }
    });
    ui.menu_button(format!("Interface: {}", config.interface.label()), |ui| {
        for interface in SioInterface::ALL {
            if ui.selectable_label(config.interface == interface, interface.label()).clicked() {
                let mut next = config;
                next.interface = interface;
                replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                ui.close();
            }
        }
    });
    ui.menu_button(
        format!("I/O address: {:02X}h/{:02X}h", config.address.status(), config.address.data()),
        |ui| {
            for base in (0u8..=0xfe).step_by(2) {
                let address = SioAddressPair::try_new(base).expect("even address");
                if ui
                    .selectable_label(config.address == address, format!("{base:02X}h/{:02X}h", base + 1))
                    .clicked()
                {
                    let mut next = config;
                    next.address = address;
                    replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                    ui.close();
                }
            }
        },
    );
    ui.menu_button(format!("Baud: {}", config.baud.label()), |ui| {
        for baud in SioBaudRate::STANDARD {
            if ui.selectable_label(config.baud == baud, baud.label()).clicked() {
                let mut next = config;
                next.baud = baud;
                replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                ui.close();
            }
        }
    });
    ui.menu_button(format!("Data bits: {}", config.format.data_bits.bits()), |ui| {
        for data_bits in SioDataBits::ALL {
            if ui.selectable_label(config.format.data_bits == data_bits, data_bits.label()).clicked() {
                let mut next = config;
                next.format.data_bits = data_bits;
                replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                ui.close();
            }
        }
    });
    ui.menu_button(format!("Parity: {}", config.format.parity.label()), |ui| {
        for parity in SioParity::ALL {
            if ui.selectable_label(config.format.parity == parity, parity.label()).clicked() {
                let mut next = config;
                next.format.parity = parity;
                replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                ui.close();
            }
        }
    });
    ui.menu_button(format!("Stop bits: {}", config.format.stop_bits.bits()), |ui| {
        for stop_bits in SioStopBits::ALL {
            if ui.selectable_label(config.format.stop_bits == stop_bits, stop_bits.label()).clicked() {
                let mut next = config;
                next.format.stop_bits = stop_bits;
                replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                ui.close();
            }
        }
    });
    ui.menu_button(
        format!("Input IRQ: {}", config.interrupt_wiring.input.label()),
        |ui| {
            for target in SioInterruptTarget::ALL {
                if ui.selectable_label(config.interrupt_wiring.input == target, target.label()).clicked() {
                    let mut next = config;
                    next.interrupt_wiring.input = target;
                    replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                    ui.close();
                }
            }
        },
    );
    ui.menu_button(
        format!("Output IRQ: {}", config.interrupt_wiring.output.label()),
        |ui| {
            for target in SioInterruptTarget::ALL {
                if ui.selectable_label(config.interrupt_wiring.output == target, target.label()).clicked() {
                    let mut next = config;
                    next.interrupt_wiring.output = target;
                    replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88Sio(next));
                    ui.close();
                }
            }
        },
    );
}

fn draw_two_sio_card_configuration(
    app: &mut RusTairApp,
    hardware: S100HardwareConfig,
    slot: usize,
    straps: crate::config::TwoSioStraps,
    interrupt_wiring: crate::config::TwoSioInterruptWiring,
    ui: &mut egui::Ui,
) {
    ui.label("88-2SIO physical straps");
    ui.menu_button(
        format!("I/O block: {:02X}h–{:02X}h", straps.address.base(), straps.address.base() + 3),
        |ui| {
            for base in (0u8..=0xf8).step_by(4) {
                let address = TwoSioAddressBlock::try_new(base).expect("aligned 88-2SIO block");
                if ui.selectable_label(straps.address == address, format!("{base:02X}h–{:02X}h", base + 3)).clicked() {
                    let mut next = straps;
                    next.address = address;
                    replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88TwoSio { straps: next, interrupt_wiring });
                    ui.close();
                }
            }
        },
    );

    for port in 0..2 {
        let baud = if port == 0 { straps.port0_baud } else { straps.port1_baud };
        ui.menu_button(format!("Port {port} baud tap: {}", baud.label()), |ui| {
            for next_baud in TwoSioBaudTap::ALL {
                if ui.selectable_label(baud == next_baud, next_baud.label()).clicked() {
                    let mut next = straps;
                    if port == 0 { next.port0_baud = next_baud; } else { next.port1_baud = next_baud; }
                    replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88TwoSio { straps: next, interrupt_wiring });
                    ui.close();
                }
            }
        });

        let interface = if port == 0 { straps.port0_interface } else { straps.port1_interface };
        ui.menu_button(format!("Port {port} interface: {}", interface.label()), |ui| {
            for next_interface in TwoSioSignalInterface::ALL {
                if ui.selectable_label(interface == next_interface, next_interface.label()).clicked() {
                    let mut next = straps;
                    if port == 0 { next.port0_interface = next_interface; } else { next.port1_interface = next_interface; }
                    replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88TwoSio { straps: next, interrupt_wiring });
                    ui.close();
                }
            }
        });

        let target = if port == 0 { interrupt_wiring.port0 } else { interrupt_wiring.port1 };
        ui.menu_button(format!("Port {port} IRQ: {}", target.label()), |ui| {
            for next_target in TwoSioInterruptTarget::ALL {
                if ui.selectable_label(target == next_target, next_target.label()).clicked() {
                    let mut next = interrupt_wiring;
                    if port == 0 { next.port0 = next_target; } else { next.port1 = next_target; }
                    replace_slot(app, hardware, slot, S100InstalledCardConfig::Mits88TwoSio { straps, interrupt_wiring: next });
                    ui.close();
                }
            }
        });
    }
}

fn replace_slot(
    app: &mut RusTairApp,
    hardware: S100HardwareConfig,
    slot: usize,
    card: S100InstalledCardConfig,
) {
    let mut candidate = hardware;
    match candidate.set_slot(slot, Some(card)) {
        Ok(()) => commit_hardware(app, candidate, &format!("S-100 slot {slot} straps updated")),
        Err(error) => app.status = format!("Invalid S-100 slot {slot} configuration: {error:?}"),
    }
}

fn commit_hardware(app: &mut RusTairApp, candidate: S100HardwareConfig, action: &str) {
    match candidate.validate() {
        Ok(valid) => app.apply_s100_hardware_configuration(valid, action),
        Err(error) => {
            app.status = format!("S-100 inventory rejected: {error:?}");
        }
    }
}

fn default_card_for_kind(
    hardware: S100HardwareConfig,
    kind: S100InstalledCardKind,
) -> S100InstalledCardConfig {
    if let Some(model) = kind.ram_model() {
        let base = first_free_ram_base(hardware, model).unwrap_or(0);
        return S100InstalledCardConfig::Ram(S100RamCardConfig::fully_populated(model, base));
    }
    S100InstalledCardConfig::default_for_kind(kind)
}

fn first_free_ram_base(
    hardware: S100HardwareConfig,
    model: S100RamBoardModel,
) -> Option<u16> {
    let quantum = model.address_granularity();
    let size = model.capacity_bytes();
    let last = 0x1_0000usize.checked_sub(size)?;
    (0..=last)
        .step_by(quantum)
        .find(|&base| {
            let end = base + size;
            hardware.installed_cards().all(|(_, card)| {
                let Some((other_base, other_size)) = ram_window(card) else { return true };
                let other_end = other_base + other_size;
                end <= other_base || base >= other_end
            })
        })
        .map(|base| base as u16)
}

fn ram_window(card: S100InstalledCardConfig) -> Option<(usize, usize)> {
    match card {
        S100InstalledCardConfig::Ram(config) => {
            Some((config.base_address as usize, config.model.capacity_bytes()))
        }
        S100InstalledCardConfig::FastRamCompatibility(config) => {
            Some((config.base_address as usize, config.populated_bytes))
        }
        _ => None,
    }
}

fn card_summary(card: Option<S100InstalledCardConfig>) -> String {
    match card {
        None => "Empty".to_owned(),
        Some(S100InstalledCardConfig::Ram(config)) => format!(
            "{} @ {:04X}h",
            config.model.label(),
            config.base_address
        ),
        Some(S100InstalledCardConfig::FastRamCompatibility(config)) => format!(
            "Fast RAM compatibility @ {:04X}h ({} bytes)",
            config.base_address, config.populated_bytes
        ),
        Some(card) => card.kind().label().to_owned(),
    }
}

use super::*;
use crate::config::S100InstalledCardConfig;

const S100_HARDWARE_EDITOR_OPEN_ID: &str = "rustair-s100-hardware-editor-open";

pub(in crate::app) fn open_s100_hardware_editor(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new(S100_HARDWARE_EDITOR_OPEN_ID), true);
    });
}

pub(in crate::app) fn show_s100_hardware_editor(
    app: &mut RusTairApp,
    parent_ctx: &egui::Context,
) {
    let id = egui::Id::new(S100_HARDWARE_EDITOR_OPEN_ID);
    let open = parent_ctx
        .data(|data| data.get_temp::<bool>(id))
        .unwrap_or(false);
    if !open {
        return;
    }

    parent_ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("rustair-s100-hardware-editor"),
        egui::ViewportBuilder::default()
            .with_title("RusTair — S-100 Hardware")
            .with_inner_size([1040.0, 780.0])
            .with_min_inner_size([700.0, 520.0])
            .with_resizable(true),
        |ctx, _class| {
            egui::TopBottomPanel::top("s100-hardware-editor-summary").show(ctx, |ui| {
                let hardware = app.config.machine.s100_hardware;
                ui.horizontal_wrapped(|ui| {
                    ui.heading("S-100 Hardware");
                    ui.separator();
                    ui.label(hardware.chassis.model.label());
                    ui.separator();
                    ui.label(format!("{} connectors", hardware.fitted_connectors()));
                    ui.separator();
                    ui.label(format!("{} KiB RAM", hardware.installed_ram_bytes() / 1024));
                    ui.separator();
                    ui.strong(if app.machine.powered() {
                        "POWER ON — hardware editing locked"
                    } else {
                        "POWER OFF — hardware editing enabled"
                    });
                });
            });

            egui::SidePanel::left("s100-hardware-inventory")
                .resizable(true)
                .default_width(250.0)
                .width_range(190.0..=360.0)
                .show(ctx, |ui| {
                    ui.heading("Physical inventory");
                    ui.small("The list below is the mounted S-100 slot inventory used by the machine.");
                    ui.separator();

                    let hardware = app.config.machine.s100_hardware;
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for slot in 1..=hardware.fitted_connectors() {
                            ui.horizontal_wrapped(|ui| {
                                ui.monospace(format!("{slot:02}"));
                                ui.label(card_summary(hardware.slot(slot)));
                            });
                        }
                    });
                });

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Chassis and card configuration");
                ui.label("Configure the physical Altair chassis, fitted S-100 connectors, installed cards and card straps here.");
                ui.small("All mutations still pass through the existing validated S-100 hardware editor and the single machine.s100_hardware authority.");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    super::draw_s100_hardware_menu(app, ui);
                });
            });

            if ctx.input(|input| input.viewport().close_requested()) {
                parent_ctx.data_mut(|data| data.insert_temp(id, false));
            }
        },
    );
}

fn card_summary(card: Option<S100InstalledCardConfig>) -> String {
    match card {
        None => "Empty".to_owned(),
        Some(S100InstalledCardConfig::Ram(config)) => {
            format!("{} @ {:04X}h", config.model.label(), config.base_address)
        }
        Some(S100InstalledCardConfig::FastRamCompatibility(config)) => format!(
            "Fast RAM compatibility @ {:04X}h ({} bytes)",
            config.base_address, config.populated_bytes
        ),
        Some(card) => card.kind().label().to_owned(),
    }
}

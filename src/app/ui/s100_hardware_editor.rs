use super::*;

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
            .with_inner_size([860.0, 760.0])
            .with_min_inner_size([700.0, 560.0])
            .with_resizable(true),
        |ctx, _class| {
            egui::TopBottomPanel::top("s100-hardware-editor-summary").show(ctx, |ui| {
                let hardware = app.config.machine.s100_hardware;
                ui.horizontal_wrapped(|ui| {
                    ui.heading("S-100 Hardware");
                    ui.separator();
                    ui.label(format!("{}", hardware.chassis.model.label()));
                    ui.separator();
                    ui.label(format!("{} connectors", hardware.fitted_connectors()));
                    ui.separator();
                    ui.label(format!("{} KiB RAM", hardware.installed_ram_bytes() / 1024));
                    ui.separator();
                    ui.label(if app.machine.powered() {
                        "POWER ON — hardware editing locked"
                    } else {
                        "POWER OFF — hardware editing enabled"
                    });
                });
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Configure the physical Altair chassis, fitted S-100 connectors, installed cards and card straps from one dedicated hardware editor.");
                ui.small("This window edits the same slot-native hardware authority used by the running machine; it does not maintain a second configuration model.");
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

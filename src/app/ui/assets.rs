use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, FontFamily};

use super::front_panel_assets::load_switch_textures;
use crate::embedded_assets;

/// GPU/UI resources used by the application renderers.
///
/// Keeping asset loading here prevents the application composition root from
/// knowing image-decoding details while leaving rendering behavior unchanged.
pub(in crate::app) struct Tex {
    pub(in crate::app) panel: Option<egui::TextureHandle>,
    pub(in crate::app) switch_sprites: HashMap<&'static str, egui::TextureHandle>,
    pub(in crate::app) tty_body: Option<egui::TextureHandle>,
    pub(in crate::app) tty_keys: Option<egui::TextureHandle>,
    pub(in crate::app) tty_key_up: Option<egui::TextureHandle>,
    pub(in crate::app) tty_key_mid: Option<egui::TextureHandle>,
    pub(in crate::app) tty_spacebar_up: Option<egui::TextureHandle>,
    pub(in crate::app) tty_spacebar_mid: Option<egui::TextureHandle>,
    pub(in crate::app) tty_head: Option<egui::TextureHandle>,
    pub(in crate::app) tty_line_local: Option<egui::TextureHandle>,
    pub(in crate::app) tty_knob: Option<egui::TextureHandle>,
}

impl Tex {
    pub(in crate::app) fn load(ctx: &egui::Context) -> Self {
        Self {
            panel: Self::load_texture(
                ctx,
                "front-panel",
                "assets/panels/white-pivot/panel.png",
            ),
            switch_sprites: load_switch_textures(ctx),
            tty_body: Self::load_texture(ctx, "tty-body", "assets/asr33_body_clean.png"),
            // The clean body contains the key wells and each key is painted
            // independently from aligned photographic poses.
            tty_keys: None,
            tty_key_up: Self::load_key_pose_texture(
                ctx,
                "tty-key-up",
                "assets/asr33_key_up.png",
            ),
            tty_key_mid: Self::load_key_pose_texture(
                ctx,
                "tty-key-mid",
                "assets/asr33_key_mid.png",
            ),
            tty_spacebar_up: Self::load_key_pose_texture(
                ctx,
                "tty-spacebar-up",
                "assets/as33_spacebar_up.png",
            ),
            tty_spacebar_mid: Self::load_key_pose_texture(
                ctx,
                "tty-spacebar-mid",
                "assets/as33_spacebar_mid.png",
            ),
            tty_head: Self::load_texture(ctx, "tty-head", "assets/asr33head.png"),
            tty_line_local: Self::load_texture(
                ctx,
                "tty-line-local",
                "assets/asrlinelocal.png",
            ),
            tty_knob: Self::load_texture(ctx, "tty-knob", "assets/asrlinelocalknob.png"),
        }
    }

    pub(in crate::app) fn install_teletype_font(ctx: &egui::Context) {
        let Some(bytes) = embedded_assets::get("assets/teletype.ttf") else {
            return;
        };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "teletype".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes.to_vec())),
        );
        fonts.families.insert(
            FontFamily::Name("teletype".into()),
            vec!["teletype".to_owned()],
        );
        ctx.set_fonts(fonts);
    }

    fn load_texture(
        ctx: &egui::Context,
        name: &str,
        path: &str,
    ) -> Option<egui::TextureHandle> {
        let bytes = embedded_assets::get(path)?;
        let image = image::load_from_memory(bytes).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        Some(ctx.load_texture(
            name,
            egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw()),
            egui::TextureOptions::LINEAR,
        ))
    }

    /// Load one ASR-33 key pose while removing only transparent vertical
    /// padding. The original canvas width is preserved to retain the horizontal
    /// registration authored in the source sprites.
    fn load_key_pose_texture(
        ctx: &egui::Context,
        name: &str,
        path: &str,
    ) -> Option<egui::TextureHandle> {
        let bytes = embedded_assets::get(path)?;
        let image = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (width, height) = image.dimensions();

        let mut min_y = height;
        let mut max_y = 0u32;
        let mut found = false;

        const ALPHA_THRESHOLD: u8 = 8;
        for (_x, y, pixel) in image.enumerate_pixels() {
            if pixel[3] <= ALPHA_THRESHOLD {
                continue;
            }
            found = true;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }

        let cropped = if found {
            image::imageops::crop_imm(&image, 0, min_y, width, max_y - min_y + 1).to_image()
        } else {
            image
        };

        let size = [cropped.width() as usize, cropped.height() as usize];
        Some(ctx.load_texture(
            name,
            egui::ColorImage::from_rgba_unmultiplied(size, &cropped.into_raw()),
            egui::TextureOptions::LINEAR,
        ))
    }
}

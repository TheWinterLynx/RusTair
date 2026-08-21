use std::collections::HashMap;

use eframe::egui;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum SwitchSpriteId {
    WhiteUp,
    WhiteCenter,
    WhiteDown,
}

#[derive(Clone, Copy)]
enum SwitchAlphaMode {
    Preserve,
    RemoveBlack,
}

// Retain support for legacy sprites with a black background even though the
// currently selected photographic switch set already carries useful alpha.
const _: SwitchAlphaMode = SwitchAlphaMode::RemoveBlack;

#[derive(Clone, Copy)]
pub(super) struct SwitchSpriteAsset {
    pub(super) path: &'static str,
    pub(super) canvas_size: (f32, f32),
    pub(super) crop_min: (f32, f32),
    pub(super) crop_max: (f32, f32),
    pub(super) pivot: (f32, f32),
    pub(super) source_to_panel: f32,
    alpha_mode: SwitchAlphaMode,
}

impl SwitchSpriteId {
    pub(super) fn asset(self) -> SwitchSpriteAsset {
        const CANVAS: (f32, f32) = (32.0, 96.0);
        const CROP_MIN: (f32, f32) = (0.0, 0.0);
        const CROP_MAX: (f32, f32) = (32.0, 96.0);
        const SOCKET_PIVOT: (f32, f32) = (15.5, 47.5);
        const SOURCE_TO_PANEL: f32 = 1.30;

        let path = match self {
            Self::WhiteUp => "assets/panels/white-pivot/switch_up.png",
            Self::WhiteCenter => "assets/panels/white-pivot/switch_center.png",
            Self::WhiteDown => "assets/panels/white-pivot/switch_down.png",
        };

        SwitchSpriteAsset {
            path,
            canvas_size: CANVAS,
            crop_min: CROP_MIN,
            crop_max: CROP_MAX,
            pivot: SOCKET_PIVOT,
            source_to_panel: SOURCE_TO_PANEL,
            alpha_mode: SwitchAlphaMode::Preserve,
        }
    }
}

fn load_switch_sprite_texture(
    ctx: &egui::Context,
    sprite: SwitchSpriteId,
) -> Option<egui::TextureHandle> {
    let asset = sprite.asset();
    let bytes = std::fs::read(asset.path).ok()?;
    let mut image = image::load_from_memory(&bytes).ok()?.to_rgba8();

    if matches!(asset.alpha_mode, SwitchAlphaMode::RemoveBlack) {
        for pixel in image.pixels_mut() {
            let brightness = pixel[0].max(pixel[1]).max(pixel[2]);
            if brightness <= 2 {
                pixel[3] = 0;
            } else if brightness < 16 {
                pixel[3] = (((brightness - 2) as u16 * 255) / 14) as u8;
            }
        }
    }

    let size = [image.width() as usize, image.height() as usize];
    Some(ctx.load_texture(
        asset.path,
        egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw()),
        egui::TextureOptions::LINEAR,
    ))
}

pub(super) fn load_switch_textures(
    ctx: &egui::Context,
) -> HashMap<&'static str, egui::TextureHandle> {
    let mut textures = HashMap::new();
    for sprite in [
        SwitchSpriteId::WhiteUp,
        SwitchSpriteId::WhiteCenter,
        SwitchSpriteId::WhiteDown,
    ] {
        let key = sprite.asset().path;
        if let Some(texture) = load_switch_sprite_texture(ctx, sprite) {
            textures.insert(key, texture);
        }
    }
    textures
}

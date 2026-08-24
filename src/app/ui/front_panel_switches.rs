use super::front_panel_assets::SwitchSpriteId;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SwitchPosition {
    Up,
    Center,
    Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SwitchKind {
    TwoPosition,
    ThreePosition,
}

#[derive(Clone, Copy, Default)]
pub(super) struct MomentarySwitchUiState {
    pub(super) latched: Option<SwitchPosition>,
    pub(super) press_started: Option<Instant>,
    pub(super) press_direction: Option<SwitchPosition>,
    pub(super) press_began_on_latched: bool,
    pub(super) long_latched_this_press: bool,
}

#[derive(Clone, Copy)]
pub(super) struct SwitchPoseConfig {
    pub(super) sprite: SwitchSpriteId,
    pub(super) offset: (f32, f32),
    pub(super) scale: f32,
}

#[derive(Clone, Copy)]
pub(super) struct SwitchConfig {
    pub(super) name: &'static str,
    pub(super) socket: (f32, f32),
    pub(super) hit_size: (f32, f32),
    pub(super) kind: SwitchKind,
    pub(super) up: SwitchPoseConfig,
    pub(super) center: Option<SwitchPoseConfig>,
    pub(super) down: SwitchPoseConfig,
}

impl SwitchConfig {
    pub(super) fn pose(&self, position: SwitchPosition) -> Option<SwitchPoseConfig> {
        match position {
            SwitchPosition::Up => Some(self.up),
            SwitchPosition::Center => self.center,
            SwitchPosition::Down => Some(self.down),
        }
    }
}

const fn pose(sprite: SwitchSpriteId) -> SwitchPoseConfig {
    SwitchPoseConfig { sprite, offset: (0.0, 0.0), scale: 1.0 }
}

const fn switch_config(
    name: &'static str,
    x: f32,
    y: f32,
    kind: SwitchKind,
) -> SwitchConfig {
    SwitchConfig {
        name,
        socket: (x, y),
        hit_size: (if matches!(kind, SwitchKind::TwoPosition) { 72.0 } else { 76.0 }, if matches!(kind, SwitchKind::TwoPosition) { 92.0 } else { 96.0 }),
        kind,
        up: pose(SwitchSpriteId::WhiteUp),
        center: if matches!(kind, SwitchKind::ThreePosition) { Some(pose(SwitchSpriteId::WhiteCenter)) } else { None },
        down: pose(SwitchSpriteId::WhiteDown),
    }
}

pub(super) const SENSE_SWITCHES: [SwitchConfig; 16] = [
    switch_config("A0", 1665.0, 425.8, SwitchKind::TwoPosition),
    switch_config("A1", 1597.8, 425.8, SwitchKind::TwoPosition),
    switch_config("A2", 1527.0, 425.8, SwitchKind::TwoPosition),
    switch_config("A3", 1426.2, 425.8, SwitchKind::TwoPosition),
    switch_config("A4", 1359.0, 425.8, SwitchKind::TwoPosition),
    switch_config("A5", 1290.6, 425.8, SwitchKind::TwoPosition),
    switch_config("A6", 1192.2, 425.8, SwitchKind::TwoPosition),
    switch_config("A7", 1122.6, 425.8, SwitchKind::TwoPosition),
    switch_config("A8", 1053.0, 425.8, SwitchKind::TwoPosition),
    switch_config("A9", 953.4, 425.8, SwitchKind::TwoPosition),
    switch_config("A10", 883.8, 425.8, SwitchKind::TwoPosition),
    switch_config("A11", 816.6, 425.8, SwitchKind::TwoPosition),
    switch_config("A12", 718.2, 425.8, SwitchKind::TwoPosition),
    switch_config("A13", 648.6, 425.8, SwitchKind::TwoPosition),
    switch_config("A14", 576.6, 425.8, SwitchKind::TwoPosition),
    switch_config("A15", 480.6, 425.8, SwitchKind::TwoPosition),
];

pub(super) const SWITCH_POWER: SwitchConfig = switch_config("POWER", 151.8, 562.2, SwitchKind::TwoPosition);
pub(super) const SWITCH_RUN_STOP: SwitchConfig = switch_config("RUN / STOP", 477.0, 562.2, SwitchKind::ThreePosition);
pub(super) const SWITCH_SINGLE_STEP: SwitchConfig = switch_config("SINGLE STEP", 610.2, 561.0, SwitchKind::ThreePosition);
pub(super) const SWITCH_EXAMINE: SwitchConfig = switch_config("EXAMINE", 748.2, 562.2, SwitchKind::ThreePosition);
pub(super) const SWITCH_DEPOSIT: SwitchConfig = switch_config("DEPOSIT", 885.0, 562.2, SwitchKind::ThreePosition);
pub(super) const SWITCH_RESET: SwitchConfig = switch_config("RESET", 1018.2, 559.8, SwitchKind::ThreePosition);
pub(super) const SWITCH_PROTECT: SwitchConfig = switch_config("PROTECT", 1152.6, 563.4, SwitchKind::ThreePosition);
pub(super) const SWITCH_AUX1: SwitchConfig = switch_config("AUX 1", 1285.8, 559.8, SwitchKind::ThreePosition);
pub(super) const SWITCH_AUX2: SwitchConfig = switch_config("AUX 2", 1423.8, 562.2, SwitchKind::ThreePosition);

from __future__ import annotations

from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if text.count(old) != 1:
        raise RuntimeError(f"{label}: expected exactly one occurrence, found {text.count(old)}")
    return text.replace(old, new, 1)


def move(old: str, new: str) -> Path:
    src = SRC / old
    dst = SRC / new
    if not src.exists():
        raise RuntimeError(f"missing source file: {src}")
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(src), str(dst))
    return dst


# ---------------------------------------------------------------------------
# 1. Turn the historically include!-assembled binary into real Rust modules.
# ---------------------------------------------------------------------------
app_mod = move("application.rs", "app/mod.rs")
runtime = move("application_loop.rs", "app/runtime.rs")
asr_controller = move("teletype_controller.rs", "app/asr33_controller.rs")
terminal_serial = move("terminal_serial.rs", "app/terminal_serial.rs")

front_panel = move("front_panel.rs", "app/ui/front_panel.rs")
asr_renderer = move("teletype_renderer.rs", "app/ui/asr33.rs")
asr_window = move("teletype_io.rs", "app/ui/asr33_window.rs")
terminal_ui = move("terminal.rs", "app/ui/terminal.rs")

move("altair_machine.rs", "machine/mod.rs")
move("teletype.rs", "peripherals/asr33/model.rs")

write(
    SRC / "peripherals/mod.rs",
    "pub mod asr33;\n",
)
write(
    SRC / "peripherals/asr33/mod.rs",
    "mod model;\n\npub use model::*;\n",
)
write(
    SRC / "io/mod.rs",
    "//! Host-side I/O adapters. Serial host integration will live here.\n",
)
write(
    SRC / "app/ui/mod.rs",
    "mod asr33;\nmod asr33_window;\npub(super) mod front_panel;\npub(super) mod terminal;\n",
)

# Child implementation modules explicitly import the app module's shared names.
write(runtime, "use super::*;\n\n" + read(runtime))
write(asr_controller, "use super::*;\n\n" + read(asr_controller))

serial_text = read(terminal_serial)
serial_text = replace_once(
    serial_text,
    "use eframe::egui;\nuse std::time::Instant;\n\nuse crate::RusTairApp;\n\n",
    "use super::*;\n\n",
    "terminal_serial imports",
)
write(terminal_serial, serial_text)

for path in (front_panel, asr_renderer, asr_window, terminal_ui):
    write(path, "use super::super::*;\n\n" + read(path))

# ---------------------------------------------------------------------------
# 2. application.rs becomes app/mod.rs and the executable becomes tiny.
# ---------------------------------------------------------------------------
text = read(app_mod)
text = replace_once(
    text,
    "#[allow(dead_code)]\nmod cpu8080;\nmod altair_machine;\n\n",
    "mod asr33_controller;\nmod runtime;\nmod terminal_serial;\nmod ui;\n\n",
    "app module declarations",
)
text = text.replace(
    "use altair_machine::{AltairMachine, CLOCK_HZ};\nuse rustair::audio::AudioEngine;\nuse rustair::teletype::{self, KeyKind, Mode as TtyMode, PrintEvent, Teletype};",
    "use crate::audio::AudioEngine;\nuse crate::machine::{AltairMachine, CLOCK_HZ};\nuse crate::peripherals::asr33::{self as teletype, KeyKind, Mode as TtyMode, PrintEvent, Teletype};\nuse self::ui::front_panel::{\n    SwitchAlphaMode, SwitchPosition, SwitchSpriteId, CONTROL_SWITCHES, SENSE_SWITCHES,\n};\nuse self::ui::terminal::TerminalSpeed;",
)
text = replace_once(text, "fn main() -> eframe::Result {", "pub fn run() -> eframe::Result {", "app run entry")

include_tail = '''include!("front_panel.rs");
// Keep the optional black-background cleanup path exercised so the enum variant
// remains part of the supported switch-asset pipeline without triggering dead-code warnings.
const _: SwitchAlphaMode = SwitchAlphaMode::RemoveBlack;
include!("teletype_controller.rs");
include!("teletype_renderer.rs");
include!("teletype_io.rs");
include!("terminal.rs");
include!("application_loop.rs");'''
text = replace_once(
    text,
    include_tail,
    "// Keep the optional black-background cleanup path exercised so the enum variant\n// remains part of the supported switch-asset pipeline without triggering dead-code warnings.\nconst _: SwitchAlphaMode = SwitchAlphaMode::RemoveBlack;",
    "include tail",
)
write(app_mod, text)

write(
    SRC / "main.rs",
    "fn main() -> eframe::Result {\n    rustair::app::run()\n}\n",
)
write(
    SRC / "lib.rs",
    "pub mod app;\npub mod audio;\npub mod cpu8080;\npub mod io;\npub mod machine;\npub mod peripherals;\n\n// Compatibility re-export for the original public module path.\npub use peripherals::asr33 as teletype;\n",
)

# ---------------------------------------------------------------------------
# 3. Expose only the few UI-local definitions needed by app/mod.rs.
# ---------------------------------------------------------------------------
text = read(front_panel)
for old, new, label in [
    ("enum SwitchPosition {", "pub(in crate::app) enum SwitchPosition {", "SwitchPosition visibility"),
    ("enum SwitchSpriteId {", "pub(in crate::app) enum SwitchSpriteId {", "SwitchSpriteId visibility"),
    ("enum SwitchAlphaMode {", "pub(in crate::app) enum SwitchAlphaMode {", "SwitchAlphaMode visibility"),
    ("struct SwitchSpriteAsset {", "pub(in crate::app) struct SwitchSpriteAsset {", "SwitchSpriteAsset visibility"),
    ("struct SwitchPoseConfig {", "pub(in crate::app) struct SwitchPoseConfig {", "SwitchPoseConfig visibility"),
    ("struct SwitchConfig {", "pub(in crate::app) struct SwitchConfig {", "SwitchConfig visibility"),
    ("    fn pose(&self, position: SwitchPosition) -> Option<SwitchPoseConfig> {", "    pub(in crate::app) fn pose(&self, position: SwitchPosition) -> Option<SwitchPoseConfig> {", "pose visibility"),
    ("    fn asset(self) -> SwitchSpriteAsset {", "    pub(in crate::app) fn asset(self) -> SwitchSpriteAsset {", "asset visibility"),
    ("const SENSE_SWITCHES: [SwitchConfig; 16] = [", "pub(in crate::app) const SENSE_SWITCHES: [SwitchConfig; 16] = [", "sense switches visibility"),
    ("const CONTROL_SWITCHES: [SwitchConfig; 9] = [", "pub(in crate::app) const CONTROL_SWITCHES: [SwitchConfig; 9] = [", "control switches visibility"),
    ("    fn set_altair_power(&mut self, on: bool) {", "    pub(in crate::app) fn set_altair_power(&mut self, on: bool) {", "power method visibility"),
    ("    fn draw_altair(&mut self, ui: &mut egui::Ui) {", "    pub(in crate::app) fn draw_altair(&mut self, ui: &mut egui::Ui) {", "draw_altair visibility"),
]:
    text = replace_once(text, old, new, label)

# The parent app needs a handful of fields for texture loading.
for field in ["path", "canvas_size", "crop_min", "crop_max", "pivot", "source_to_panel", "alpha_mode"]:
    text = text.replace(f"    {field}: ", f"    pub(in crate::app) {field}: ", 1)
text = text.replace("    sprite: SwitchSpriteId,", "    pub(in crate::app) sprite: SwitchSpriteId,", 1)
write(front_panel, text)

# ---------------------------------------------------------------------------
# 4. Make cross-module app methods visible within crate::app only.
# ---------------------------------------------------------------------------
text = read(terminal_ui)
for old, new, label in [
    ("enum TerminalSpeed {", "pub(in crate::app) enum TerminalSpeed {", "TerminalSpeed visibility"),
    ("    const ALL: [Self; 5] = [", "    pub(in crate::app) const ALL: [Self; 5] = [", "TerminalSpeed ALL visibility"),
    ("    fn label(self) -> &'static str {", "    pub(in crate::app) fn label(self) -> &'static str {", "TerminalSpeed label visibility"),
    ("    fn char_time(self) -> Duration {", "    pub(in crate::app) fn char_time(self) -> Duration {", "TerminalSpeed char_time visibility"),
    ("    fn terminal_receive_byte(&mut self, byte: u8) {", "    pub(in crate::app) fn terminal_receive_byte(&mut self, byte: u8) {", "terminal_receive_byte visibility"),
    ("    fn process_terminal_input(&mut self, ctx: &egui::Context) {", "    pub(in crate::app) fn process_terminal_input(&mut self, ctx: &egui::Context) {", "process_terminal_input visibility"),
    ("    fn show_terminal_viewport(&mut self, parent_ctx: &egui::Context) {", "    pub(in crate::app) fn show_terminal_viewport(&mut self, parent_ctx: &egui::Context) {", "show_terminal_viewport visibility"),
]:
    text = replace_once(text, old, new, label)
write(terminal_ui, text)

text = read(asr_controller)
for name in [
    "update_teletype_mechanics",
    "set_tty_mode",
    "process_tty_answerback",
    "process_tty_serial",
    "process_tty_keyboard",
    "press_tty_key",
    "release_tty_key",
]:
    needle = f"    fn {name}("
    if needle not in text:
        raise RuntimeError(f"missing ASR controller method: {name}")
    text = text.replace(needle, f"    pub(in crate::app) fn {name}(", 1)
write(asr_controller, text)

text = read(asr_window)
for name in ["update_paper_tape", "load_bundled_basic", "show_tty_viewport"]:
    needle = f"    fn {name}("
    if needle not in text:
        raise RuntimeError(f"missing ASR window method: {name}")
    text = text.replace(needle, f"    pub(in crate::app) fn {name}(", 1)
write(asr_window, text)

text = read(asr_renderer)
needle = "    fn draw_teletype(&mut self, ui: &mut egui::Ui)"
if needle not in text:
    raise RuntimeError("missing draw_teletype method")
text = text.replace(needle, "    pub(in crate::app) fn draw_teletype(&mut self, ui: &mut egui::Ui)", 1)
write(asr_renderer, text)

# terminal_serial is called by runtime; keep its existing pub(crate) visibility.

# One-shot migration scaffolding removes itself after a successful checkout run.
(ROOT / "scripts/refactor_modular.py").unlink()
(ROOT / ".github/workflows/refactor-modular.yml").unlink()

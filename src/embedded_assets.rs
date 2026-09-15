//! Compile-time registry for all runtime assets shipped with RusTair.
//!
//! Keeping these bytes in the executable makes release builds self-contained:
//! no `assets/` directory is required next to `rustair.exe` at runtime.

pub(crate) fn get(path: &str) -> Option<&'static [u8]> {
    Some(match path {
        "assets/4kbas32.bin" => include_bytes!("../assets/4kbas32.bin"),

        "assets/panels/white-pivot/panel.png" => {
            include_bytes!("../assets/panels/white-pivot/panel.png")
        }
        "assets/panels/white-pivot/switch_up.png" => {
            include_bytes!("../assets/panels/white-pivot/switch_up.png")
        }
        "assets/panels/white-pivot/switch_center.png" => {
            include_bytes!("../assets/panels/white-pivot/switch_center.png")
        }
        "assets/panels/white-pivot/switch_down.png" => {
            include_bytes!("../assets/panels/white-pivot/switch_down.png")
        }

        "assets/panels/altair-3d/runtime/Altair8800_1975.glb" => {
            include_bytes!("../assets/panels/altair-3d/runtime/Altair8800_1975.glb")
        }
        "assets/panels/altair-3d/runtime/bindings.json" => {
            include_bytes!("../assets/panels/altair-3d/runtime/bindings.json")
        }

        "assets/asr33_body_clean.png" => include_bytes!("../assets/asr33_body_clean.png"),
        "assets/asr33_key_up.png" => include_bytes!("../assets/asr33_key_up.png"),
        "assets/asr33_key_mid.png" => include_bytes!("../assets/asr33_key_mid.png"),
        "assets/as33_spacebar_up.png" => include_bytes!("../assets/as33_spacebar_up.png"),
        "assets/as33_spacebar_mid.png" => include_bytes!("../assets/as33_spacebar_mid.png"),
        "assets/asr33head.png" => include_bytes!("../assets/asr33head.png"),
        "assets/asrlinelocal.png" => include_bytes!("../assets/asrlinelocal.png"),
        "assets/asrlinelocalknob.png" => include_bytes!("../assets/asrlinelocalknob.png"),
        "assets/teletype.ttf" => include_bytes!("../assets/teletype.ttf"),

        "assets/bellpadded.mp3" => include_bytes!("../assets/bellpadded.mp3"),
        "assets/click.mp3" => include_bytes!("../assets/click.mp3"),
        "assets/crpadded.mp3" => include_bytes!("../assets/crpadded.mp3"),
        "assets/fan.mp3" => include_bytes!("../assets/fan.mp3"),
        "assets/powerbtn.mp3" => include_bytes!("../assets/powerbtn.mp3"),
        "assets/printcharpadded.mp3" => include_bytes!("../assets/printcharpadded.mp3"),
        "assets/up-hum4.mp3" => include_bytes!("../assets/up-hum4.mp3"),

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_runtime_assets_are_present() {
        for path in [
            "assets/4kbas32.bin",
            "assets/panels/white-pivot/panel.png",
            "assets/panels/white-pivot/switch_up.png",
            "assets/panels/white-pivot/switch_center.png",
            "assets/panels/white-pivot/switch_down.png",
            "assets/panels/altair-3d/runtime/Altair8800_1975.glb",
            "assets/panels/altair-3d/runtime/bindings.json",
            "assets/asr33_body_clean.png",
            "assets/asr33_key_up.png",
            "assets/asr33_key_mid.png",
            "assets/as33_spacebar_up.png",
            "assets/as33_spacebar_mid.png",
            "assets/asr33head.png",
            "assets/asrlinelocal.png",
            "assets/asrlinelocalknob.png",
            "assets/teletype.ttf",
            "assets/bellpadded.mp3",
            "assets/click.mp3",
            "assets/crpadded.mp3",
            "assets/fan.mp3",
            "assets/powerbtn.mp3",
            "assets/printcharpadded.mp3",
            "assets/up-hum4.mp3",
        ] {
            assert!(get(path).is_some(), "missing embedded asset: {path}");
        }
    }

    #[test]
    fn embedded_altair_3d_runtime_assets_have_expected_container_markers() {
        let glb = get("assets/panels/altair-3d/runtime/Altair8800_1975.glb").unwrap();
        assert!(glb.len() >= 12, "GLB header is truncated");
        assert_eq!(&glb[0..4], b"glTF", "Altair runtime model must be a binary glTF container");

        let bindings = get("assets/panels/altair-3d/runtime/bindings.json").unwrap();
        let bindings = std::str::from_utf8(bindings).expect("Altair bindings must be UTF-8 JSON");
        assert!(
            bindings.contains("\"schema\": \"altair8800.frontpanel.v1\""),
            "Altair bindings schema changed unexpectedly"
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Character(&'static str),
    Escape,
    LineFeed,
    CarriageReturn,
    Delete,
    Repeat,
    Break,
    HereIs,
    Space,
    Control,
    Shift,
}

#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub kind: KeyKind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Key {
    pub const fn new(kind: KeyKind, x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { kind, x, y, w, h }
    }

    pub const fn centered(kind: KeyKind, cx: f32, cy: f32, w: f32, h: f32) -> Self {
        Self {
            kind,
            x: cx - w * 0.5,
            y: cy - h * 0.5,
            w,
            h,
        }
    }

    pub fn center(self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x - 20.0
            && x <= self.x + self.w + 20.0
            && y >= self.y - 20.0
            && y <= self.y + self.h + 20.0
    }
}

const W: f32 = 114.0;
const H: f32 = 97.0;
const MOD_W: f32 = 150.0;
const MOD_H: f32 = 120.0;

/// Socket-centre calibration for `assets/asr33_body_clean.png` (3008×2983).
pub const KEYS: &[Key] = &[
    Key::centered(KeyKind::Character("1!"), 621.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("2\""), 778.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("3#"), 931.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("4$"), 1087.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("5%"), 1257.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("6&"), 1411.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("7'"), 1567.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("8("), 1727.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("9)"), 1884.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("0"), 2044.0, 2023.0, W, H),
    Key::centered(KeyKind::Character(":*"), 2202.0, 2023.0, W, H),
    Key::centered(KeyKind::Character("-="), 2358.0, 2023.0, W, H),
    Key::centered(KeyKind::HereIs, 2517.0, 2023.0, W, H),

    Key::centered(KeyKind::Escape, 546.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("Q"), 705.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("W"), 857.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("E"), 1019.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("R"), 1176.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("T"), 1338.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("Y"), 1494.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("U"), 1654.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("I"), 1813.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("O_"), 1971.0, 2190.0, W, H),
    Key::centered(KeyKind::Character("P@"), 2131.0, 2190.0, W, H),
    Key::centered(KeyKind::LineFeed, 2287.0, 2190.0, W, H),
    Key::centered(KeyKind::CarriageReturn, 2443.0, 2190.0, W, H),

    Key::centered(KeyKind::Control, 573.0, 2358.0, MOD_W, MOD_H),
    Key::centered(KeyKind::Character("A"), 732.0, 2358.0, W, H),
    Key::centered(KeyKind::Character("S"), 895.0, 2358.0, W, H),
    Key::centered(KeyKind::Character("D"), 1054.0, 2358.0, W, H),
    Key::centered(KeyKind::Character("F"), 1212.0, 2358.0, W, H),
    Key::centered(KeyKind::Character("G"), 1378.0, 2358.0, W, H),
    Key::centered(KeyKind::Character("H"), 1540.0, 2358.0, W, H),
    Key::centered(KeyKind::Character("J"), 1704.0, 2358.0, W, H),
    Key::centered(KeyKind::Character("K["), 1865.0, 2358.0, W, H),
    Key::centered(KeyKind::Character("L\\"), 2026.0, 2358.0, W, H),
    Key::centered(KeyKind::Character(";+"), 2188.0, 2358.0, W, H),
    Key::centered(KeyKind::Delete, 2350.0, 2358.0, W, H),
    Key::centered(KeyKind::Repeat, 2518.0, 2358.0, W, H),
    Key::centered(KeyKind::Break, 2683.0, 2358.0, W, H),

    Key::centered(KeyKind::Shift, 643.0, 2530.0, MOD_W, MOD_H),
    Key::centered(KeyKind::Character("Z"), 803.0, 2530.0, W, H),
    Key::centered(KeyKind::Character("X"), 969.0, 2530.0, W, H),
    Key::centered(KeyKind::Character("C"), 1132.0, 2530.0, W, H),
    Key::centered(KeyKind::Character("V"), 1295.0, 2530.0, W, H),
    Key::centered(KeyKind::Character("B"), 1457.0, 2530.0, W, H),
    Key::centered(KeyKind::Character("N^"), 1620.0, 2530.0, W, H),
    Key::centered(KeyKind::Character("M]"), 1782.0, 2530.0, W, H),
    Key::centered(KeyKind::Character(",<"), 1945.0, 2530.0, W, H),
    Key::centered(KeyKind::Character(".>"), 2109.0, 2530.0, W, H),
    Key::centered(KeyKind::Character("/?"), 2267.0, 2530.0, W, H),
    Key::centered(KeyKind::Shift, 2429.0, 2530.0, MOD_W, MOD_H),

    Key::new(KeyKind::Space, 1220.0, 2652.0, 671.0, 120.0),
];

pub fn hit_test(x: f32, y: f32) -> Option<&'static Key> {
    KEYS.iter().find(|key| key.contains(x, y))
}

pub fn key_to_byte(kind: KeyKind, shifted: bool, control: bool) -> Option<u8> {
    let mut ch = match kind {
        KeyKind::Character(chars) => {
            let mut iter = chars.chars();
            let first = iter.next()?;
            if shifted {
                iter.next().unwrap_or(first)
            } else {
                first
            }
        }
        KeyKind::Escape => return Some(0x1b),
        KeyKind::LineFeed => return Some(b'\n'),
        KeyKind::CarriageReturn => return Some(b'\r'),
        KeyKind::Delete => return Some(0x7f),
        KeyKind::Break => return Some(0x00),
        KeyKind::Space => return Some(b' '),
        KeyKind::Repeat | KeyKind::HereIs | KeyKind::Control | KeyKind::Shift => return None,
    };

    ch = ch.to_ascii_uppercase();
    let mut byte = ch as u8;
    if control && byte.is_ascii_uppercase() {
        byte -= 64;
    }
    Some(byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrated_hitbox_finds_a_key() {
        let k = hit_test(732.0, 2358.0).unwrap();
        assert_eq!(key_to_byte(k.kind, false, false), Some(b'A'));
    }

    #[test]
    fn here_is_is_physical_but_not_a_single_ascii_key() {
        let k = hit_test(2517.0, 2023.0).unwrap();
        assert_eq!(k.kind, KeyKind::HereIs);
        assert_eq!(key_to_byte(k.kind, false, false), None);
    }

    #[test]
    fn shift_and_control_match_asr33() {
        assert_eq!(key_to_byte(KeyKind::Character("K["), true, false), Some(b'['));
        assert_eq!(key_to_byte(KeyKind::Character("A"), false, true), Some(1));
    }

    #[test]
    fn break_maps_to_nul_in_byte_level_serial_model() {
        assert_eq!(key_to_byte(KeyKind::Break, false, false), Some(0));
        assert_eq!(key_to_byte(KeyKind::Repeat, false, false), None);
    }
}

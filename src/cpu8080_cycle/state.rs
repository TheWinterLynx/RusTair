/// Programmer-visible Intel 8080 register state.
///
/// The shape intentionally mirrors the existing fast core so the future
/// differential harness can compare both engines directly after each
/// instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Registers {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub f: u8,
    pub sp: u16,
    pub pc: u16,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            // Intel 8080 PSW bit 1 is conventionally held high.
            f: 0x02,
            sp: 0,
            pc: 0,
        }
    }
}

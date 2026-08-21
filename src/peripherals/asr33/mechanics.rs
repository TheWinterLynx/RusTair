use std::time::Instant;

/// Time-based mechanical state of the ASR-33 printer mechanism.
///
/// This type deliberately contains no egui or Altair-machine dependencies. The
/// application controller advances it while the renderer only observes it.
#[derive(Default)]
pub(crate) struct MechanicsState {
    pub(crate) print_head_raise_until: Option<Instant>,
    pub(crate) print_head_impact_at: Option<Instant>,
    pub(crate) print_head_auto_return_at: Option<Instant>,
    pub(crate) print_head_glyph: u8,
    pub(crate) print_head_carriage_return_until: Option<Instant>,
    pub(crate) paper_feed_until: Option<Instant>,
}

impl MechanicsState {
    pub(crate) fn new() -> Self {
        Self {
            print_head_glyph: b' ',
            ..Self::default()
        }
    }

    pub(crate) fn clear_motion(&mut self) {
        self.print_head_raise_until = None;
        self.print_head_impact_at = None;
        self.print_head_auto_return_at = None;
        self.print_head_carriage_return_until = None;
        self.paper_feed_until = None;
    }

    pub(crate) fn printing_active(&self) -> bool {
        self.print_head_impact_at.is_some()
            || self.print_head_auto_return_at.is_some()
            || self.print_head_raise_until.is_some()
            || self.print_head_carriage_return_until.is_some()
            || self.paper_feed_until.is_some()
    }
}

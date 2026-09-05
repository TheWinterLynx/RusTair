//! Cycle-accurate Intel 8080 backend dispatcher.
//!
//! `partial_impl.rs` is the existing edge-by-edge electrical oracle. `full.rs`
//! adds a MAME-style whole-instruction executor only for chassis/instructions
//! whose intervening electrical states are proven not to affect installed
//! hardware. Both operate on the same CPU state, Altair chassis and S-100 cards.

mod full;

include!("cycle/partial_impl.rs");

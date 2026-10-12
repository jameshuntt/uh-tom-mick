#![forbid(unsafe_code)]
//! Public facade for Uh Tom Mick atomic state capabilities.
//!
//! This is the public form of the recovered V4 Fix2 atomic system: packed
//! admission/shutdown state, checked counters, atomic flag sets, generated
//! admission gates, async dispatch wrappers, generated counters/flags, and the
//! original hybrid/Loom/AF_UNIX test model.

extern crate self as uh_tom_mick;

pub use uh_tom_mick_core::*;
pub use uh_tom_mick_derive::{
    atomic_admission, atomic_counter, atomic_daemon, atomic_flags, uh_tom_mick,
};

pub use big_bad_bool;
pub use big_bad_bool::bitpack;

pub use uh_tom_mick_core as core;
pub use uh_tom_mick_derive as derive;

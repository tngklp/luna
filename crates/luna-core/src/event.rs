//! Event bus re-exported from the crate root.
//!
//! Kept as a module for path stability (`luna_core::event::*`) while the
//! implementation lives in `lib.rs`.

pub use crate::{EventBus, EventKind, LunaEvent};

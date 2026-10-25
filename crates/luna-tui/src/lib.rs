//! Luna's terminal interface, as a library.
//!
//! The binary is a thin wrapper around this so the rendering and the command
//! palette can be tested without a terminal.

pub mod app;
pub mod commands;
pub mod headless;
pub mod highlight;
pub mod theme;
pub mod ui;

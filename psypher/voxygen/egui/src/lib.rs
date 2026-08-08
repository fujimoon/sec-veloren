//! A real, pty-backed terminal widget for egui.
//!
//! Extracted out of `veloren-voxygen-egui` so it has no dependency on, and no
//! knowledge of, anything Veloren-specific (`Client`, `comp`, ECS, ...). The only
//! things it knows about are `egui` and `alacritty_terminal`. Anything that wants to
//! embed a terminal just needs [`TerminalState`].

mod terminal;

pub use terminal::TerminalState;

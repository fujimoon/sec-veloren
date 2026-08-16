//! Always-on structured trace logger for debugging server-generated geometry
//! (currently: the OS Dungeon feature) from *outside* the running game.
//!
//! The point of this crate: server-side bugs in generated voxel geometry were
//! previously only diagnosable by asking a human to walk around in-game and
//! send back screenshots — slow, and lossy (lighting/angle/perspective made
//! it hard to tell "open doorway" from "solid wall" at a glance). This crate
//! instead appends structured JSON-lines events to a plain file on disk,
//! readable directly (no game client needed) by whoever — human or agent —
//! is debugging the feature.

mod tracer;

pub use tracer::{clear, log};

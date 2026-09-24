//! Optional, procedurally synthesised sound effects (off by default).
//!
//! Every cue is generated in code from original parameters; no recorded or
//! third-party audio assets are used (see `docs/SECURITY_AND_PROVENANCE.md`).

mod player;
mod synth;

pub use player::{SoundPlayer, SoundSettings};
pub use synth::{Cue, SAMPLE_RATE, render_cue};

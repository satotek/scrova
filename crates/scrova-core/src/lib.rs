//! Platform-independent core of Scrova.
//!
//! Native observers turn player state into [`PlaybackEvent`]s; the
//! [`ScrobbleEngine`] turns those into Now Playing updates and scrobbles.

mod engine;
mod model;
mod recovery;

pub use engine::{EngineConfig, ScrobbleEngine, scrobble_threshold};
pub use model::*;
pub use recovery::{
    LibraryState, MAX_RECOVERY_SCROBBLES, RECOVERY_MAX_AGE, RecoveryConfig, RecoveryPlan,
    plan_recovery,
};

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

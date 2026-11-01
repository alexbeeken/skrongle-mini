//! Skrongle AI on the pages: the provider for one call on the server's key,
//! and what a learner can do when it does not cover a job (`OutOfAi`). The
//! metering itself is `metering::Meter`; the calls go through the same `ai`
//! client as a learner's own key.

use crate::ai;
use crate::AppState;

/// A provider on the server's key for `purpose`; `None` when the server has
/// no key of its own.
pub fn hosted_provider(state: &AppState, purpose: &str) -> Option<ai::Provider> {
    state.meter.provider(purpose)
}

/// The way on when Skrongle AI does not cover a job or a turn: the
/// learner's own key (`ai_out.html`).
#[derive(Clone, Debug, Default)]
pub struct OutOfAi;

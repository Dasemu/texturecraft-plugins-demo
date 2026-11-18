//! A reviewed bundle waiting for its install.

use serde_json::Value;

use super::Candidate;

/// A bundle checked and shown for review (`plugin.install {review: true}`), kept so the install
/// that follows doesn't download it again.
pub(crate) struct Pending {
    pub sha256: String,
    /// The `plugin.install` parameters that named it.
    pub request: Value,
    pub candidate: Candidate,
}

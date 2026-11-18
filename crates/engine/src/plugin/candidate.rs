//! A bundle obtained for installing.

use serde_json::Value;

/// A bundle obtained for installing: its bytes, where it came from and, for a repository, whether
/// a catalog in use lists it.
pub(crate) struct Candidate {
    pub bytes: Vec<u8>,
    /// `{type: file, path}`, `{type: catalog, catalog, package}` or `{type: github, repo, tag, asset}`.
    pub source: Value,
    /// `Some(true)` from a catalog; for a repository, whether a catalog in use lists it.
    pub in_catalog: Option<bool>,
}

//! An installed plug-in.

use std::sync::Arc;

use craft_plugin::Manifest;
use serde_json::{Value, json};

/// An installed plug-in.
#[derive(Clone, Debug)]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    /// The SHA-256 of its bundle (lower-case hex).
    pub sha256: String,
    /// Where it came from: `{type: file, path}`, `{type: catalog, catalog, package}` or
    /// `{type: github, repo, tag, asset}` (craft-plugin's `store::Source`).
    pub source: Value,
    /// Kept in the user's `Plug-ins` folder; otherwise for this session only (the CLI, MCP).
    pub stored: bool,
    pub(crate) wasm: Arc<Vec<u8>>,
}

impl InstalledPlugin {
    /// What `plugin.list` says about it.
    pub fn describe(&self) -> Value {
        let mut v = serde_json::to_value(&self.manifest).unwrap_or_else(|_| json!({"id": self.manifest.id}));
        v["kind"] = json!(self.manifest.kind);
        v["sha256"] = json!(self.sha256);
        v["source"] = self.source.clone();
        v["stored"] = json!(self.stored);
        v
    }
}

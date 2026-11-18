//! A plug-in bundle checked and waiting for the person's choice.

use serde_json::Value;

/// The install confirmation's content: the `plugin.install` parameters that named the bundle, and
/// what `plugin.install {review: true}` said about it.
#[derive(Clone, Debug)]
pub struct PluginReview {
    pub request: Value,
    pub info: Value,
}

impl PluginReview {
    /// Where the plug-in comes from, in a line.
    pub fn source_text(&self) -> String {
        let source = &self.info["source"];
        let t = |k: &str| source[k].as_str().unwrap_or_default();
        match t("type") {
            "github" => format!("github.com/{} {} ({})", t("repo"), t("tag"), t("asset")),
            "catalog" => format!("{} · {}", t("package"), t("catalog")),
            _ => t("path").to_string(),
        }
    }
}

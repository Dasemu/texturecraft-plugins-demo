//! What the app keeps of plug-ins in its settings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The plug-ins' part of the app's settings: each plug-in's own settings and the catalogs added.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginPrefs {
    /// Each plug-in's settings, by plug-in id (`plugin::MAX_SETTINGS_BYTES` each).
    pub settings: BTreeMap<String, Map<String, Value>>,
    /// Plug-in catalogs added with `plugin.catalog.add` (`index.json` URLs).
    pub catalogs: Vec<String>,
}

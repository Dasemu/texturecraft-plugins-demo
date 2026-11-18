//! The session's plug-ins.

use std::sync::Mutex;

use craft_plugin::fetch::Fetch;
use serde_json::Value;

use super::{InstalledPlugin, Pending, PluginPrefs};

/// The session's plug-ins: the installed ones, where they are kept, their settings and catalogs,
/// and the HTTPS client installs download with. It outlives the project ([`crate::Session::switch_to`]).
#[derive(Default)]
pub struct Runtime {
    /// The user's `Plug-ins` folder (the app sets it; the CLI and MCP install for the session).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) store: Option<craft_plugin::store::Store>,
    plugins: Vec<InstalledPlugin>,
    /// Plug-in folders that couldn't be read, or plug-ins this version can't run.
    problems: Vec<String>,
    /// HTTPS for catalogs and GitHub releases (the app sets its own client; without one, only
    /// files and `file://` catalogs install).
    pub(crate) fetch: Mutex<Option<Box<dyn Fetch + Send>>>,
    pub(crate) pending: Option<Pending>,
    /// The plug-ins' settings and catalogs, kept with the app's settings.
    pub prefs: PluginPrefs,
    /// File access is confined (automation with roots): no `path` installs and no `file://` catalogs.
    pub confined: bool,
    /// The last run (plug-in id, command id, params).
    pub last: Option<(String, String, Value)>,
}

impl Runtime {
    /// Keep plug-ins in `<settings_dir>/Plug-ins` and read the ones installed there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_store(&mut self, settings_dir: &std::path::Path) {
        self.store = Some(craft_plugin::store::Store::new(craft_plugin::store::plugins_dir(settings_dir)));
        self.reload();
    }

    /// Read the store again (the session's own plug-ins stay).
    #[cfg(not(target_arch = "wasm32"))]
    fn reload(&mut self) {
        let Some(store) = &self.store else { return };
        // The store keeps whatever was installed; only plug-ins for TextureCraft are offered
        let listing = store.list_for(&super::APP);
        self.plugins.retain(|p| !p.stored);
        self.problems = listing.problems.into_iter().map(|p| format!("{}: {}", p.dir.display(), p.error)).collect();
        self.problems.extend(listing.not_runnable.into_iter().map(|(i, why)| format!("{}: {why}", i.manifest.name)));
        for (i, _) in listing.runnable {
            let source = serde_json::to_value(&i.record.source).unwrap_or(Value::Null);
            let plugin = InstalledPlugin { manifest: i.manifest, sha256: i.record.sha256, source, stored: true, wasm: std::sync::Arc::new(i.bundle.wasm) };
            self.keep(plugin);
        }
    }

    /// Download catalogs and releases with `client`.
    pub fn set_fetch(&mut self, client: Option<Box<dyn Fetch + Send>>) {
        *self.fetch.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner) = client;
    }

    /// The installed plug-ins.
    pub fn list(&self) -> &[InstalledPlugin] {
        &self.plugins
    }

    /// Why plug-in folders couldn't be used.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// Plug-in `id` (ignoring case, as the store does).
    pub fn get(&self, id: &str) -> Option<&InstalledPlugin> {
        self.plugins.iter().find(|p| p.manifest.id.eq_ignore_ascii_case(id))
    }

    /// Whether plug-ins are kept in the user's folder (else for this session only).
    pub fn has_store(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.store.is_some();
        #[cfg(target_arch = "wasm32")]
        false
    }

    /// Put `plugin` in the list, in place of one with the same id.
    pub(crate) fn keep(&mut self, plugin: InstalledPlugin) -> InstalledPlugin {
        match self.plugins.iter_mut().find(|p| p.manifest.id.eq_ignore_ascii_case(&plugin.manifest.id)) {
            Some(slot) => *slot = plugin.clone(),
            None => self.plugins.push(plugin.clone()),
        }
        plugin
    }

    /// Drop plug-in `id` from the list, with its settings.
    pub(crate) fn forget(&mut self, id: &str) {
        self.plugins.retain(|p| !p.manifest.id.eq_ignore_ascii_case(id));
        self.prefs.settings.retain(|k, _| !k.eq_ignore_ascii_case(id));
        if self.last.as_ref().is_some_and(|(last, ..)| last.eq_ignore_ascii_case(id)) {
            self.last = None;
        }
    }
}

//! The sandbox's view of the session.

use craft_plugin::{Access, Manifest};
use serde_json::{Value, json};

use super::{MAX_NOTE_CHARS, MAX_NOTES, refusal, restore_document, selected_ids, set_setting};
use crate::Session;

/// The sandbox's view of the session: every call is checked by [`refusal`], and one that, from a
/// read-only plug-in, changes the project is put back and refused.
pub(super) struct EngineHost<'s> {
    s: &'s mut Session,
    manifest: &'s Manifest,
    notes: Vec<String>,
}

impl<'s> EngineHost<'s> {
    pub(super) fn new(s: &'s mut Session, manifest: &'s Manifest) -> Self {
        EngineHost { s, manifest, notes: Vec::new() }
    }

    /// What the run asked to show the user.
    pub(super) fn into_notes(self) -> Vec<String> {
        self.notes
    }

    /// Run `command` for a read-only plug-in: a call that changed the project (an undo step, a
    /// change marked unsaved, a stroke, box fill or path left open) is put back and refused.
    fn call_reading(&mut self, command: &str, params: &Value) -> Result<String, String> {
        let s = &mut *self.s;
        let before = s.doc.clone();
        let steps = s.undo.len();
        let open = (s.stroke.is_some(), s.polygon_fill_drag.is_some(), s.path_draft.is_some());
        // Every change to the project marks it unsaved, so the flag tells whether this call made one
        let modified = std::mem::replace(&mut s.modified, false);
        let r = crate::commands::execute(s, command, params);
        let changed = s.modified || s.undo.len() != steps || (s.stroke.is_some(), s.polygon_fill_drag.is_some(), s.path_draft.is_some()) != open;
        s.modified |= modified;
        if changed {
            s.undo.truncate(steps);
            restore_document(s, before);
            s.modified = modified;
            return Err(craft_plugin::check_read_only(self.manifest, command, true).err().unwrap_or_else(|| format!("`{command}` changes the project")));
        }
        r.map(|v| v.to_string()).map_err(|e| e.to_string())
    }
}

impl craft_plugin::Host for EngineHost<'_> {
    fn call(&mut self, command: &str, params: &str) -> Result<String, String> {
        let params: Value = serde_json::from_str(params).map_err(|e| format!("the params for `{command}` aren't JSON: {e}"))?;
        if let Some(why) = refusal(self.manifest, command, &params) {
            return Err(why);
        }
        if self.manifest.permissions.access != Access::Write {
            return self.call_reading(command, &params);
        }
        crate::commands::execute(self.s, command, &params).map(|v| v.to_string()).map_err(|e| e.to_string())
    }

    fn selection(&mut self) -> String {
        json!(selected_ids(self.s)).to_string()
    }

    fn notify(&mut self, message: &str) {
        if self.notes.len() < MAX_NOTES {
            self.notes.push(message.chars().take(MAX_NOTE_CHARS).collect());
        }
    }

    fn log(&mut self, line: &str) {
        self.s.log.push(format!("plug-in {}: {line}", self.manifest.id));
    }

    fn setting(&mut self, key: &str) -> Option<String> {
        Some(self.s.plugins.prefs.settings.get(&self.manifest.id)?.get(key)?.to_string())
    }

    fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        set_setting(&mut self.s.plugins.prefs.settings, &self.manifest.id, key, value)
    }
}

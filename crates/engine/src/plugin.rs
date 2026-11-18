//! Plug-ins: WebAssembly modules that act on the project through the same commands as the menus,
//! the CLI, the control channel and MCP, inside the sandbox of `craft-plugin` (RFC-0046). This
//! module is TextureCraft's side of it: which commands a plug-in may call ([`policy`]), one undo
//! step per run (rolled back when the run fails), read-only access checked after every call, the
//! plug-ins' settings (kept with the app's settings) and the installed plug-ins (the user's
//! `Plug-ins` folder, or this session only). The `plugin.*` commands are in `plugin_cmds`. See
//! `docs/plugins.md`.

mod candidate;
mod engine_host;
mod installed_plugin;
mod no_network;
mod pending;
mod plugin_prefs;
mod runtime;

use std::sync::{Arc, OnceLock};

use base64::Engine as _;
pub use craft_plugin;
use craft_plugin::fetch::{self, Fetch};
use craft_plugin::{API_VERSION, AppInfo, Instance, Limits, Manifest, Policy, RunOutput, UiChange, bundle, catalog, github};
use serde_json::{Map, Value, json};

pub(crate) use candidate::Candidate;
use engine_host::EngineHost;
pub use installed_plugin::InstalledPlugin;
use no_network::NoNetwork;
use pending::Pending;
pub use plugin_prefs::PluginPrefs;
pub use runtime::Runtime;

use crate::commands::bad;
use crate::{EngineError, Result, Session};

/// TextureCraft as manifests name it: the command plug-ins of every shared ABI version, and only
/// manifests that name `texturecraft` in `apps`.
pub const APP: AppInfo<'static> =
    AppInfo { id: "texturecraft", version: env!("CARGO_PKG_VERSION"), api: 1..=API_VERSION, kinds: &["command"], accept_legacy: false, kind_api: &[] };

/// What one plug-in may keep in its settings (keys and JSON values, in bytes), and its longest key.
pub const MAX_SETTINGS_BYTES: usize = 64 * 1024;
pub const MAX_SETTING_KEY: usize = 128;
/// Catalogs a person may add.
pub const MAX_CATALOGS: usize = 32;
/// Messages a run may show (`notify`), and their length in characters.
const MAX_NOTES: usize = 20;
const MAX_NOTE_CHARS: usize = 1000;
/// Least instruction budget `plugin.run {fuel}` may ask for.
const MIN_FUEL: u64 = 1000;

/// TextureCraft's commands no plug-in may call, on top of [`craft_plugin::ALWAYS_DENIED`]
/// (plug-ins, Undo and Redo, `file.*`, `app.*`, `pref.*`, `window.*`). A plug-in gets no path, no
/// file outside the project, no other project, none of the user's own presets and templates and no
/// undo history; it works on the open project only.
const DENIED: &[&str] = &[
    // The undo history, and other projects (a new or sample project replaces the open one).
    "edit.history",
    "project.new",
    "project.sample",
    // The user's own templates and presets, kept outside the project.
    "project.templates",
    "project.template.save",
    "export.preset.save",
    "export.preset.remove",
    "brush.preset.save",
    "brush.preset.load",
    // Files beside the project: smart material files, images, mesh maps and meshes on disk.
    "material.save",
    "material.load",
    "resource.import",
    "textureSet.meshMap.import",
    "bake.highPoly.add",
    "bake.cage.set",
    // MaterialCraft links read and write source files and run MaterialCraft itself.
    "material.link.*",
    // Bakes are long jobs the app runs and shows; their results don't go through the undo history.
    "bake.run",
    "bake.preview",
    // The layer and effect clipboard belongs to the session, and may hold another project.
    "layer.copy",
    "layer.cut",
    "layer.paste",
    "layer.pasteInstance",
    "effect.copy",
    "effect.cut",
    "effect.paste",
];

/// The rules every plug-in run follows: [`craft_plugin::ALWAYS_DENIED`] and [`DENIED`].
pub fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| Policy::new(DENIED, &[]))
}

/// Why plug-in `m` may not call `command` with `params`, before it runs: a refused command, one
/// outside its manifest's `permissions.commands`, or parameters that name a file.
pub fn refusal(m: &Manifest, command: &str, params: &Value) -> Option<String> {
    if let Err(why) = craft_plugin::check_call(m, policy(), command) {
        return Some(why);
    }
    if names_a_file(params, 0) {
        return Some(format!("plug-ins can't name files (`{command}` was given a `path`, `dir`, `folder` or `files`): use resources already in the project"));
    }
    None
}

/// Whether `params` hold a `path`, `paths`, `dir`, `folder` or `files` key at any depth:
/// TextureCraft's names for files and folders on disk (`resource.import`, `bake.skew.set`,
/// `textureSet.meshMap.import`…).
fn names_a_file(params: &Value, depth: usize) -> bool {
    // serde_json refuses nesting past 128, so nothing deeper reaches here
    depth < 128
        && match params {
            Value::Object(m) => m.iter().any(|(k, v)| matches!(k.as_str(), "path" | "paths" | "dir" | "folder" | "files") || names_a_file(v, depth + 1)),
            Value::Array(a) => a.iter().any(|v| names_a_file(v, depth + 1)),
            _ => false,
        }
}

/// Run `f` with the session's HTTPS client (or [`NoNetwork`]).
pub(crate) fn with_fetch<T>(s: &Session, f: impl FnOnce(&mut dyn Fetch) -> T) -> T {
    let mut client = s.plugins.fetch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match client.as_deref_mut() {
        Some(http) => f(http),
        None => f(&mut NoNetwork),
    }
}

/// A plug-in error as the engine reports it.
fn failed(name: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("{name}: {e}"))
}

/// Store (`Some`) or forget (`None`) plug-in `id`'s setting `key` in `all`, within
/// [`MAX_SETTINGS_BYTES`] and [`MAX_SETTING_KEY`].
fn set_setting(all: &mut std::collections::BTreeMap<String, Map<String, Value>>, id: &str, key: &str, value: Option<&str>) -> std::result::Result<(), String> {
    let Some(text) = value else {
        if let Some(mine) = all.get_mut(id) {
            mine.remove(key);
            if mine.is_empty() {
                all.remove(id);
            }
        }
        return Ok(());
    };

    if key.is_empty() || key.chars().count() > MAX_SETTING_KEY || key.chars().any(char::is_control) {
        return Err(format!("a setting's key must be 1–{MAX_SETTING_KEY} characters without control characters"));
    }
    let too_big = || format!("a plug-in's settings are limited to {} KB", MAX_SETTINGS_BYTES / 1024);
    if key.len() + text.len() > MAX_SETTINGS_BYTES {
        return Err(too_big());
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("a setting must be JSON: {e}"))?;
    let others: usize = all.get(id).map_or(0, |mine| mine.iter().filter(|(k, _)| k.as_str() != key).map(|(k, v)| k.len() + v.to_string().len()).sum());
    if others + key.len() + v.to_string().len() > MAX_SETTINGS_BYTES {
        return Err(too_big());
    }
    all.entry(id.to_string()).or_default().insert(key.to_string(), v);
    Ok(())
}

// ---------- running ----------

/// What a run may change besides the document and must put back when it fails: the selected layer
/// per texture set, the active set and the stack target.
type Selection = (std::collections::BTreeMap<usize, crate::core::LayerId>, usize, crate::StackTarget);

fn selection_of(s: &Session) -> Selection {
    (s.selected.clone(), s.active_set, s.target)
}

/// The selected layer ids, as a plug-in sees the selection.
fn selected_ids(s: &Session) -> Vec<u64> {
    s.selected_layer().map(|id| vec![id.0]).unwrap_or_default()
}

/// End what a gesture left open, as before an undo: a box fill, a stroke, a path being placed
fn settle_gestures(s: &mut Session) -> Result<()> {
    s.end_polygon_fill_box();
    if s.stroke_active() {
        s.end_stroke()?;
    }
    s.finish_path()
}

/// Drop what a gesture left open without applying it (the document is restored after)
fn drop_gestures(s: &mut Session) {
    s.cancel_stroke();
    s.polygon_fill_drag = None;
    s.path_draft = None;
    s.path_steps = Default::default();
}

/// Put `before` back as the document, with its caches and decoded images
fn restore_document(s: &mut Session, before: crate::Document) {
    drop_gestures(s);
    let current = std::mem::replace(&mut s.doc, before);
    s.invalidate_changed(&current);
    s.revision = s.revision.wrapping_add(1);
    s.sync_images();
}

/// Run plug-in `id`'s `command` (default: its first) with `params` as one undo step named after
/// the plug-in. A run that fails (a trap, an exhausted budget, a refusal it passes on, a non-zero
/// status) leaves the project, the selection and the undo history as they were.
pub fn run(s: &mut Session, id: &str, command: Option<&str>, params: &Value, fuel: Option<u64>) -> Result<Value> {
    let cmd = "plugin.run";
    let p = s.plugins.get(id).cloned().ok_or_else(|| bad(cmd, format!("no plug-in `{id}` is installed (see plugin.list)")))?;
    let m = &p.manifest;
    let api = m.target(&APP).map_err(|e| bad(cmd, format!("{} can't run here: {e}", m.name)))?.api();
    let c = m.command(command).ok_or_else(|| bad(cmd, format!("{} has no command `{}`", m.name, command.unwrap_or_default())))?;
    let params = c.resolve_params(params).map_err(|e| bad(cmd, e))?;
    let mut limits = Limits::default();
    if let Some(f) = fuel {
        limits.fuel = f.clamp(MIN_FUEL, limits.fuel);
    }
    let mut instance = Instance::new(&p.wasm, &limits).map_err(|e| failed(&m.name, e))?;

    // What a gesture left open becomes its own step first, as before an undo
    settle_gestures(s)?;
    let input = json!({"api": api, "command": c.id, "params": params, "selection": selected_ids(s)}).to_string();
    s.plugins.last = Some((m.id.clone(), c.id.clone(), params));
    let label = if m.commands.len() > 1 { format!("{}: {}", m.name, c.label.trim_end_matches(['…', '.'])) } else { m.name.clone() };

    // The run's commands record undo steps of their own in an empty history; they collapse into one below
    let before = s.doc.clone();
    let selection = selection_of(s);
    let modified = s.modified;
    let undo = std::mem::take(&mut s.undo);
    let redo = std::mem::take(&mut s.redo);
    let mut host = EngineHost::new(s, m);
    let ran = instance.call(&input, &mut host);
    let notes = host.into_notes();
    let ran: std::result::Result<RunOutput, String> = match ran {
        Ok(out) => settle_gestures(s).map(|()| out).map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let steps = std::mem::replace(&mut s.undo, undo);
    let out = match ran {
        Ok(out) => out,
        Err(e) => {
            restore_document(s, before);
            (s.selected, s.active_set, s.target) = selection;
            s.redo = redo;
            s.modified = modified;
            return Err(failed(&m.name, e));
        }
    };
    if steps.is_empty() {
        s.redo = redo;
    } else {
        // The edits already marked their tiles for recompositing
        s.push_step(&label, before, false);
    }

    let output = serde_json::from_str::<Value>(&out.output).unwrap_or(Value::String(out.output));
    // Panels (`ui_show`) aren't shown in TextureCraft yet: say so rather than drop it silently
    let panel = matches!(out.ui, Some(UiChange::Show(_))).then_some("TextureCraft doesn't show plug-in panels yet");
    Ok(json!({"plugin": m.id, "command": c.id, "output": output, "notes": notes, "logs": out.logs, "calls": out.calls, "fuel": out.fuel, "panel": panel}))
}

// ---------- installing ----------

/// Read the bundle `p` names: `path`, `data` (base64), `repo` (`owner/repo[@tag]`) or `catalog` +
/// `id` (and optionally `version`).
pub(crate) fn obtain(s: &mut Session, cmd: &str, p: &Value) -> Result<Candidate> {
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
    if let Some(path) = text("path") {
        if s.plugins.confined {
            return Err(bad(cmd, "plug-in files can't be read while automation is confined to folders: pass the bundle as `data` (base64)"));
        }
        let bytes = read_bundle_file(path).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
        return Ok(Candidate { bytes, source: json!({"type": "file", "path": path}), in_catalog: None });
    }

    if let Some(data) = text("data") {
        if data.len() > bundle::MAX_BUNDLE_BYTES / 3 * 4 + 4 {
            return Err(bad(cmd, format!("the bundle is larger than {} MB", bundle::MAX_BUNDLE_BYTES >> 20)));
        }
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| bad(cmd, format!("`data` isn't base64: {e}")))?;
        let name = text("name").unwrap_or("data");
        return Ok(Candidate { bytes, source: json!({"type": "file", "path": name}), in_catalog: None });
    }

    if let Some(spec) = text("repo") {
        let repo = github::parse_repo_spec(spec).map_err(|e| bad(cmd, e))?;
        let (plan, bytes) = with_fetch(s, |f| {
            let plan = fetch::resolve_repo(f, &repo)?;
            let bytes = fetch::download_repo(f, &plan)?;
            Ok::<_, String>((plan, bytes))
        })
        .map_err(|e| bad(cmd, e))?;
        let source = json!({"type": "github", "repo": plan.repo.slug(), "tag": plan.repo.tag.clone().unwrap_or_default(), "asset": plan.asset});
        let catalogs = s.plugins.prefs.catalogs.clone();
        let confined = s.plugins.confined;
        let in_catalog = with_fetch(s, |f| {
            catalogs.iter().filter(|url| local_refused(url, confined).is_ok()).any(|url| {
                fetch::fetch_catalog(f, url)
                    .is_ok_and(|(cat, _)| cat.packages.iter().any(|pkg| pkg.repo_ref().is_some_and(|r| r.same_repo(&repo.owner, &repo.name))))
            })
        });
        return Ok(Candidate { bytes, source, in_catalog: Some(in_catalog) });
    }

    if let Some(url) = text("catalog") {
        let id = text("id").ok_or_else(|| bad(cmd, "`id` required with `catalog` (a package of the catalog)"))?;
        let version = text("version");
        local_refused(url, s.plugins.confined).map_err(|e| bad(cmd, e))?;
        let bytes = with_fetch(s, |f| {
            let (cat, _) = fetch::fetch_catalog(f, url)?;
            let package = cat.package(id).ok_or_else(|| format!("{url} has no package `{id}`"))?;
            let release = match version {
                Some(v) => package.release(v).ok_or_else(|| format!("{id} has no version {v} in {url}"))?,
                None => package.latest_for(&APP).ok_or_else(|| format!("{id} has no release for TextureCraft {}", APP.version))?,
            };
            release.target(&package.kind, &APP)?;
            let bytes = fetch::download_release(f, url, package, release)?;
            let (_, manifest) = bundle::open(&bytes)?;
            package.check_manifest(release, &manifest)?;
            Ok::<_, String>(bytes)
        })
        .map_err(|e| bad(cmd, e))?;
        return Ok(Candidate { bytes, source: json!({"type": "catalog", "catalog": url, "package": id}), in_catalog: Some(true) });
    }

    Err(bad(cmd, "pass `path`, `data` (base64), `repo` (owner/repo or owner/repo@tag) or `catalog` with `id`"))
}

/// The bundle file at `path`, refused past [`bundle::MAX_BUNDLE_BYTES`] before it is read whole.
#[cfg(not(target_arch = "wasm32"))]
fn read_bundle_file(path: &str) -> std::result::Result<Vec<u8>, String> {
    use std::io::Read as _;

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    // One byte past the limit tells a bundle at the limit from a larger one
    file.take(bundle::MAX_BUNDLE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > bundle::MAX_BUNDLE_BYTES {
        return Err(format!("larger than {} MB", bundle::MAX_BUNDLE_BYTES >> 20));
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
fn read_bundle_file(_path: &str) -> std::result::Result<Vec<u8>, String> {
    Err("the browser has no file paths: pass the bundle as `data` (base64)".into())
}

/// What an install confirmation shows about `c`: the manifest's permissions, kind and commands,
/// the source, the SHA-256, whether a catalog lists it and what it replaces. An error when the
/// bundle isn't a plug-in TextureCraft can run.
pub(crate) fn review(s: &Session, cmd: &str, c: &Candidate) -> Result<(Manifest, Value)> {
    let (_, manifest) = bundle::open(&c.bytes).map_err(|e| bad(cmd, e))?;
    manifest.target(&APP).map_err(|e| bad(cmd, format!("{} can't be installed: {e}", manifest.name)))?;
    let old = s.plugins.get(&manifest.id);
    let mut v = serde_json::to_value(&manifest).unwrap_or_else(|_| json!({"id": manifest.id}));
    v["kind"] = json!(manifest.kind);
    v["sha256"] = json!(bundle::sha256_hex(&c.bytes));
    v["source"] = c.source.clone();
    v["inCatalog"] = json!(c.in_catalog);
    v["replaces"] = json!(old.map(|o| &o.manifest.version));
    v["permissionsChanged"] = json!(old.is_some_and(|o| craft_plugin::permissions::changed(&o.manifest, &manifest)));
    v["stored"] = json!(s.plugins.has_store());
    Ok((manifest, v))
}

/// Install `c` (already reviewed): into the user's folder when the session has one, else for the
/// session. Replaces an installed plug-in with the same id; its settings stay.
pub(crate) fn install(s: &mut Session, cmd: &str, c: Candidate) -> Result<InstalledPlugin> {
    let (manifest, _) = review(s, cmd, &c)?;
    let sha256 = bundle::sha256_hex(&c.bytes);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(store) = &s.plugins.store {
        let source: craft_plugin::store::Source = serde_json::from_value(c.source.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        let (i, _) = store.install_for(&APP, &c.bytes, source).map_err(|e| bad(cmd, e))?;
        let plugin = InstalledPlugin { manifest: i.manifest, sha256: i.record.sha256, source: c.source, stored: true, wasm: Arc::new(i.bundle.wasm) };
        return Ok(s.plugins.keep(plugin));
    }

    let (b, _) = bundle::open(&c.bytes).map_err(|e| bad(cmd, e))?;
    Ok(s.plugins.keep(InstalledPlugin { manifest, sha256, source: c.source, stored: false, wasm: Arc::new(b.wasm) }))
}

/// Keep `c`, named by the install parameters `request`, for the install that follows its review
/// (one at a time).
pub(crate) fn hold(s: &mut Session, sha256: &str, request: Value, c: Candidate) {
    s.plugins.pending = Some(Pending { sha256: sha256.into(), request, candidate: c });
}

/// The bundle reviewed last, when it is `sha256` and `request` named it.
pub(crate) fn take_held(s: &mut Session, sha256: &str, request: &Value) -> Option<Candidate> {
    let held = s.plugins.pending.take_if(|h| h.sha256.eq_ignore_ascii_case(sha256) && h.request == *request)?;
    Some(held.candidate)
}

/// Remove plug-in `id` and its settings; whether it was installed.
pub(crate) fn remove(s: &mut Session, cmd: &str, id: &str) -> Result<bool> {
    let Some(p) = s.plugins.get(id).cloned() else { return Ok(false) };
    #[cfg(not(target_arch = "wasm32"))]
    if p.stored
        && let Some(store) = &s.plugins.store
    {
        store.remove(&p.manifest.id).map_err(|e| bad(cmd, e))?;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = cmd;
    s.plugins.forget(&p.manifest.id);
    Ok(true)
}

/// The catalog `url`'s packages TextureCraft can run, with their newest release for it.
pub(crate) fn catalog_packages(s: &Session, url: &str) -> std::result::Result<(Vec<Value>, Vec<String>), String> {
    local_refused(url, s.plugins.confined)?;
    let (cat, warnings) = with_fetch(s, |f| fetch::fetch_catalog(f, url))?;
    let packages = cat
        .packages
        .iter()
        .filter_map(|pkg| {
            let release = pkg.latest_for(&APP)?;
            let installed = s.plugins.get(&pkg.id).map(|p| p.manifest.version.clone());
            Some(json!({
                "catalog": url, "id": pkg.id, "name": pkg.name, "version": release.version, "author": pkg.author,
                "description": pkg.description, "license": pkg.license, "repo": pkg.repo, "kind": pkg.kind,
                "sha256": release.sha256, "installed": installed,
            }))
        })
        .collect();
    Ok((packages, warnings))
}

/// Why the catalog `url` can't be read here: a `file://` catalog (and the files it names) is read
/// straight from the disk, past the automation roots, so it is refused while they are in force.
fn local_refused(url: &str, confined: bool) -> std::result::Result<(), String> {
    if confined && url.get(..7).is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://")) {
        return Err("file:// catalogs can't be read while automation is confined to folders".into());
    }
    Ok(())
}

/// Whether `url` is a catalog address TextureCraft reads (`https://` on GitHub's hosts, or
/// `file://`), or why not.
pub(crate) fn check_catalog_url(s: &Session, url: &str) -> std::result::Result<(), String> {
    local_refused(url, s.plugins.confined)?;
    catalog::check_url(url, None).map(|_| ())
}

/// Where plug-in `p` has a newer release: the install parameters that get it and its version (a
/// tag, for a repository). `None` when it is up to date or came from a file.
pub(crate) fn update_for(s: &Session, p: &InstalledPlugin) -> std::result::Result<Option<(Value, String)>, String> {
    let text = |k: &str| p.source.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match p.source.get("type").and_then(Value::as_str) {
        Some("catalog") => {
            let (url, id) = (text("catalog"), text("package"));
            local_refused(&url, s.plugins.confined)?;
            let (cat, _) = with_fetch(s, |f| fetch::fetch_catalog(f, &url))?;
            let release = cat.package(&id).and_then(|pkg| pkg.latest_for(&APP));
            Ok(release
                .filter(|r| bundle::newer(&r.version, &p.manifest.version))
                .map(|r| (json!({"catalog": url, "id": id, "version": r.version}), r.version.clone())))
        }
        Some("github") => {
            let mut repo = github::parse_repo_spec(&text("repo"))?;
            repo.tag = None;
            let plan = with_fetch(s, |f| fetch::resolve_repo(f, &repo))?;
            let tag = plan.repo.tag.clone().unwrap_or_default();
            Ok((tag != text("tag")).then(|| (json!({"repo": format!("{}@{tag}", plan.repo.slug())}), tag)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests;

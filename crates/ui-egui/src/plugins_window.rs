//! The Plug-ins menu and its windows: Manage plug-ins (the installed plug-ins and their commands,
//! installing from a file, a GitHub repository or a catalog, updates), the install confirmation
//! (permissions, kind, source, SHA-256) and the parameter dialog generated from a plug-in command's
//! `params`. Everything goes through the engine's `plugin.*` commands
//! (`texturecraft_engine::plugin`).

use egui::RichText;
use serde_json::{Value, json};
use texturecraft_engine::plugin::craft_plugin::Access;

use crate::TextureCraftApp;
use crate::i18n::{tr, trf};
use crate::plugin_params_dialog::PluginParamsDialog;
use crate::plugin_review::PluginReview;
use crate::theme::{Tokens, caption};
use crate::widgets::primary_button;

/// The menu bar's Plug-ins menu
pub const MENU: &str = "Plug-ins";
/// The menu item that opens the Plug-ins window
pub const MANAGE: &str = "ui.plugins";
/// The Plug-ins window's width (points)
const WINDOW_WIDTH: f32 = 440.0;
/// Width of the repository and catalog address fields (points)
const ADDRESS_WIDTH: f32 = 260.0;

/// The Plug-ins window, the install confirmation and the parameter dialog, while they show.
#[derive(Default)]
pub struct PluginsWindow {
    pub open: bool,
    /// The GitHub repository typed (`owner/repo` or `owner/repo@tag`).
    repo: String,
    /// The catalog address typed.
    catalog_url: String,
    /// The last action's message.
    status: String,
    /// The catalogs' packages, after Browse catalogs.
    packages: Vec<Value>,
    /// The updates found, after Check for updates.
    updates: Vec<Value>,
    /// The install confirmation, while it waits for an answer.
    pub review: Option<PluginReview>,
    /// A plug-in command's parameters, while they are entered.
    pub params: Option<PluginParamsDialog>,
}

/// Run `f` on the app with its Plug-ins window state held apart.
fn with_window<T>(app: &mut TextureCraftApp, f: impl FnOnce(&mut TextureCraftApp, &mut PluginsWindow) -> T) -> T {
    let mut w = std::mem::take(&mut app.plugins);
    let out = f(app, &mut w);
    app.plugins = w;
    out
}

/// An engine command, its error as text.
fn engine(app: &mut TextureCraftApp, id: &str, params: Value) -> Result<Value, String> {
    texturecraft_engine::execute(&mut app.session, id, &params).map_err(|e| e.to_string())
}

/// The Plug-ins menu: each installed plug-in's commands, then Manage plug-ins.
pub fn menu(app: &mut TextureCraftApp, ctx: &egui::Context, ui: &mut egui::Ui) {
    ui.menu_button(tr(MENU), |ui| {
        ui.set_min_width(220.0);
        let plugins: Vec<_> = app.session.plugins.list().to_vec();
        for p in &plugins {
            let m = &p.manifest;
            // Plug-in names and command labels are the plug-in's own, not translated
            match m.commands.as_slice() {
                [] => {}
                [one] => {
                    if ui.button(m.name.as_str()).clicked() {
                        run_command(app, &m.id, &one.id);
                        ui.close();
                    }
                }
                many => {
                    ui.menu_button(m.name.as_str(), |ui| {
                        for c in many {
                            if ui.button(c.label.as_str()).clicked() {
                                run_command(app, &m.id, &c.id);
                                ui.close();
                            }
                        }
                    });
                }
            }
        }
        if !plugins.is_empty() {
            ui.separator();
        }
        if ui.button(tr("Manage plug-ins...")).clicked() {
            crate::menus::invoke(app, ctx, MANAGE);
            ui.close();
        }
    });
}

/// Run a plug-in command, after its parameter dialog when it declares parameters.
pub fn run_command(app: &mut TextureCraftApp, id: &str, command: &str) {
    let Some(plugin) = app.session.plugins.get(id) else { return };
    let Some(c) = plugin.manifest.command(Some(command)) else { return };
    if c.params.is_empty() {
        let params = json!({"id": plugin.manifest.id, "command": c.id});
        run_plugin(app, params);
        return;
    }
    app.plugins.params = Some(PluginParamsDialog::new(&plugin.manifest.id, &plugin.manifest.name, c));
}

/// `plugin.run`, its notes (or that it is done) in a toast; a failure is toasted by `app.run`.
fn run_plugin(app: &mut TextureCraftApp, params: Value) {
    let name = params["id"].as_str().and_then(|id| app.session.plugins.get(id)).map(|p| p.manifest.name.clone()).unwrap_or_default();
    let Some(r) = app.run("plugin.run", params) else { return };
    let notes: Vec<&str> = r["notes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let mut line = if notes.is_empty() { trf("{name}: done", &[("name", &name)]) } else { notes.join(" · ") };
    if r["panel"].is_string() {
        line = format!("{line} ({})", tr("TextureCraft doesn't show plug-in panels yet"));
    }
    app.toast(line, false);
}

/// Check a bundle (`plugin.install {…, review: true}`) and show its confirmation.
fn review(app: &mut TextureCraftApp, w: &mut PluginsWindow, request: Value) {
    let mut asked = request.clone();
    asked["review"] = json!(true);
    match engine(app, "plugin.install", asked) {
        Ok(info) => {
            w.review = Some(PluginReview { request, info });
            w.status.clear();
        }
        Err(e) => w.status = e,
    }
}

/// Install the reviewed bundle (the one with the reviewed SHA-256 only).
fn install_reviewed(app: &mut TextureCraftApp, w: &mut PluginsWindow, reviewed: PluginReview) {
    let mut request = reviewed.request;
    request["sha256"] = reviewed.info["sha256"].clone();
    w.status = match engine(app, "plugin.install", request) {
        Ok(_) => trf("Installed {name}.", &[("name", &reviewed.info["name"].as_str().unwrap_or_default())]),
        Err(e) => e,
    };
    w.open = true;
}

/// The Plug-ins window and its dialogs, when they show.
pub fn show(app: &mut TextureCraftApp, ctx: &egui::Context) {
    with_window(app, |app, w| {
        if w.open {
            manage(app, w, ctx);
        }
        if w.review.is_some() {
            confirmation(app, w, ctx);
        }
        if w.params.is_some() {
            parameters(app, w, ctx);
        }
    });
}

fn manage(app: &mut TextureCraftApp, w: &mut PluginsWindow, ctx: &egui::Context) {
    let mut open = true;
    crate::windows::window("plugins", tr("Plug-ins"), &mut open).default_width(WINDOW_WIDTH).show(ctx, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| manage_body(app, w, ui));
    });
    w.open = open;
}

fn manage_body(app: &mut TextureCraftApp, w: &mut PluginsWindow, ui: &mut egui::Ui) {
    let dim = Tokens::get(ui.ctx()).text_dim;
    ui.label(caption(tr("Installed")));
    let plugins: Vec<_> = app.session.plugins.list().to_vec();
    if plugins.is_empty() {
        ui.label(RichText::new(tr("No plug-ins installed.")).color(dim));
    }
    for p in &plugins {
        let m = &p.manifest;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {}", m.name, m.version)).strong());
            let access = if m.permissions.access == Access::Write { tr("edits the project") } else { tr("reads the project") };
            ui.label(RichText::new(access).color(dim));
            if !p.stored {
                ui.label(RichText::new(tr("this session only")).color(dim));
            }
        });
        ui.horizontal_wrapped(|ui| {
            for c in &m.commands {
                if ui.button(c.label.as_str()).clicked() {
                    // A command with parameters opens its dialog over this window
                    std::mem::swap(&mut app.plugins, w);
                    run_command(app, &m.id, &c.id);
                    std::mem::swap(&mut app.plugins, w);
                }
            }
            if ui.button(tr("Remove")).clicked() {
                w.status = engine(app, "plugin.remove", json!({"id": m.id})).err().unwrap_or_default();
            }
        });
    }
    for problem in app.session.plugins.problems() {
        ui.label(RichText::new(problem).color(dim));
    }

    ui.add_space(8.0);
    ui.label(caption(tr("Install")));
    #[cfg(not(target_arch = "wasm32"))]
    if ui.button(tr("From file...")).clicked() {
        let dialog = crate::file_dialogs::FileDialog::open().filter(tr("Plug-ins"), &[texturecraft_engine::plugin::craft_plugin::BUNDLE_EXTENSION]);
        app.ask_file(dialog, crate::file_dialogs::Bound::App, |app, paths| {
            let Some(path) = paths.first() else { return };
            let request = json!({"path": path.display().to_string()});
            with_window(app, |app, w| review(app, w, request));
        });
    }
    ui.horizontal(|ui| {
        ui.label(tr("GitHub repository"));
        ui.add(egui::TextEdit::singleline(&mut w.repo).desired_width(ADDRESS_WIDTH).hint_text("owner/repo"));
        if ui.add_enabled(!w.repo.trim().is_empty(), egui::Button::new(tr("Review..."))).clicked() {
            let repo = w.repo.trim().to_string();
            review(app, w, json!({"repo": repo}));
        }
    });

    ui.add_space(8.0);
    ui.label(caption(tr("Catalogs")));
    for url in app.session.plugins.prefs.catalogs.clone() {
        ui.horizontal(|ui| {
            ui.label(url.as_str());
            if ui.small_button(tr("Remove")).clicked() {
                w.status = engine(app, "plugin.catalog.add", json!({"url": url, "remove": true})).err().unwrap_or_default();
            }
        });
    }
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut w.catalog_url).desired_width(ADDRESS_WIDTH).hint_text("https://raw.githubusercontent.com/…/index.json"));
        if ui.add_enabled(!w.catalog_url.trim().is_empty(), egui::Button::new(tr("Add"))).clicked() {
            match engine(app, "plugin.catalog.add", json!({"url": w.catalog_url.trim()})) {
                Ok(_) => w.catalog_url.clear(),
                Err(e) => w.status = e,
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!app.session.plugins.prefs.catalogs.is_empty(), egui::Button::new(tr("Browse catalogs"))).clicked() {
            match engine(app, "plugin.catalog.list", json!({})) {
                Ok(r) => {
                    w.packages = r["packages"].as_array().cloned().unwrap_or_default();
                    let warnings: Vec<&str> = r["warnings"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                    w.status = warnings.join("\n");
                }
                Err(e) => w.status = e,
            }
        }
        if ui.add_enabled(!plugins.is_empty(), egui::Button::new(tr("Check for updates"))).clicked() {
            match engine(app, "plugin.update", json!({})) {
                Ok(r) => {
                    w.updates = r["updates"].as_array().cloned().unwrap_or_default();
                    w.status = if w.updates.is_empty() { tr("Every plug-in is up to date.").to_string() } else { String::new() };
                }
                Err(e) => w.status = e,
            }
        }
    });
    for pkg in w.packages.clone() {
        ui.horizontal(|ui| {
            let installed = pkg["installed"].as_str().map(|v| format!(" ({})", trf("installed: {version}", &[("version", &v)]))).unwrap_or_default();
            ui.label(format!("{} {}{installed}", pkg["name"].as_str().unwrap_or_default(), pkg["version"].as_str().unwrap_or_default()));
            if ui.small_button(tr("Install...")).clicked() {
                review(app, w, json!({"catalog": pkg["catalog"], "id": pkg["id"]}));
            }
        });
        if let Some(text) = pkg["description"].as_str().filter(|t| !t.is_empty()) {
            ui.label(RichText::new(text).color(dim));
        }
    }
    for u in w.updates.clone() {
        ui.horizontal(|ui| {
            let text = |k: &str| u[k].as_str().unwrap_or_default().to_string();
            ui.label(format!("{} {} → {}", text("id"), text("installed"), text("available")));
            if ui.small_button(tr("Update...")).clicked() {
                review(app, w, u["install"].clone());
            }
        });
    }
    if !w.status.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(w.status.as_str()).color(dim));
    }
}

/// The install confirmation: nothing is written before Install.
fn confirmation(app: &mut TextureCraftApp, w: &mut PluginsWindow, ctx: &egui::Context) {
    let Some(reviewed) = w.review.clone() else { return };
    let info = &reviewed.info;
    let text = |k: &str| info[k].as_str().unwrap_or_default().to_string();
    let mut open = true;
    let mut answer = None;
    crate::windows::dialog("plugin_install", tr("Install plug-in"), &mut open).show(ctx, |ui| {
        let dim = Tokens::get(ui.ctx()).text_dim;
        ui.label(RichText::new(format!("{} {}", text("name"), text("version"))).font(crate::theme::semibold(14.0)));
        for k in ["author", "description", "license", "repo"] {
            if !text(k).is_empty() {
                ui.label(text(k));
            }
        }
        egui::Grid::new("plugin_confirm").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            ui.label(tr("Kind"));
            ui.label(text("kind"));
            ui.end_row();
            ui.label(tr("Access"));
            let write = info["permissions"]["access"] == "write";
            ui.label(if write { tr("Reads and edits the open project") } else { tr("Reads the open project") });
            ui.end_row();
            ui.label(tr("Commands"));
            let allowed: Vec<&str> = info["permissions"]["commands"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            ui.label(if allowed.is_empty() { tr("Any command its access allows").to_string() } else { allowed.join(", ") });
            ui.end_row();
            if info["permissions"]["saveFile"] == true {
                ui.label(tr("Files"));
                ui.label(tr("Asks to save files (TextureCraft doesn't offer this yet)"));
                ui.end_row();
            }
            ui.label(tr("Source"));
            ui.label(reviewed.source_text());
            ui.end_row();
            ui.label("SHA-256");
            ui.add(egui::Label::new(RichText::new(text("sha256")).monospace()).wrap());
            ui.end_row();
        });
        if info["inCatalog"] == false {
            ui.label(RichText::new(tr("This plug-in isn't in any catalog you use: install it only if you trust its author.")).strong());
        }
        if let Some(old) = info["replaces"].as_str() {
            ui.label(RichText::new(trf("Replaces version {version}.", &[("version", &old)])).color(dim));
        }
        if info["permissionsChanged"] == true {
            ui.label(RichText::new(tr("This version asks for different permissions.")).strong());
        }
        if info["stored"] == false {
            ui.label(RichText::new(tr("It is kept for this session only.")).color(dim));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add(primary_button(ui, tr("Install"))).clicked() {
                answer = Some(true);
            }
            if ui.button(tr("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if !open || answer.is_some() {
        w.review = None;
    }
    if answer == Some(true) {
        install_reviewed(app, w, reviewed);
    }
}

/// A plug-in command's parameter dialog.
fn parameters(app: &mut TextureCraftApp, w: &mut PluginsWindow, ctx: &egui::Context) {
    let Some(dialog) = w.params.as_mut() else { return };
    let mut open = true;
    let mut answer = None;
    let title = dialog.title.clone();
    crate::windows::dialog("plugin_params", &title, &mut open).show(ctx, |ui| {
        dialog.body(ui);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add(primary_button(ui, tr("Run"))).clicked() {
                answer = Some(true);
            }
            if ui.button(tr("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if !open || answer.is_some() {
        let finished = w.params.take();
        if answer == Some(true)
            && let Some(dialog) = finished
        {
            run_plugin(app, dialog.run_params());
        }
    }
}

#[cfg(test)]
mod tests {
    use texturecraft_engine::Session;
    use texturecraft_engine::plugin::craft_plugin::bundle::Bundle;

    use super::*;

    /// A plug-in whose commands each add a fill layer; `asks` declares a parameter.
    fn install(app: &mut TextureCraftApp) {
        let wasm = wat::parse_str(
            r#"(module
              (import "craft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0) "layer.add.fill")
              (data (i32.const 32) "{}")
              (func (export "craft_run") (param i32) (result i32)
                (drop (call $call (i32.const 0) (i32.const 14) (i32.const 32) (i32.const 2))) (i32.const 0)))"#,
        )
        .unwrap();
        let manifest = json!({"id": "io.example.ui", "name": "UI Test", "version": "1.0.0", "main": "p.wasm",
            "apps": {"texturecraft": {"api": 6}}, "permissions": {"access": "write"},
            "commands": [{"id": "plain", "label": "Plain"},
                         {"id": "asks", "label": "Asks...", "params": [{"name": "count", "type": "number", "default": 2, "min": 1, "max": 9}]}]});
        let bytes = Bundle { manifest: manifest.to_string(), wasm, assets: vec![] }.encode();
        let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);

        // Review, then install what was reviewed, as the confirmation does
        with_window(app, |app, w| {
            review(app, w, json!({"data": data}));
            let reviewed = w.review.take().unwrap_or_else(|| panic!("{}", w.status));
            assert_eq!(reviewed.info["permissions"]["access"], "write");
            assert_eq!(reviewed.source_text(), "data");
            install_reviewed(app, w, reviewed);
        });
    }

    #[test]
    fn plugin_commands_run_as_one_step_and_ask_for_their_parameters() {
        let session = Session::new_project("Plug-ins", texturecraft_engine::geom::primitives::plane(1.0, 1), "builtin:plane", None, 128).unwrap();
        let context = eframe::CreationContext::_new_kittest(egui::Context::default());
        let mut app = TextureCraftApp::new(&context, session);
        install(&mut app);
        let layers = app.session.doc.project.layers.len();

        // A command without parameters runs at once
        run_command(&mut app, "io.example.ui", "plain");
        assert_eq!(app.session.doc.project.layers.len(), layers + 1);
        assert_eq!(app.session.history_labels().last().map(String::as_str), Some("UI Test: Plain"));

        // One with parameters asks first
        run_command(&mut app, "io.example.ui", "asks");
        let dialog = app.plugins.params.clone().unwrap();
        assert_eq!(dialog.values["count"], 2.0);
        assert_eq!(dialog.title, "UI Test: Asks");
        run_plugin(&mut app, dialog.run_params());
        assert_eq!(app.session.doc.project.layers.len(), layers + 2);
    }

    #[test]
    fn a_repository_in_no_catalog_says_so() {
        let reviewed = PluginReview {
            request: json!({"repo": "example/x"}),
            info: json!({"source": {"type": "github", "repo": "example/x", "tag": "v1", "asset": "x.craftplugin"}, "inCatalog": false}),
        };
        assert_eq!(reviewed.source_text(), "github.com/example/x v1 (x.craftplugin)");
    }
}

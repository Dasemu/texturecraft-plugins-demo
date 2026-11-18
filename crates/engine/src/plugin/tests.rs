//! Plug-in runs against a real session, with modules assembled from WebAssembly text.

use serde_json::json;

use crate::Session;
use crate::plugin::craft_plugin::bundle::Bundle;

/// A module that calls `calls` (command, params JSON) in order. A refused or failed call fails the
/// run when `strict`; otherwise its message becomes the output and the run goes on. `tail` is WAT
/// run after the calls (a trap, an endless loop), before returning 0.
fn module(calls: &[(&str, &str)], strict: bool, tail: &str) -> Vec<u8> {
    let mut data = String::new();
    let mut body = String::new();
    let mut at = 0usize;
    for (command, params) in calls {
        let (c, p) = (at, at + command.len());
        at = p + params.len();
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        data.push_str(&format!(r#"(data (i32.const {c}) "{}") (data (i32.const {p}) "{}")"#, esc(command), esc(params)));
        let on_error = if strict {
            "(return (i32.const 1))".to_string()
        } else {
            "(call $set_output (i32.const 8192) (call $reply_read (i32.const 8192) (call $reply_len)))".to_string()
        };
        body.push_str(&format!(
            "(if (i32.ne (call $call (i32.const {c}) (i32.const {}) (i32.const {p}) (i32.const {})) (i32.const 0)) (then {on_error}))\n",
            command.len(),
            params.len()
        ));
    }
    let wat = format!(
        r#"(module
          (import "craft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
          (import "craft" "reply_len" (func $reply_len (result i32)))
          (import "craft" "reply_read" (func $reply_read (param i32 i32) (result i32)))
          (import "craft" "set_output" (func $set_output (param i32 i32)))
          (memory (export "memory") 1)
          {data}
          (func (export "craft_run") (param $n i32) (result i32) {body} {tail} (i32.const 0)))"#
    );
    wat::parse_str(&wat).unwrap_or_else(|e| panic!("{e}\n{wat}"))
}

/// The `.craftplugin` of plug-in `id` for TextureCraft, asking for `access`.
fn bundle(id: &str, access: &str, wasm: Vec<u8>) -> Vec<u8> {
    let manifest = json!({
        "id": id, "name": "Test Plug-in", "version": "1.0.0", "main": "plugin.wasm",
        "apps": {"texturecraft": {"api": 6}},
        "permissions": {"access": access},
        "commands": [{"id": "go", "label": "Go"}]
    });
    Bundle { manifest: manifest.to_string(), wasm, assets: vec![] }.encode()
}

fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn session() -> Session {
    Session::new_project("t", crate::geom::primitives::plane(1.0, 1), "builtin:plane", None, 128).unwrap()
}

/// A session with plug-in `id` installed for the session.
fn with_plugin(id: &str, access: &str, wasm: Vec<u8>) -> Session {
    let mut s = session();
    let r = crate::execute(&mut s, "plugin.install", &json!({"data": base64(&bundle(id, access, wasm)), "name": "test.craftplugin"})).unwrap();
    assert_eq!(r["installed"], true);
    assert_eq!(r["stored"], false, "without a store, a plug-in is kept for the session");
    s
}

fn layers(s: &Session) -> usize {
    s.doc.project.layers.len()
}

/// A scratch folder for one test.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tc-plugins-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const FILL: (&str, &str) = ("layer.add.fill", r#"{"color":[1,0,0]}"#);
const FILL2: (&str, &str) = ("layer.add.fill", r#"{"color":[0,0,1]}"#);

#[test]
fn a_write_plugin_runs_real_commands_as_one_undo_step_named_after_it() {
    let mut s = with_plugin("io.example.fills", "write", module(&[FILL, FILL2], true, ""));
    let (count, steps) = (layers(&s), s.history_labels().len());

    let r = crate::execute(&mut s, "plugin.run", &json!({"id": "io.example.fills"})).unwrap();

    assert_eq!(r["calls"], 2);
    assert_eq!(layers(&s), count + 2);
    let history = s.history_labels();
    assert_eq!(history.len(), steps + 1, "two commands, one undo step");
    assert_eq!(history.last().map(String::as_str), Some("Test Plug-in"));
    crate::execute(&mut s, "edit.undo", &json!({})).unwrap();
    assert_eq!(layers(&s), count, "one Undo takes the whole run back");
}

#[test]
fn a_read_plugin_that_edits_is_put_back_and_refused() {
    let mut s = with_plugin("io.example.reader", "read", module(&[("layer.list", "{}"), FILL], false, ""));
    let (count, steps) = (layers(&s), s.history_labels().len());
    s.modified = false;

    let r = crate::execute(&mut s, "plugin.run", &json!({"id": "io.example.reader"})).unwrap();

    let message = r["output"].as_str().unwrap();
    assert!(message.contains("may only read it"), "{message}");
    // Both commands were called (the reply reads count as host calls too)
    assert!(r["calls"].as_u64().is_some_and(|n| n >= 2), "{r}");
    assert_eq!(layers(&s), count);
    assert_eq!(s.history_labels().len(), steps, "nothing to undo");
    assert!(!s.modified, "the project isn't marked changed");
}

#[test]
fn refused_commands_and_file_paths_are_refused() {
    for (call, why) in [
        (("file.save", "{}"), "isn't available to plugins"),
        (("file.open", r#"{"path":"/etc/hosts"}"#), "isn't available to plugins"),
        (("edit.undo", "{}"), "isn't available to plugins"),
        (("edit.history", "{}"), "isn't available to plugins"),
        (("project.new", "{}"), "isn't available to plugins"),
        (("layer.paste", "{}"), "isn't available to plugins"),
        (("material.link.cook", "{}"), "isn't available to plugins"),
        (("bake.skew.set", r#"{"path":"/etc/hosts"}"#), "can't name files"),
        (("plugin.list", "{}"), "isn't available to plugins"),
    ] {
        let mut s = with_plugin("io.example.sneaky", "write", module(&[call], false, ""));
        let r = crate::execute(&mut s, "plugin.run", &json!({"id": "io.example.sneaky"})).unwrap();
        let message = r["output"].as_str().unwrap_or_default();
        assert!(message.contains(why), "{}: {message}", call.0);
    }
}

#[test]
fn a_command_outside_the_manifest_allowlist_is_refused() {
    let mut s = session();
    let manifest = json!({"id": "io.example.narrow", "name": "Narrow", "version": "1.0.0", "main": "p.wasm",
        "apps": {"texturecraft": {"api": 6}}, "permissions": {"access": "write", "commands": ["layer.select"]}});
    let bytes = Bundle { manifest: manifest.to_string(), wasm: module(&[FILL], false, ""), assets: vec![] }.encode();
    crate::execute(&mut s, "plugin.install", &json!({"data": base64(&bytes)})).unwrap();
    let count = layers(&s);

    let r = crate::execute(&mut s, "plugin.run", &json!({"id": "io.example.narrow"})).unwrap();

    assert!(r["output"].as_str().unwrap().contains("asked permission for"), "{r}");
    assert_eq!(layers(&s), count);
}

#[test]
fn a_trapping_or_endless_plugin_leaves_the_project_unchanged() {
    for (tail, fuel) in [("unreachable", None), ("(loop $l (br $l))", Some(200_000))] {
        let mut s = with_plugin("io.example.crash", "write", module(&[FILL, FILL2], true, tail));
        let (count, steps, selected) = (layers(&s), s.history_labels(), s.selected_layer());
        let mut params = json!({"id": "io.example.crash"});
        if let Some(f) = fuel {
            params["fuel"] = json!(f);
        }

        let e = crate::execute(&mut s, "plugin.run", &params).unwrap_err().to_string();

        assert!(e.contains("Test Plug-in"), "{e}");
        assert_eq!(layers(&s), count, "{tail}: the edits before the failure are rolled back");
        assert_eq!(s.history_labels(), steps);
        assert_eq!(s.selected_layer(), selected, "the selection is put back");
    }
}

#[test]
fn settings_are_bounded_and_kept_per_plugin() {
    let mut all = Default::default();
    super::set_setting(&mut all, "a", "k", Some("{\"x\": 1}")).unwrap();
    assert_eq!(all["a"]["k"], json!({"x": 1}));
    assert!(super::set_setting(&mut all, "a", &"k".repeat(129), Some("1")).is_err());
    assert!(super::set_setting(&mut all, "a", "big", Some(&format!("\"{}\"", "x".repeat(64 * 1024)))).is_err());
    assert!(super::set_setting(&mut all, "a", "bad", Some("not json")).is_err());
    super::set_setting(&mut all, "a", "k", None).unwrap();
    assert!(!all.contains_key("a"));
}

#[test]
fn install_review_list_and_remove_through_the_store() {
    let dir = scratch("store");
    let mut s = session();
    s.plugins.open_store(&dir);
    let bytes = bundle("io.example.stored", "write", module(&[], true, ""));
    let data = base64(&bytes);

    // The confirmation: described, nothing written
    let review = crate::execute(&mut s, "plugin.install", &json!({"data": data, "review": true})).unwrap();
    assert_eq!(review["installed"], false);
    assert_eq!(review["permissions"]["access"], "write");
    assert_eq!(review["kind"], "command");
    assert_eq!(review["sha256"], crate::plugin::craft_plugin::sha256_hex(&bytes));
    assert!(s.plugins.list().is_empty());
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "nothing is written before the install");

    // Another bundle than the one reviewed isn't installed
    let e = crate::execute(&mut s, "plugin.install", &json!({"data": data, "sha256": "0".repeat(64)})).unwrap_err();
    assert!(e.to_string().contains("reviewed"), "{e}");
    crate::execute(&mut s, "plugin.install", &json!({"data": data, "review": true})).unwrap();
    let r = crate::execute(&mut s, "plugin.install", &json!({"data": data, "sha256": review["sha256"]})).unwrap();
    assert_eq!((r["installed"].clone(), r["stored"].clone()), (json!(true), json!(true)));

    // A new session reads it back from the folder, and another project keeps it
    let mut again = session();
    again.plugins.open_store(&dir);
    let list = crate::execute(&mut again, "plugin.list", &json!({})).unwrap();
    assert_eq!(list["plugins"][0]["id"], "io.example.stored");
    assert_eq!(list["plugins"][0]["source"]["type"], "file");
    assert_eq!(list["stored"], true);
    again.switch_to(session());
    assert_eq!(again.plugins.list().len(), 1, "plug-ins outlive the project");

    crate::execute(&mut again, "plugin.remove", &json!({"id": "io.example.stored"})).unwrap();
    assert_eq!(crate::execute(&mut again, "plugin.list", &json!({})).unwrap()["plugins"], json!([]));
    let mut third = session();
    third.plugins.open_store(&dir);
    assert!(third.plugins.list().is_empty(), "removed from the folder");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plugins_for_other_apps_or_not_bundles_are_refused() {
    let mut s = session();
    let manifest = json!({"id": "io.example.other", "name": "Other", "version": "1.0.0", "main": "p.wasm", "apps": {"figcraft": {"api": 6}}});
    let bytes = Bundle { manifest: manifest.to_string(), wasm: module(&[], true, ""), assets: vec![] }.encode();
    let e = crate::execute(&mut s, "plugin.install", &json!({"data": base64(&bytes)})).unwrap_err().to_string();
    assert!(e.contains("written for figcraft"), "{e}");
    let e = crate::execute(&mut s, "plugin.install", &json!({"data": base64(b"not a bundle")})).unwrap_err().to_string();
    assert!(e.contains("isn't a Craft plugin bundle"), "{e}");
    assert!(crate::execute(&mut s, "plugin.install", &json!({})).is_err());
}

#[test]
fn a_local_catalog_installs_its_package() {
    let dir = scratch("catalog");
    let bytes = bundle("io.example.listed", "write", module(&[], true, ""));
    let file = dir.join("listed.craftplugin");
    std::fs::write(&file, &bytes).unwrap();
    let url = |p: &std::path::Path| format!("file://{}", p.to_string_lossy().replace('\\', "/"));
    let index = json!({"schema": 1, "packages": [{"id": "io.example.listed", "name": "Listed", "repo": "https://github.com/example/listed",
        "versions": [{"version": "1.0.0", "apps": {"texturecraft": {"api": 6}}, "url": url(&file),
            "sha256": crate::plugin::craft_plugin::sha256_hex(&bytes), "size": bytes.len()}]}]});
    let catalog = dir.join("index.json");
    std::fs::write(&catalog, index.to_string()).unwrap();
    let mut s = session();

    crate::execute(&mut s, "plugin.catalog.add", &json!({"url": url(&catalog)})).unwrap();
    let listed = crate::execute(&mut s, "plugin.catalog.list", &json!({})).unwrap();
    let r = crate::execute(&mut s, "plugin.install", &json!({"catalog": url(&catalog), "id": "io.example.listed"})).unwrap();

    assert_eq!(listed["packages"][0]["id"], "io.example.listed", "{listed}");
    assert_eq!(r["inCatalog"], true);
    assert_eq!(r["source"]["type"], "catalog");
    assert_eq!(crate::execute(&mut s, "plugin.update", &json!({})).unwrap()["updates"], json!([]), "up to date");
    assert!(crate::execute(&mut s, "plugin.catalog.add", &json!({"url": "https://example.com/index.json"})).is_err(), "GitHub's hosts only");

    // Confined automation reads neither local catalogs nor bundle paths
    s.plugins.confined = true;
    let e = crate::execute(&mut s, "plugin.catalog.list", &json!({"url": url(&catalog)})).unwrap()["warnings"].to_string();
    assert!(e.contains("confined"), "{e}");
    let e = crate::execute(&mut s, "plugin.install", &json!({"path": file.to_string_lossy()})).unwrap_err().to_string();
    assert!(e.contains("confined"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_repository_install_without_network_says_so() {
    let mut s = session();
    let e = crate::execute(&mut s, "plugin.install", &json!({"repo": "example/tools"})).unwrap_err().to_string();
    assert!(e.contains("no network access"), "{e}");
}

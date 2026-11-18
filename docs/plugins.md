# Plug-ins

TextureCraft runs **command plug-ins**: WebAssembly modules that act on the open project through the
same commands as the menus, the CLI, the control channel and MCP. The sandbox, the manifest
(`craft-plugin.json`), the `.craftplugin` bundle, the per-user store, catalogs and GitHub installs
come from the shared `craft-plugin` crate in
[craft-libs](https://github.com/storytold/craft-libs/tree/main/crates/plugin) (RFC-0046); its
README describes the ABI and how to write and publish a plug-in. TextureCraft's side is
`crates/engine/src/plugin.rs` and the `plugin.*` commands in `crates/engine/src/plugin_cmds.rs`.

## Using plug-ins

- **Plug-ins ▸ Manage plug-ins...** lists the installed plug-ins with their commands, and installs
  from a `.craftplugin` file (desktop), from a GitHub repository (`owner/repo` or `owner/repo@tag`:
  its latest release's `.craftplugin`, checked against the release's `SHA256SUMS`) or from a
  catalog (`index.json` on GitHub's hosts, or a `file://` one). Check for updates looks at each
  plug-in's catalog or repository.
- Every install shows a confirmation first: the plug-in's access, the commands it may call, its
  kind, where it comes from and its SHA-256, and says plainly when a repository is in no catalog
  you use. Nothing is written before **Install**.
- The Plug-ins menu lists each plug-in's commands. A command with parameters asks for them in a
  dialog generated from its manifest.
- The desktop app keeps plug-ins in `Plug-ins/` beside its saved settings. The CLI, MCP, the web
  build and runs with `TEXTURECRAFT_NO_PREFS` keep them for the session only.
- A plug-in only installs when its manifest names `texturecraft` in `apps`.

## Commands

| Command | Parameters |
|---|---|
| `plugin.list` | `{}` |
| `plugin.install` | `{path}`, `{data: base64, name?}`, `{repo: owner/repo[@tag]}` or `{catalog, id, version?}`; `review: true` checks and describes the bundle without installing; `sha256` installs only that bundle |
| `plugin.update` | `{id?, apply?}`: lists newer releases; `apply` installs those asking for no other permissions |
| `plugin.remove` | `{id}`: the plug-in and its settings |
| `plugin.run` | `{id, command?, params?, fuel?}` |
| `plugin.catalog.list` | `{url?}`: the packages TextureCraft can run |
| `plugin.catalog.add` | `{url, remove?}` |

## What a run may do

- A run is **one undo step** named after the plug-in. A run that fails (a trap, an exhausted
  instruction budget, a non-zero status) puts the project, the selection and the undo history back
  as they were. A stroke, box fill or path the run left open is applied as part of the run.
- `permissions.access: "read"` plug-ins may call any allowed command, but a call that changes the
  project (an undo step, or a change that marks the project unsaved) is undone and refused.
  `permissions.commands` narrows the commands further.
- Settings: each plug-in keeps up to 64 KB of its own settings (keys up to 128 characters), saved
  with the app's settings.
- **Refused commands**, whatever the manifest asks: every `plugin.*`, `file.*`, `edit.undo` and
  `edit.redo` (and the other names `craft-plugin` refuses in every app); the undo history
  (`edit.history`); other projects (`project.new`, `project.sample`); the user's own templates
  and presets (`project.templates`, `project.template.save`, `export.preset.save`,
  `export.preset.remove`, `brush.preset.save`, `brush.preset.load`); files beside the project
  (`material.save`, `material.load`, `resource.import`, `textureSet.meshMap.import`,
  `bake.highPoly.add`, `bake.cage.set`, `material.link.*`); bakes (`bake.run`, `bake.preview`);
  and the layer and effect clipboard (`layer.copy`, `layer.cut`, `layer.paste`,
  `layer.pasteInstance`, `effect.copy`, `effect.cut`, `effect.paste`).
- Parameters that name files (`path`, `paths`, `dir`, `folder`, `files`) are refused: a plug-in
  works with the resources already in the project. It gets no disk, network, clock or other
  project.
- Automation confined to folders (MCP and RPC servers with roots) installs from `data`,
  repositories and `https://` catalogs only.

Not yet: plug-in panels (`ui_show` runs report that TextureCraft doesn't show them) and offering
files to save (`saveFile`).

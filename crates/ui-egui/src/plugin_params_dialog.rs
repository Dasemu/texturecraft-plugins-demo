//! The parameter dialog generated from a plug-in command's `params`.

use serde_json::{Map, Value, json};
use texturecraft_engine::plugin::craft_plugin::{Param, ParamKind, PluginCommand};

/// Width of a text parameter's field (points).
const TEXT_FIELD_WIDTH: f32 = 240.0;

/// A plug-in command waiting for its parameters.
#[derive(Clone, Debug)]
pub struct PluginParamsDialog {
    pub plugin: String,
    pub command: String,
    pub title: String,
    pub params: Vec<Param>,
    pub values: Map<String, Value>,
}

impl PluginParamsDialog {
    /// The dialog for `command` of plug-in `plugin` (named `name`), with each parameter's default.
    pub fn new(plugin: &str, name: &str, command: &PluginCommand) -> Self {
        PluginParamsDialog {
            plugin: plugin.to_string(),
            command: command.id.clone(),
            title: format!("{name}: {}", command.label.trim_end_matches(['…', '.'])),
            params: command.params.clone(),
            values: command.params.iter().map(|p| (p.name.clone(), p.default_value())).collect(),
        }
    }

    /// One row per parameter.
    pub fn body(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("plugin_params").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            for p in &self.params {
                ui.label(p.title());
                let v = self.values.entry(p.name.clone()).or_insert_with(|| p.default_value());
                field(ui, p, v);
                ui.end_row();
            }
        });
    }

    /// The `plugin.run` parameters with the values entered.
    pub fn run_params(&self) -> Value {
        json!({"id": self.plugin, "command": self.command, "params": self.values})
    }
}

/// One parameter's control, editing `v`.
fn field(ui: &mut egui::Ui, p: &Param, v: &mut Value) {
    match p.kind {
        ParamKind::Number | ParamKind::Slider => {
            let mut n = v.as_f64().unwrap_or(0.0);
            let changed = match (p.kind, p.min, p.max) {
                (ParamKind::Slider, Some(lo), Some(hi)) if lo < hi => ui.add(egui::Slider::new(&mut n, lo..=hi)).changed(),
                _ => {
                    let mut drag = egui::DragValue::new(&mut n);
                    if let (Some(lo), Some(hi)) = (p.min, p.max)
                        && lo <= hi
                    {
                        drag = drag.range(lo..=hi);
                    }
                    ui.add(drag).changed()
                }
            };
            if changed {
                *v = json!(n);
            }
        }
        ParamKind::Boolean => {
            let mut b = v.as_bool().unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                *v = json!(b);
            }
        }
        ParamKind::Select | ParamKind::List => {
            let current = v.as_str().unwrap_or_default().to_string();
            egui::ComboBox::from_id_salt(("plugin_param", &p.name)).selected_text(current.clone()).show_ui(ui, |ui| {
                for o in &p.options {
                    if ui.selectable_label(*o == current, o.as_str()).clicked() {
                        *v = json!(o);
                    }
                }
            });
        }
        // Text, and a colour as `#RRGGBB`
        _ => {
            let mut s = v.as_str().unwrap_or_default().to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(TEXT_FIELD_WIDTH)).changed() {
                *v = json!(s);
            }
        }
    }
}

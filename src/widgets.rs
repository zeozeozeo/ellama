use eframe::egui;
use ollama_rs::models::{LocalModel, ModelInfo};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SelectedModel {
    pub name: String,
    #[serde(default)]
    modified_ago: String,
    modified_at: String,
    size: u64,
}

impl From<LocalModel> for SelectedModel {
    fn from(model: LocalModel) -> Self {
        let ago = chrono::DateTime::parse_from_rfc3339(&model.modified_at)
            .map(|time| timeago::Formatter::new().convert_chrono(time, chrono::Utc::now()))
            .unwrap_or_else(|e| e.to_string());
        Self {
            name: model.name,
            modified_ago: ago,
            modified_at: model.modified_at,
            size: model.size,
        }
    }
}

#[derive(Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct ModelPicker {
    pub selected: SelectedModel,
    pub info: Option<ModelInfo>,
}

impl ModelPicker {
    pub fn show<R>(&mut self, ui: &mut egui::Ui, models: Option<&[LocalModel]>, request_info: R)
    where
        R: FnOnce(&str),
    {
        if let Some(models) = models {
            egui::ComboBox::from_id_source("model_selector_combobox")
                .selected_text(&self.selected.name)
                .show_ui(ui, |ui| {
                    for model in models {
                        if ui
                            .selectable_label(self.selected.name == model.name, &model.name)
                            .clicked()
                        {
                            self.selected = model.clone().into();
                            self.info = None;
                        }
                    }
                });
        } else {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Loading model list…");
            });
        }

        if self.selected.name.is_empty() {
            return;
        }
        ui.separator();

        egui::Grid::new("selected_model_info_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Size");
                ui.label(format!("{}", bytesize::ByteSize(self.selected.size)))
                    .on_hover_text(format!("{} bytes", self.selected.size));
                ui.end_row();

                ui.label("Modified");
                ui.add(egui::Label::new(&self.selected.modified_ago).truncate(true))
                    .on_hover_text(&self.selected.modified_at);
                ui.end_row();
            });

        if let Some(info) = &self.info {
            for (heading, mut text) in [
                ("License", info.license.as_str()),
                ("Modelfile", info.modelfile.as_str()),
                ("Parameters", info.parameters.as_str()),
                ("Template", info.template.as_str()),
            ] {
                if !text.is_empty() {
                    ui.collapsing(heading, |ui| {
                        ui.code_editor(&mut text);
                    });
                }
            }
        } else {
            request_info(&self.selected.name);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Loading model info…");
            });
        }
    }

    pub fn on_new_model_info(&mut self, name: &str, info: &ModelInfo) {
        if self.selected.name == name {
            self.info = Some(info.clone());
        }
    }
}

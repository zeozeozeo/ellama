use eframe::egui::{self, Color32};
use ollama_rs::models::{LocalModel, ModelInfo};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SelectedModel {
    pub name: String,
    #[serde(default)]
    pub short_name: String,
    #[serde(default)]
    modified_ago: String,
    modified_at: String,
    size: u64,
}

/// Convert a model name into a short name.
///
/// # Example
///
/// - nous-hermes2:latest -> Nous
/// - gemma:latest -> Gemma
/// - starling-lm:7b-beta-q5_K_M -> Starling
fn make_short_name(name: &str) -> String {
    let mut c = name.chars().take_while(|c| c.is_alphanumeric());
    match c.next() {
        None => "Llama".to_string(),
        Some(f) => f.to_uppercase().collect::<String>() + c.collect::<String>().as_str(),
    }
}

impl From<LocalModel> for SelectedModel {
    fn from(model: LocalModel) -> Self {
        let ago = chrono::DateTime::parse_from_rfc3339(&model.modified_at)
            .map(|time| timeago::Formatter::new().convert_chrono(time, chrono::Utc::now()))
            .unwrap_or_else(|e| e.to_string());
        Self {
            short_name: make_short_name(&model.name),
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

pub enum RequestInfoType<'a> {
    Models,
    ModelInfo(&'a str),
}

impl ModelPicker {
    pub fn show<R>(&mut self, ui: &mut egui::Ui, models: Option<&[LocalModel]>, mut request_info: R)
    where
        R: FnMut(RequestInfoType),
    {
        if let Some(models) = models {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_source("model_selector_combobox")
                    .selected_text(&self.selected.name)
                    .show_ui(ui, |ui| {
                        for model in models {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(self.selected.name == model.name, &model.name)
                                    .clicked()
                                {
                                    self.selected = model.clone().into();
                                    self.info = None;
                                }
                                // TODO: make this stick to the right
                                ui.add_enabled(
                                    false,
                                    egui::Label::new(format!("{}", bytesize::ByteSize(model.size))),
                                );
                            });
                        }
                        if models.is_empty() {
                            ui.label("No models found, is the server running?");
                        }
                    });
                if ui
                    .add(egui::Button::new("⟳").small().fill(Color32::TRANSPARENT))
                    .clicked()
                {
                    request_info(RequestInfoType::Models);
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Loading model list…");
            });
        }

        if !self.has_selection() {
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
            request_info(RequestInfoType::ModelInfo(&self.selected.name));
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

    pub fn select_best_model(&mut self, models: &[LocalModel]) {
        models
            .iter()
            .max_by_key(|m| m.size)
            .map(|m| self.selected = m.clone().into());
        if self.has_selection() {
            log::info!("subjectively selected best model: {}", self.selected.name);
        }
    }

    #[inline]
    pub fn has_selection(&self) -> bool {
        !self.selected.name.is_empty()
    }
}

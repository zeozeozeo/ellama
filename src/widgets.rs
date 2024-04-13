use eframe::{
    egui::{self, Color32},
    emath::Numeric,
};
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
    settings: ModelSettings,
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
                    .on_hover_text("Refresh model list")
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

        ui.collapsing("Settings", |ui| {
            self.settings.show(ui);
        });

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

#[derive(Default, Clone, serde::Deserialize, serde::Serialize)]
struct ModelSettings {
    /// Enable Mirostat sampling for controlling perplexity. (default: 0, 0 = disabled, 1 = Mirostat, 2 = Mirostat 2.0)
    pub mirostat: Option<u8>,
    /// Influences how quickly the algorithm responds to feedback from the generated text. A lower learning rate will result in slower adjustments, while a higher learning rate will make the algorithm more responsive. (Default: 0.1)
    pub mirostat_eta: Option<f32>,
    /// Controls the balance between coherence and diversity of the output. A lower value will result in more focused and coherent text. (Default: 5.0)
    pub mirostat_tau: Option<f32>,
    /// Sets the size of the context window used to generate the next token. (Default: 2048)
    pub num_ctx: Option<u32>,
    /// The number of GQA groups in the transformer layer. Required for some models, for example it is 8 for llama2:70b
    pub num_gqa: Option<u32>,
    /// The number of layers to send to the GPU(s). On macOS it defaults to 1 to enable metal support, 0 to disable.
    pub num_gpu: Option<u32>,
    /// Sets the number of threads to use during computation. By default, Ollama will detect this for optimal performance. It is recommended to set this value to the number of physical CPU cores your system has (as opposed to the logical number of cores).
    pub num_thread: Option<u32>,
    /// Sets how far back for the model to look back to prevent repetition. (Default: 64, 0 = disabled, -1 = num_ctx)
    pub repeat_last_n: Option<i32>,
    /// Sets how strongly to penalize repetitions. A higher value (e.g., 1.5) will penalize repetitions more strongly, while a lower value (e.g., 0.9) will be more lenient. (Default: 1.1)
    pub repeat_penalty: Option<f32>,
    /// The temperature of the model. Increasing the temperature will make the model answer more creatively. (Default: 0.8)
    pub temperature: Option<f32>,
    /// Sets the random number seed to use for generation. Setting this to a specific number will make the model generate the same text for the same prompt. (Default: 0)
    pub seed: Option<i32>,
    /// Sets the stop sequences to use. When this pattern is encountered the LLM will stop generating text and return. Multiple stop patterns may be set by specifying multiple separate `stop` parameters in a modelfile.
    pub stop: Option<Vec<String>>,
    /// Tail free sampling is used to reduce the impact of less probable tokens from the output. A higher value (e.g., 2.0) will reduce the impact more, while a value of 1.0 disables this setting. (default: 1)
    pub tfs_z: Option<f32>,
    /// Maximum number of tokens to predict when generating text. (Default: 128, -1 = infinite generation, -2 = fill context)
    pub num_predict: Option<i32>,
    /// Reduces the probability of generating nonsense. A higher value (e.g. 100) will give more diverse answers, while a lower value (e.g. 10) will be more conservative. (Default: 40)
    pub top_k: Option<u32>,
    /// Works together with top-k. A higher value (e.g., 0.95) will lead to more diverse text, while a lower value (e.g., 0.5) will generate more focused and conservative text. (Default: 0.9)
    pub top_p: Option<f32>,
}

impl ModelSettings {
    /// Default settings
    fn default_set() -> Self {
        Self {
            mirostat: Some(0),
            mirostat_eta: Some(0.1),
            mirostat_tau: Some(5.0),
            num_ctx: Some(2048),
            num_gqa: Some(8),
            num_gpu: Some(1),
            num_thread: Some(0),
            repeat_last_n: Some(64),
            repeat_penalty: Some(1.1),
            temperature: Some(0.8),
            seed: Some(0),
            stop: Some(Vec::new()),
            tfs_z: Some(1.0),
            num_predict: Some(128),
            top_k: Some(40),
            top_p: Some(0.9),
        }
    }

    fn edit_numeric<N: Numeric>(
        ui: &mut egui::Ui,
        val: &mut Option<N>,
        mut default: N,
        speed: f64,
        name: &str,
        doc: &str,
    ) {
        ui.collapsing(name, |ui| {
            ui.label(doc);
            let mut enabled = val.is_some();
            ui.checkbox(&mut enabled, "Enable");

            if !enabled {
                *val = None;
            } else if val.is_none() {
                *val = Some(default);
            }

            ui.add_enabled_ui(val.is_some(), |ui| {
                ui.horizontal(|ui| {
                    if let Some(val) = val {
                        ui.add(egui::DragValue::new(val).speed(speed));
                    } else {
                        ui.add(egui::DragValue::new(&mut default).speed(speed));
                    }
                    if ui
                        .button("max")
                        .on_hover_text("Set maximum value")
                        .clicked()
                    {
                        *val = Some(N::MAX);
                    }
                    if ui
                        .button("min")
                        .on_hover_text("Set minimum value")
                        .clicked()
                    {
                        *val = Some(N::MIN);
                    }
                    if ui
                        .button("reset")
                        .on_hover_text("Set default value")
                        .clicked()
                    {
                        *val = None;
                    }
                });
            });
        });
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        // ollama_rs::generation::options::GenerationOptions;
        Self::edit_numeric(
            ui,
            &mut self.mirostat,
            0,
            1.0,
            "Microstat",
            "Enable Mirostat sampling for controlling perplexity.",
        );
        Self::edit_numeric(
            ui,
            &mut self.mirostat_eta,
            0.1,
            0.01,
            "Microstat eta",
            "Influences how quickly the algorithm responds to feedback from the generated text.",
        );

        Self::edit_numeric(
            ui,
            &mut self.num_gqa,
            8,
            1.0,
            "Number of GQA Groups",
            "The number of GQA groups in the transformer layer. Required for some models.",
        );
        Self::edit_numeric(
            ui,
            &mut self.num_gpu,
            1,
            1.0,
            "Number of GPUs",
            "The number of layers to send to the GPU(s).",
        );
        Self::edit_numeric(
            ui,
            &mut self.num_thread,
            0,
            1.0,
            "Number of Threads",
            "Sets the number of threads to use during computation.",
        );
        Self::edit_numeric(
            ui,
            &mut self.repeat_last_n,
            64,
            1.0,
            "Repeat Last N",
            "Sets how far back for the model to look back to prevent repetition.",
        );
        Self::edit_numeric(
            ui,
            &mut self.repeat_penalty,
            1.1,
            0.1,
            "Repeat Penalty",
            "Sets how strongly to penalize repetitions.",
        );
        Self::edit_numeric(ui, &mut self.temperature, 0.8, 0.1, "Temperature", "The temperature of the model. Increasing the temperature will make the model answer more creatively.");
        Self::edit_numeric(
            ui,
            &mut self.seed,
            0,
            1.0,
            "Seed",
            "Sets the random number seed to use for generation.",
        );

        //ui.collapsing("Stop Sequences", |ui| {
        //    ui.label(
        //        "When this pattern is encountered the LLM will stop generating text and return.",
        //    );
        //    if let Some(ref mut stop_patterns) = self.stop {
        //        for (i, pattern) in stop_patterns.iter().enumerate() {
        //            ui.text_input(&mut stop_patterns[i])
        //                .label(format!("Stop {}", i + 1));
        //        }
        //        ui.add(
        //            egui::Button::new("Add Stop Pattern")
        //                .on_click(|| stop_patterns.push(String::new())),
        //        );
        //    }
        //});

        Self::edit_numeric(
            ui,
            &mut self.tfs_z,
            1.0,
            0.1,
            "Tail-Free Sampling Z",
            "Used to reduce the impact of less probable tokens from the output.",
        );
        Self::edit_numeric(
            ui,
            &mut self.num_predict,
            128,
            1.0,
            "Number to Predict",
            "Maximum number of tokens to predict when generating text.",
        );
        Self::edit_numeric(ui, &mut self.top_k, 40, 1.0, "Top K", "Reduces the probability of generating nonsense. A higher value will give more diverse answers.");
        Self::edit_numeric(
            ui,
            &mut self.top_p,
            0.9,
            0.1,
            "Top P",
            "Works together with top-k. A higher value will lead to more diverse text.",
        );
    }
}

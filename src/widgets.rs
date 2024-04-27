use anyhow::Result;
use eframe::{
    egui::{
        self, collapsing_header::CollapsingState, Color32, Frame, Layout, RichText, Rounding,
        Stroke, Vec2,
    },
    emath::Numeric,
};
use egui_modal::{Icon, Modal};
use ollama_rs::{
    generation::options::GenerationOptions,
    models::{LocalModel, ModelInfo},
    Ollama,
};
use url::Url;

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
    settings: ModelSettings,
    pub template: Option<String>,
}

pub enum RequestInfoType<'a> {
    Models,
    ModelInfo(&'a str),
}

fn collapsing_frame<R>(
    ui: &mut egui::Ui,
    heading: &str,
    show: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    let style = ui.style();

    egui::Frame {
        inner_margin: egui::Margin::same(4.0),
        rounding: style.visuals.menu_rounding,
        fill: style.visuals.extreme_bg_color,
        ..egui::Frame::none()
    }
    .show(ui, |ui| {
        ui.with_layout(Layout::top_down_justified(egui::Align::Min), |ui| {
            let mut state = CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id(heading),
                false,
            );

            let resp = ui.add(
                egui::Label::new(heading)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            );
            if resp.clicked() {
                state.toggle(ui);
            }

            state.show_body_unindented(ui, |ui| {
                ui.separator();
                ui.vertical(|ui| {
                    show(ui);
                });
            });

            resp
        });
    })
    .response
}

const TEMPLATE_HINT_TEXT: &str = r#"{{ if .System }}<|im_start|>system
{{ .System }}<|im_end|>
{{ end }}{{ if .Prompt }}<|im_start|>user
{{ .Prompt }}<|im_end|>
{{ end }}<|im_start|>assistant"#;

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

        ui.collapsing("Inference Settings", |ui| {
            self.settings.show(ui, &mut self.template);
        });

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
            ] {
                if !text.is_empty() {
                    collapsing_frame(ui, heading, |ui| {
                        ui.code_editor(&mut text);
                    });
                }
            }

            collapsing_frame(ui, "Template", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("Prompt template to be passed into the model. It may include (optionally) a system message, a user's message and the response from the model. Note: syntax may be model specific. Templates use Go ");
                    ui.spacing_mut().item_spacing.x = 0.0;
                    const TEMPLATE_LINK: &str = "https://pkg.go.dev/text/template";
                    ui.hyperlink_to("template syntax", TEMPLATE_LINK).on_hover_text(TEMPLATE_LINK);
                    ui.label(". This overrides what is defined in the Modelfile. The default template is shown in the Template header.");
                });
                egui::Grid::new("set_template_variable_grid").num_columns(2).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new("Variable").strong()).wrap(true));
                    ui.add(egui::Label::new(RichText::new("Description").strong()).wrap(true));
                    ui.end_row();

                    ui.code("{{ .System }}");
                    ui.add(egui::Label::new("The system message used to specify custom behavior.").wrap(true));
                    ui.end_row();

                    ui.code("{{ .Prompt }}");
                    ui.add(egui::Label::new("The user prompt message.").wrap(true));
                    ui.end_row();

                    ui.code("{{ .Response }}");
                    ui.add(egui::Label::new("The response from the model. When generating a response, text after this variable is omitted.").wrap(true));
                    ui.end_row();
                });

                const DOCS_LINK: &str =
                    "https://github.com/ollama/ollama/blob/main/docs/modelfile.md#template";
                ui.hyperlink_to("Ollama Docmentation", DOCS_LINK)
                    .on_hover_text(DOCS_LINK);

                let mut enabled = self.template.is_some();
                ui.horizontal(|ui| {
                    ui.add(toggle(&mut enabled));
                    ui.label("Override (overrides the template set in the Modelfile)");
                });
                if !enabled {
                    self.template = None;
                } else if self.template.is_none() {
                    self.template = Some(String::new());
                }

                ui.add_enabled_ui(self.template.is_some(), |ui| {
                    if let Some(ref mut template) = self.template {
                        ui.add(
                            egui::TextEdit::multiline(template)
                                .hint_text(TEMPLATE_HINT_TEXT)
                                .code_editor(),
                        );
                    }
                });

                ui.separator();
                ui.label("Modelfile template:");
                ui.code_editor(&mut info.template.as_str());
            });
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
        if let Some(m) = models.iter().max_by_key(|m| m.size) {
            self.selected = m.clone().into();
        }

        if self.has_selection() {
            log::info!("subjectively selected best model: {}", self.selected.name);
        }
    }

    #[inline]
    pub fn has_selection(&self) -> bool {
        !self.selected.name.is_empty()
    }

    #[inline]
    pub fn get_generation_options(&self) -> GenerationOptions {
        self.settings.clone().into()
    }

    #[inline]
    pub fn selected_model(&self) -> String {
        self.selected.name.clone()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum MirostatKind {
    Disabled,
    Mirostat,
    Mirostat2,
}

impl MirostatKind {
    #[inline]
    const fn to_u8(self) -> u8 {
        self as u8
    }

    #[inline]
    const fn name(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Mirostat => "Mirostat",
            Self::Mirostat2 => "Mirostat 2.0",
        }
    }
}

#[derive(Default, Clone, serde::Deserialize, serde::Serialize)]
struct ModelSettings {
    /// Enable Mirostat sampling for controlling perplexity. (default: 0, 0 = disabled, 1 = Mirostat, 2 = Mirostat 2.0)
    pub mirostat: Option<MirostatKind>,
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

impl From<ModelSettings> for GenerationOptions {
    fn from(value: ModelSettings) -> Self {
        let mut s = Self::default();
        if let Some(mirostat) = value.mirostat {
            s = s.mirostat(mirostat.to_u8());
        }
        if let Some(mirostat_eta) = value.mirostat_eta {
            s = s.mirostat_eta(mirostat_eta);
        }
        if let Some(mirostat_tau) = value.mirostat_tau {
            s = s.mirostat_tau(mirostat_tau);
        }
        if let Some(num_ctx) = value.num_ctx {
            s = s.num_ctx(num_ctx);
        }
        if let Some(num_gqa) = value.num_gqa {
            s = s.num_gqa(num_gqa);
        }
        if let Some(num_gpu) = value.num_gpu {
            s = s.num_gpu(num_gpu);
        }
        if let Some(num_thread) = value.num_thread {
            s = s.num_thread(num_thread);
        }
        if let Some(repeat_last_n) = value.repeat_last_n {
            s = s.repeat_last_n(repeat_last_n);
        }
        if let Some(repeat_penalty) = value.repeat_penalty {
            s = s.repeat_penalty(repeat_penalty);
        }
        if let Some(temperature) = value.temperature {
            s = s.temperature(temperature);
        }
        if let Some(seed) = value.seed {
            s = s.seed(seed);
        }
        if let Some(stop) = value.stop {
            s = s.stop(stop);
        }
        if let Some(tfs_z) = value.tfs_z {
            s = s.tfs_z(tfs_z);
        }
        if let Some(num_predict) = value.num_predict {
            s = s.num_predict(num_predict);
        }
        if let Some(top_k) = value.top_k {
            s = s.top_k(top_k);
        }
        if let Some(top_p) = value.top_p {
            s = s.top_p(top_p);
        }
        s
    }
}

#[inline]
pub fn f64_range(range: std::ops::RangeInclusive<f64>) -> f64 {
    fastrand::f64() * (range.end() - range.start()) + range.start()
}

impl ModelSettings {
    fn edit_numeric<N: Numeric>(
        ui: &mut egui::Ui,
        val: &mut Option<N>,
        mut default: N,
        speed: f64,
        name: &str,
        doc: &str,
    ) {
        collapsing_frame(ui, name, |ui: &mut egui::Ui| {
            ui.label(doc);
            let mut enabled = val.is_some();
            ui.horizontal(|ui| {
                ui.add(toggle(&mut enabled));
                ui.label("Enable");
            });

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
                        .button("rand")
                        .on_hover_text("Set random value")
                        .clicked()
                    {
                        *val = Some(N::from_f64(f64_range(N::MIN.to_f64()..=N::MAX.to_f64())));
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

    fn show(&mut self, ui: &mut egui::Ui, template: &mut Option<String>) {
        if ui.button("Reset Settings").clicked() {
            *self = Self::default();
            *template = None;
        }

        collapsing_frame(ui, "Mirostat", |ui| {
            ui.label("Enable Mirostat sampling for controlling perplexity.");

            let mut enabled = self.mirostat.is_some();

            ui.horizontal(|ui| {
                ui.add(toggle(&mut enabled));
                ui.label("Enable");
            });

            if !enabled {
                self.mirostat = None;
            } else if self.mirostat.is_none() {
                self.mirostat = Some(MirostatKind::Disabled);
            }

            ui.add_enabled_ui(self.mirostat.is_some(), |ui| {
                if let Some(mirostat) = self.mirostat {
                    egui::ComboBox::new("mirostat_combobox", "Mirostat")
                        .selected_text(mirostat.name())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.mirostat,
                                Some(MirostatKind::Disabled),
                                "Disabled",
                            );
                            ui.selectable_value(
                                &mut self.mirostat,
                                Some(MirostatKind::Mirostat),
                                "Mirostat",
                            );
                            ui.selectable_value(
                                &mut self.mirostat,
                                Some(MirostatKind::Mirostat2),
                                "Mirostat 2.0",
                            );
                        });
                }
            });
        });

        Self::edit_numeric(ui, &mut self.mirostat_eta, 0.1, 0.01, "Mirostat eta", "Influences how quickly the algorithm responds to feedback from the generated text. A lower learning rate will result in slower adjustments, while a higher learning rate will make the algorithm more responsive.");
        Self::edit_numeric(ui, &mut self.mirostat_tau, 5.0, 0.01, "Mirostat tau", "Controls the balance between coherence and diversity of the output. A lower value will result in more focused and coherent text.");
        Self::edit_numeric(
            ui,
            &mut self.num_ctx,
            2048,
            1.0,
            "Context Window",
            "Sets the size of the context window used to generate the next token.",
        );
        Self::edit_numeric(ui, &mut self.num_gqa, 8, 1.0, "Number of GQA Groups", "The number of GQA groups in the transformer layer. Required for some models, for example it is 8 for llama2:70b.");
        Self::edit_numeric(ui, &mut self.num_gpu, 1, 1.0, "GPU Layers", "The number of layers to send to the GPU(s). On macOS it defaults to 1 to enable metal support, 0 to disable.");
        Self::edit_numeric(ui, &mut self.num_thread, 0, 1.0, "Number of Threads", "Sets the number of threads to use during computation. By default, Ollama will detect this for optimal performance. It is recommended to set this value to the number of physical CPU cores your system has (as opposed to the logical number of cores).");
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
            0.01,
            "Repeat Penalty",
            "Sets how strongly to penalize repetitions. A higher value (e.g., 1.5) will penalize repetitions more strongly, while a lower value (e.g., 0.9) will be more lenient.",
        );
        Self::edit_numeric(ui, &mut self.temperature, 0.8, 0.1, "Temperature", "The temperature of the model. Increasing the temperature will make the model answer more creatively.");
        Self::edit_numeric(ui, &mut self.seed, 0, 1.0, "Seed", "Sets the random number seed to use for generation. Setting this to a specific number will make the model generate the same text for the same prompt.");

        collapsing_frame(ui, "Stop Sequence", |ui| {
            ui.label(
                "Sets the stop sequences to use. \
                When this pattern is encountered the LLM will stop generating text and return.",
            );
            let mut enabled = self.stop.is_some();

            ui.horizontal(|ui| {
                ui.add(toggle(&mut enabled));
                ui.label("Enable");
            });

            if !enabled {
                self.stop = None;
            } else if self.stop.is_none() {
                self.stop = Some(Vec::new());
            }

            ui.add_enabled_ui(self.stop.is_some(), |ui| {
                if let Some(ref mut stop) = self.stop {
                    stop.retain_mut(|pat| {
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(pat);
                            !ui.button("❌").clicked()
                        })
                        .inner
                    });
                    if stop.is_empty() {
                        ui.label("No stop sequences set, add one.");
                    }
                    ui.horizontal(|ui| {
                        if ui.button("➕ Add").clicked() {
                            stop.push(String::new());
                        }
                        if ui.button("Clear").clicked() {
                            stop.clear();
                        }
                    });
                } else {
                    let _ = ui.button("➕ Add");
                }
            });
        });

        Self::edit_numeric(
            ui,
            &mut self.tfs_z,
            1.0,
            0.01,
            "Tail-Free Sampling Z",
            "Tail free sampling is used to reduce the impact \
            of less probable tokens from the output. A higher value (e.g., 2.0) \
            will reduce the impact more, while a value of 1.0 disables this setting.",
        );
        Self::edit_numeric(ui, &mut self.num_predict, 128, 1.0, "Number to Predict", "Maximum number of tokens to predict when generating text. (Default: 128, -1 = infinite generation, -2 = fill context)");
        Self::edit_numeric(ui, &mut self.top_k, 40, 1.0, "Top-K", "Reduces the probability of generating nonsense. A higher value (e.g. 100) will give more diverse answers, while a lower value (e.g. 10) will be more conservative.");
        Self::edit_numeric(ui, &mut self.top_p, 0.9, 0.01, "Top-P", "Works together with top-k. A higher value (e.g., 0.95) will lead to more diverse text, while a lower value (e.g., 0.5) will generate more focused and conservative text.");
    }
}

/// Helper function to center arbitrary widgets. It works by measuring the width of the widgets after rendering, and
/// then using that offset on the next frame.
//
// adapted from https://gist.github.com/juancampa/faf3525beefa477babdad237f5e81ffe
pub fn centerer(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let available_height = ui.available_height();
    ui.horizontal(|ui| {
        let id = ui.id().with("_centerer");
        let last_size: Option<(f32, f32)> = ui.memory_mut(|mem| mem.data.get_temp(id));
        if let Some(last_size) = last_size {
            ui.add_space((ui.available_width() - last_size.0) / 2.0);
        }

        let res = ui
            .vertical(|ui| {
                if let Some(last_size) = last_size {
                    ui.add_space((available_height - last_size.1) / 2.0)
                }
                ui.scope(|ui| {
                    add_contents(ui);
                })
                .response
            })
            .inner;

        let (width, height) = (res.rect.width(), res.rect.height());
        ui.memory_mut(|mem| mem.data.insert_temp(id, (width, height)));

        // Repaint if width changed
        match last_size {
            None => ui.ctx().request_repaint(),
            Some((last_width, last_height)) if last_width != width || last_height != height => {
                ui.ctx().request_repaint()
            }
            Some(_) => {}
        }
    });
}

pub fn suggestion(ui: &mut egui::Ui, text: &str, subtext: &str) -> egui::Response {
    let mut resp = Frame::group(ui.style())
        .rounding(Rounding::same(6.0))
        .stroke(Stroke::NONE)
        .fill(ui.style().visuals.faint_bg_color)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.add(egui::Label::new(text).wrap(false).selectable(false));
                ui.add_enabled(
                    false,
                    egui::Label::new(subtext).wrap(false).selectable(false),
                );
            });
            ui.add_space(ui.available_width());
        })
        .response;

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // for some reason egui sets `Frame::group` to not sense clicks, so we
    // have to hack it here
    resp.clicked = resp.hovered()
        && ui.input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .interact_pos()
                    .map(|p| resp.rect.contains(p))
                    .unwrap_or(false)
        });

    resp
}

pub fn dummy(ui: &mut egui::Ui) {
    ui.add_sized(
        Vec2::ZERO,
        egui::Label::new("").wrap(false).selectable(false),
    );
}

#[inline]
#[must_use]
fn cubic_ease_out(range: std::ops::RangeInclusive<f32>, t: f32) -> f32 {
    let start = *range.start();
    let end = *range.end();
    let t = if t > 1.0 { 1.0 } else { t };
    let value = 1.0 - (1.0 - t).powf(3.0);
    start + (end - start) * value
}

/// taken from https://github.com/emilk/egui/blob/master/crates/egui_demo_lib/src/demo/toggle_switch.rs
fn toggle_ui(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let desired_size = ui.spacing().interact_size.y * egui::vec2(2.0, 1.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, *on, ""));

    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool(response.id, *on);
        let visuals = ui.style().interact_selectable(&response, *on);
        let rect = rect.expand(visuals.expansion);
        let radius = 0.5 * rect.height();
        ui.painter()
            .rect(rect, radius, visuals.bg_fill, visuals.bg_stroke);
        let circle_x = cubic_ease_out((rect.left() + radius)..=(rect.right() - radius), how_on);
        let center = egui::pos2(circle_x, rect.center().y);
        ui.painter()
            .circle(center, 0.75 * radius, visuals.bg_fill, visuals.fg_stroke);
    }

    response
}

#[inline]
fn toggle(on: &mut bool) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| toggle_ui(ui, on)
}

fn help(ui: &mut egui::Ui, text: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        add_contents(ui);
        ui.add_enabled(false, egui::Label::new("(?)").wrap(false).selectable(false))
            .on_disabled_hover_text(text);
    });
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct Settings {
    pub endpoint: String,
    pub model_picker: ModelPicker,
    pub inherit_chat_picker: bool,
    endpoint_error: String,
}

const DEFAULT_HOST: &str = "http://127.0.0.1:11434";

impl Default for Settings {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_HOST.to_owned(),
            model_picker: ModelPicker::default(),
            inherit_chat_picker: true,
            endpoint_error: String::new(),
        }
    }
}

impl Settings {
    fn parse_endpoint(&self) -> Result<Url> {
        let url = url::Url::parse(&self.endpoint)?;
        if !url.has_host() {
            return Err(anyhow::anyhow!("invalid host"));
        }
        Ok(url)
    }

    #[inline]
    pub fn make_ollama(&self) -> Ollama {
        Ollama::from_url(
            self.parse_endpoint()
                .unwrap_or_else(|_| Url::parse(DEFAULT_HOST).unwrap()),
        )
    }

    pub fn show_modal(&mut self, modal: &Modal) {
        modal.show(|ui| {
            modal.title(ui, "Reset Settings");
            modal.frame(ui, |ui| {
                modal.body_and_icon(
                    ui,
                    "Are you sure you want to reset global settings? \
                    This action cannot be undone!",
                    Icon::Warning,
                );
            });
            modal.buttons(ui, |ui| {
                modal.button(ui, "no");
                if modal.caution_button(ui, "yes").clicked() {
                    *self = Self::default();
                }
            });
        });
    }

    async fn ask_save_settings(settings: Self) {
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("JSON file", &["json"])
            .save_file()
            .await
        else {
            log::warn!("no file selected");
            return;
        };

        let Ok(f) = std::fs::File::create(file.path())
            .map_err(|e| log::error!("failed to create file: {e}"))
        else {
            return;
        };

        let _ = serde_json::to_writer_pretty(f, &settings)
            .map_err(|e| log::error!("failed to save settings: {e}"));
    }

    pub fn show<R>(
        &mut self,
        ui: &mut egui::Ui,
        models: Option<&[LocalModel]>,
        request_info: R,
        modal: &Modal,
    ) where
        R: FnMut(RequestInfoType),
    {
        ui.heading("Ollama");
        ui.label("Connection settings");
        egui::Grid::new("settings_grid")
            .num_columns(2)
            .striped(true)
            .min_row_height(32.0)
            .show(ui, |ui| {
                ui.label("Endpoint");
                ui.horizontal(|ui| {
                    let textedit = egui::TextEdit::singleline(&mut self.endpoint)
                        .hint_text(DEFAULT_HOST)
                        .show(ui);
                    if textedit.response.changed() {
                        if let Err(e) = self.parse_endpoint() {
                            self.endpoint_error = e.to_string();
                        } else {
                            self.endpoint_error.clear();
                        }
                    }
                    if self.endpoint != DEFAULT_HOST
                        && ui.button("↺").on_hover_text("Reset to default").clicked()
                    {
                        self.endpoint_error.clear();
                        self.endpoint = DEFAULT_HOST.to_owned();
                    }
                    if !self.endpoint_error.is_empty() {
                        ui.label(
                            RichText::new(&self.endpoint_error).color(ui.visuals().error_fg_color),
                        );
                    }
                });
                ui.end_row();
            });

        ui.separator();

        ui.heading("Model");
        ui.label("Default model for new chats");
        ui.horizontal(|ui| {
            ui.add(toggle(&mut self.inherit_chat_picker));
            help(ui, "Inherit model changes from chats", |ui| {
                ui.label("Inherit from chats");
            });
        });
        ui.add_space(2.0);
        self.model_picker.show(ui, models, request_info);

        ui.separator();

        ui.heading("Miscellaneous");
        ui.label("Reset global settings to defaults");
        if ui.button("Reset").clicked() {
            modal.open();
        }

        ui.label("Save and load settings as JSON");
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                let settings = self.clone();
                tokio::spawn(async move {
                    Self::ask_save_settings(settings).await;
                });
            }
            if ui.button("Load").clicked() {}
        });
    }
}

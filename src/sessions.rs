use crate::chat::Chat;
use eframe::egui::{self, Color32, Frame, Layout, RichText, Rounding, Stroke};
use egui_commonmark::CommonMarkCache;
use egui_modal::{Icon, Modal};
use flowync::{CompactFlower, CompactHandle};
use ollama_rs::{
    models::{LocalModel, ModelInfo},
    Ollama,
};
use parking_lot::RwLock;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tts::Tts;

#[derive(Default, PartialEq)]
enum SessionTab {
    #[default]
    Chats,
    Model,
}

pub type SharedTts = Option<Arc<RwLock<Tts>>>;

enum OllamaResponse {
    Models(Vec<LocalModel>),
    ModelInfo(ModelInfo),
}

#[derive(Default, PartialEq, Eq)]
enum OllamaFlowerActivity {
    /// Idle, default
    #[default]
    Idle,
    /// List models
    ListModels,
    /// Get model info
    ModelInfo,
}

// <progress, response, (error, autorefresh)>
type OllamaFlower = CompactFlower<(), OllamaResponse, (String, bool)>;
type OllamaFlowerHandle = CompactHandle<(), OllamaResponse, (String, bool)>;

#[derive(Default)]
struct SelectedModel {
    name: String,
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

pub struct Sessions {
    tab: SessionTab,
    chats: Vec<Chat>,
    selected_chat: usize,
    chat_marked_for_deletion: usize,
    is_speaking: bool,
    tts: SharedTts,
    commonmark_cache: CommonMarkCache,
    flower: OllamaFlower,
    models: Vec<LocalModel>,
    models_error: String,
    flower_activity: OllamaFlowerActivity,
    selected_model: SelectedModel,
    model_info: Option<ModelInfo>,
    last_model_refresh: Instant,
    last_request_time: Instant,
    is_auto_refresh: bool,
}

impl Default for Sessions {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            tab: SessionTab::Chats,
            chats: vec![Chat::default()],
            selected_chat: 0,
            chat_marked_for_deletion: 0,
            is_speaking: false,
            tts: Tts::default()
                .map_err(|e| log::error!("failed to initialize TTS: {e}"))
                .map(|tts| Arc::new(RwLock::new(tts)))
                .ok(),
            commonmark_cache: CommonMarkCache::default(),
            flower: OllamaFlower::new(1),
            models: Vec::new(),
            models_error: String::new(),
            flower_activity: OllamaFlowerActivity::default(),
            selected_model: SelectedModel::default(),
            model_info: None,
            last_model_refresh: now,
            last_request_time: now,
            is_auto_refresh: true,
        }
    }
}

async fn list_local_models(ollama: Ollama, handle: &OllamaFlowerHandle, is_auto_refresh: bool) {
    log::debug!("requesting local models... (auto-refresh: {is_auto_refresh})");
    match ollama.list_local_models().await {
        Ok(models) => {
            log::debug!("{} local models: {models:?}", models.len());
            handle.success(OllamaResponse::Models(models));
        }
        Err(e) => {
            log::error!("failed to list local models: {e} (auto-refresh: {is_auto_refresh})");
            handle.error((e.to_string(), is_auto_refresh));
        }
    }
}

async fn request_model_info(ollama: Ollama, model_name: String, handle: &OllamaFlowerHandle) {
    match ollama.show_model_info(model_name).await {
        Ok(info) => {
            log::debug!("model info: {info:?}");
            handle.success(OllamaResponse::ModelInfo(info));
        }
        Err(e) => {
            log::error!("failed to request model info: {e}");
            handle.error((e.to_string(), false));
        }
    }
}

impl Sessions {
    pub fn new(ollama: Ollama) -> Self {
        let mut sessions = Self::default();
        sessions.list_models(ollama, false);
        sessions
    }

    fn list_models(&mut self, ollama: Ollama, is_auto_refresh: bool) {
        let handle = self.flower.handle();
        self.flower_activity = OllamaFlowerActivity::ListModels;
        self.is_auto_refresh = is_auto_refresh;
        self.last_request_time = Instant::now();
        self.last_model_refresh = self.last_request_time;
        tokio::spawn(async move {
            handle.activate();
            list_local_models(ollama, &handle, is_auto_refresh).await;
        });
    }

    fn request_model_info(&mut self, ollama: Ollama) {
        let handle = self.flower.handle();
        let model_name = self.selected_model.name.clone();
        self.flower_activity = OllamaFlowerActivity::ModelInfo;
        self.model_info = None;
        self.models_error.clear();
        self.last_request_time = Instant::now();
        tokio::spawn(async move {
            handle.activate();
            request_model_info(ollama, model_name, &handle).await;
        });
    }

    pub fn show(&mut self, ctx: &egui::Context, ollama: &Ollama) {
        // check if tts stopped speaking
        let prev_is_speaking = self.is_speaking;
        self.is_speaking = if let Some(tts) = &self.tts {
            tts.read().is_speaking().unwrap_or(false)
        } else {
            false
        };

        // if speaking, continuously check if stopped
        let mut request_repaint = self.is_speaking;

        let mut modal = Modal::new(ctx, "sessions_main_modal");
        let mut chat_modal = Modal::new(ctx, "chat_main_modal").with_close_on_outside_click(true);

        // show dialogs created on the previous frame, if we move this into the end of the function
        // it won't be located in the center of the window but in the center of the centralpanel instead
        modal.show_dialog();
        chat_modal.show_dialog();

        let avail_width = ctx.available_rect().width();
        egui::SidePanel::left("sessions_panel")
            .resizable(true)
            .max_width(avail_width * 0.5)
            .show(ctx, |ui| {
                self.show_left_panel(ui, ollama);
                ui.allocate_space(ui.available_size());
            });

        // poll all flowers
        for chat in self.chats.iter_mut() {
            if chat.flower_active() {
                request_repaint = true;
                chat.poll_flower(&mut chat_modal);
            }
        }
        if self.flower.is_active() {
            request_repaint = true;
            self.poll_ollama_flower(&modal);
        }

        if request_repaint {
            ctx.request_repaint();
        }

        self.chats[self.selected_chat].show(
            ctx,
            ollama,
            self.tts.clone(),
            prev_is_speaking && !self.is_speaking, // stopped_talking
            &mut self.commonmark_cache,
            self.selected_model.name.clone(),
        );
    }

    fn show_left_panel(&mut self, ui: &mut egui::Ui, ollama: &Ollama) {
        ui.add_space(ui.style().spacing.window_margin.top);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, SessionTab::Chats, "Chats");
            ui.selectable_value(&mut self.tab, SessionTab::Model, "Model");
        });

        ui.add_space(8.0);

        match self.tab {
            SessionTab::Chats => {
                let modal = Modal::new(ui.ctx(), "left_panel_chats_modal");
                self.show_chats(ui, &modal);
                modal.show(|ui| {
                    modal.title(ui, "Remove Chat");
                    modal.frame(ui, |ui| {
                        modal.body_and_icon(
                            ui,
                            "Do you really want to remove this chat? \
                            You cannot undo this action later.\n\
                            Hold Shift to surpass this warning.",
                            Icon::Warning,
                        );
                        modal.buttons(ui, |ui| {
                            if ui.button("No").clicked() {
                                modal.close();
                            }
                            if ui.button("Yes").clicked() {
                                modal.close();
                                self.remove_chat(self.chat_marked_for_deletion);
                            }
                        });
                    });
                });
            }
            SessionTab::Model => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_model_tab(ui, ollama);
                });
            }
        }
    }

    fn poll_ollama_flower(&mut self, modal: &Modal) {
        self.flower.extract(|()| ()).finalize(|resp| {
            match resp {
                Ok(OllamaResponse::Models(models)) => {
                    self.models = models;
                    self.last_model_refresh = Instant::now();
                }
                Ok(OllamaResponse::ModelInfo(info)) => {
                    self.model_info = Some(info);
                }
                Err(flowync::error::Compact::Suppose((e, is_auto_refresh))) => {
                    self.models_error = e.clone();
                    if !is_auto_refresh {
                        modal
                            .dialog()
                            .with_icon(Icon::Error)
                            .with_title("Ollama request failed")
                            .with_body(e)
                            .open();
                    }
                }
                Err(flowync::error::Compact::Panicked(e)) => {
                    log::error!("task panicked: {e}");
                    self.models_error = format!("Task panicked: {e}");
                    modal
                        .dialog()
                        .with_icon(Icon::Error)
                        .with_title("Ollama request task panicked")
                        .with_body(self.models_error.clone())
                        .open();
                }
            };
            self.is_auto_refresh = false;
        });
    }

    fn show_model_tab(&mut self, ui: &mut egui::Ui, ollama: &Ollama) {
        if !self.models_error.is_empty() {
            ui.label(
                RichText::new(" Error! ")
                    .strong()
                    .background_color(Color32::RED)
                    .color(Color32::WHITE),
            );
            ui.colored_label(Color32::RED, &self.models_error);
            if ui.button("⟳ Retry").clicked() {
                self.models_error.clear();
                self.list_models(ollama.clone(), false);
                if !self.selected_model.name.is_empty() {
                    self.request_model_info(ollama.clone());
                }
            }
            if self.models.is_empty() {
                return;
            }
            ui.separator();
        }

        let active = self.flower.is_active();
        if active
            && self.flower_activity == OllamaFlowerActivity::ListModels
            && !self.is_auto_refresh
        {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Loading model list…");
                ui.add_enabled(
                    false,
                    egui::Label::new(format!(
                        "{:.1}s",
                        self.last_request_time.elapsed().as_secs_f64()
                    )),
                );
            });
        } else {
            ui.label("Default model used for new chats.");
            let mut changed = false;
            egui::ComboBox::new("model_selector_combobox", "Model")
                .selected_text(&self.selected_model.name)
                .show_ui(ui, |ui| {
                    for model in &self.models {
                        if ui
                            .selectable_label(self.selected_model.name == model.name, &model.name)
                            .clicked()
                        {
                            self.selected_model = model.clone().into();
                            changed = true;
                        }
                    }
                });
            if changed {
                self.request_model_info(ollama.clone());
            }
        }

        let loading_model_info = active && self.flower_activity == OllamaFlowerActivity::ModelInfo;
        if self.model_info.is_some() || loading_model_info {
            ui.separator();
        }
        if loading_model_info {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Loading model info…");
                ui.add_enabled(
                    false,
                    egui::Label::new(format!(
                        "{:.1}s",
                        self.last_request_time.elapsed().as_secs_f64()
                    )),
                );
            });
        }

        {
            const REFRESH_DURATION: Duration = Duration::from_secs(10);
            let refresh_elapsed = self.last_model_refresh.elapsed();
            if !ui.ctx().has_requested_repaint() {
                ui.ctx().request_repaint_after(REFRESH_DURATION);
            }
            if refresh_elapsed > REFRESH_DURATION {
                self.list_models(ollama.clone(), true);
            }
        }

        // selected model info grid
        if !self.selected_model.name.is_empty() {
            egui::Grid::new("selected_model_info_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Size");
                    ui.label(format!("{}", bytesize::ByteSize(self.selected_model.size)))
                        .on_hover_text(format!("{} bytes", self.selected_model.size));
                    ui.end_row();

                    ui.label("Modified");
                    ui.add(egui::Label::new(&self.selected_model.modified_ago).truncate(true))
                        .on_hover_text(&self.selected_model.modified_at);
                    ui.end_row();
                });
        }

        if let Some(info) = &self.model_info {
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
        }
    }

    #[inline]
    fn add_default_chat(&mut self) {
        // id 1 is already used, and we (probably) don't want to reuse ids for flowers
        self.chats.push(Chat::new(self.chats.len() + 2));
    }

    fn remove_chat(&mut self, idx: usize) {
        self.chats.remove(idx);
        if self.chats.is_empty() {
            self.add_default_chat();
            self.selected_chat = 0;
        } else if self.selected_chat >= self.chats.len() {
            self.selected_chat = self.chats.len() - 1;
        }
    }

    /// Returns whether any chat was removed
    fn show_chat_frame(&mut self, ui: &mut egui::Ui, idx: usize, modal: &Modal) -> bool {
        let Some(chat) = &self.chats.get(idx) else {
            return false;
        };
        let mut chat_removed = false;

        let last_message = chat
            .last_message_contents()
            .unwrap_or_else(|| "No recent messages".to_string());

        let summary = chat.summary.clone();

        ui.horizontal(|ui| {
            ui.add(if summary.is_empty() {
                egui::Label::new("New Chat")
                    .selectable(false)
                    .truncate(true)
            } else {
                egui::Label::new(summary).selectable(false)
            });
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new("❌")
                            .small()
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                    )
                    .on_hover_text("Remove chat")
                    .clicked()
                {
                    if ui.input(|i| i.modifiers.shift) {
                        self.remove_chat(idx);
                    } else {
                        self.chat_marked_for_deletion = idx;
                        modal.open();
                    }
                    chat_removed = true;
                }
                if ui
                    .add(
                        egui::Button::new("Edit")
                            .small()
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                    )
                    .on_hover_text("Edit")
                    .clicked()
                {}
            });
        });

        ui.add_enabled(
            false,
            egui::Label::new(last_message)
                .selectable(false)
                .truncate(true),
        );
        chat_removed
    }

    /// Returns whether the chat should be selected as the current one
    fn show_chat_in_sidepanel(&mut self, ui: &mut egui::Ui, idx: usize, modal: &Modal) -> bool {
        let mut chat_removed = false;
        let resp = Frame::group(ui.style())
            .rounding(Rounding::same(6.0))
            .stroke(Stroke::new(2.0, ui.style().visuals.window_stroke.color))
            .fill(if self.selected_chat == idx {
                ui.style().visuals.faint_bg_color
            } else {
                ui.style().visuals.window_fill
            })
            .show(ui, |ui| {
                chat_removed = self.show_chat_frame(ui, idx, modal);
            })
            .response;

        // very hacky way to determine if the group has been clicked, for some reason
        // egui doens't register clicked() events on it
        let (primary_clicked, hovered) = ui.input(|i| {
            (
                i.pointer.primary_clicked(),
                i.pointer
                    .interact_pos()
                    .map(|p| resp.rect.contains(p))
                    .unwrap_or(false),
            )
        });

        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        !chat_removed && primary_clicked && hovered
    }

    fn show_chats(&mut self, ui: &mut egui::Ui, modal: &Modal) {
        // TODO: use show_rows() instead of show()
        egui::ScrollArea::vertical().show(ui, |ui| {
            if ui.button("➕ New Chat").clicked() {
                self.add_default_chat();
                self.selected_chat = self.chats.len() - 1;
            }
            ui.separator();
            for i in 0..self.chats.len() {
                if self.show_chat_in_sidepanel(ui, i, modal) {
                    self.selected_chat = i;
                }
                ui.add_space(2.0);
            }
        });
    }
}

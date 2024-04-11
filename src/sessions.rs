use crate::{chat::Chat, widgets::ModelPicker};
use eframe::egui::{self, vec2, Color32, Frame, Layout, Rounding, Stroke};
use egui_commonmark::CommonMarkCache;
use egui_modal::{Icon, Modal};
use egui_virtual_list::VirtualList;
use flowync::{CompactFlower, CompactHandle};
use ollama_rs::{
    models::{LocalModel, ModelInfo},
    Ollama,
};
use parking_lot::RwLock;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Instant};
use tts::Tts;

#[derive(Default, PartialEq, serde::Serialize, serde::Deserialize)]
enum SessionTab {
    #[default]
    Chats,
    Model,
}

pub type SharedTts = Option<Arc<RwLock<Tts>>>;

enum OllamaResponse {
    Models(Vec<LocalModel>),
    ModelInfo { name: String, info: ModelInfo },
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

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct SelectedModel {
    name: String,
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

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Sessions {
    tab: SessionTab,
    chats: Vec<Chat>,
    selected_chat: usize,
    #[serde(skip)]
    chat_marked_for_deletion: usize,
    #[serde(skip)]
    is_speaking: bool,
    #[serde(skip)]
    tts: SharedTts,
    #[serde(skip)]
    commonmark_cache: CommonMarkCache,
    #[serde(skip)]
    flower: OllamaFlower,
    #[serde(skip)]
    models: Vec<LocalModel>,
    #[serde(skip)]
    flower_activity: OllamaFlowerActivity,
    #[serde(skip)]
    last_model_refresh: Instant,
    #[serde(skip)]
    last_request_time: Instant,
    #[serde(skip)]
    is_auto_refresh: bool,
    model_picker: ModelPicker,
    #[serde(skip)]
    pending_model_infos: HashMap<String, ()>,
    #[serde(skip)]
    virtual_list: Rc<RefCell<VirtualList>>,
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
            flower_activity: OllamaFlowerActivity::default(),
            last_model_refresh: now,
            last_request_time: now,
            is_auto_refresh: true,
            model_picker: ModelPicker::default(),
            pending_model_infos: HashMap::new(),
            virtual_list: Rc::new(RefCell::new(VirtualList::default())),
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
    match ollama.show_model_info(model_name.clone()).await {
        Ok(info) => {
            log::debug!("model `{model_name}` info: {info:?}");
            handle.success(OllamaResponse::ModelInfo {
                name: model_name,
                info,
            });
        }
        Err(e) => {
            log::error!("failed to request model `{model_name}` info: {e}");
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

    pub fn list_models(&mut self, ollama: Ollama, is_auto_refresh: bool) {
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

    fn request_model_info(&mut self, model_name: String, ollama: Ollama) {
        let handle = self.flower.handle();
        self.flower_activity = OllamaFlowerActivity::ModelInfo;
        self.last_request_time = Instant::now();
        self.pending_model_infos.insert(model_name.clone(), ());
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
                Ok(OllamaResponse::ModelInfo { name, info }) => {
                    self.pending_model_infos.remove(&name);
                    self.model_picker.on_new_model_info(&name, &info);
                    for chat in self.chats.iter_mut() {
                        chat.model_picker.on_new_model_info(&name, &info);
                    }
                }
                Err(flowync::error::Compact::Suppose((e, is_auto_refresh))) => {
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
                    modal
                        .dialog()
                        .with_icon(Icon::Error)
                        .with_title("Ollama request task panicked")
                        .with_body(format!("Task panicked: {e}"))
                        .open();
                }
            };
            self.is_auto_refresh = false;
        });
    }

    fn show_model_tab(&mut self, ui: &mut egui::Ui, ollama: &Ollama) {
        let active = self.flower.is_active();
        let loading_models = active
            && self.flower_activity == OllamaFlowerActivity::ListModels
            && !self.is_auto_refresh;

        let mut request_info_for: Option<String> = None;
        ui.label("Default model for new chats.");
        self.model_picker.show(
            ui,
            if loading_models {
                None
            } else {
                Some(&self.models)
            },
            |name| {
                if !self.pending_model_infos.contains_key(name) {
                    request_info_for = Some(name.to_string());
                }
            },
        );

        if let Some(name) = request_info_for {
            self.request_model_info(name, ollama.clone());
        }
    }

    #[inline]
    fn add_default_chat(&mut self) {
        // id 1 is already used, and we (probably) don't want to reuse ids for flowers
        self.chats
            .push(Chat::new(self.chats.len() + 2, self.model_picker.clone()));
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
        let mut ignore_click = false;

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
                    ignore_click = true;
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
                {
                    ignore_click = true;
                    log::info!("edit");
                }
            });
        });

        ui.add_enabled(
            false,
            egui::Label::new(last_message)
                .selectable(false)
                .truncate(true),
        );
        ignore_click
    }

    /// Returns whether the chat should be selected as the current one
    fn show_chat_in_sidepanel(&mut self, ui: &mut egui::Ui, idx: usize, modal: &Modal) -> bool {
        let mut ignore_click = false;
        let resp = Frame::group(ui.style())
            .rounding(Rounding::same(6.0))
            .stroke(Stroke::new(2.0, ui.style().visuals.window_stroke.color))
            .fill(if self.selected_chat == idx {
                ui.style().visuals.faint_bg_color
            } else {
                ui.style().visuals.window_fill
            })
            .show(ui, |ui| {
                ignore_click = self.show_chat_frame(ui, idx, modal);
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

        !ignore_click && primary_clicked && hovered
    }

    fn show_chats(&mut self, ui: &mut egui::Ui, modal: &Modal) {
        ui.vertical_centered_justified(|ui| {
            if ui
                .add(egui::Button::new("➕ New Chat").min_size(vec2(0.0, 24.0)))
                .on_hover_text("Create a new chat")
                .clicked()
            {
                self.add_default_chat();
                self.selected_chat = self.chats.len() - 1;
            }
        });

        ui.add_space(2.0);

        // TODO: use show_rows() instead of show()
        let vlist = self.virtual_list.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            vlist
                .borrow_mut()
                .ui_custom_layout(ui, self.chats.len(), |ui, i| {
                    if self.show_chat_in_sidepanel(ui, i, modal) {
                        self.selected_chat = i;
                    }
                    ui.add_space(2.0);
                    1
                });
        });
    }
}

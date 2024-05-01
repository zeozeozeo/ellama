use crate::{
    chat::{Chat, ChatAction, ChatExportFormat},
    widgets::{ModelPicker, RequestInfoType, Settings},
};
use eframe::egui::{self, vec2, Color32, Frame, Layout, Rounding, Stroke};
use egui_commonmark::CommonMarkCache;
use egui_modal::{Icon, Modal};
use egui_notify::{Toast, Toasts};
use egui_twemoji::EmojiLabel;
use egui_virtual_list::VirtualList;
use flowync::{CompactFlower, CompactHandle};
use ollama_rs::{
    models::{LocalModel, ModelInfo},
    Ollama,
};
use parking_lot::RwLock;
use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, sync::Arc, time::Instant};
use tts::Tts;

#[derive(Default, PartialEq, serde::Serialize, serde::Deserialize)]
enum SessionTab {
    #[default]
    Chats,
}

pub type SharedTts = Option<Arc<RwLock<Tts>>>;

enum OllamaResponse {
    Ignore,
    Models(Vec<LocalModel>),
    ModelInfo { name: String, info: ModelInfo },
    Toast(Toast),
    Images { id: usize, files: Vec<PathBuf> },
    Settings(Settings),
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

// <progress, response, error>
type OllamaFlower = CompactFlower<(), OllamaResponse, String>;
type OllamaFlowerHandle = CompactHandle<(), OllamaResponse, String>;

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
    last_request_time: Instant,
    #[serde(skip)]
    pending_model_infos: HashMap<String, ()>,
    #[serde(skip)]
    virtual_list: Rc<RefCell<VirtualList>>,
    edited_chat: Option<usize>,
    chat_export_format: ChatExportFormat,
    #[serde(skip)]
    toasts: Toasts,
    settings_open: bool,
    pub settings: Settings,
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
            last_request_time: now,
            pending_model_infos: HashMap::new(),
            virtual_list: Rc::new(RefCell::new(VirtualList::default())),
            edited_chat: None,
            chat_export_format: ChatExportFormat::default(),
            toasts: Toasts::default(),
            settings_open: false,
            settings: Settings::default(),
        }
    }
}

async fn list_local_models(ollama: Ollama, handle: &OllamaFlowerHandle) {
    log::debug!("requesting local models...");
    match ollama.list_local_models().await {
        Ok(models) => {
            log::debug!("{} local models: {models:?}", models.len());
            handle.success(OllamaResponse::Models(models));
        }
        Err(e) => {
            log::error!("failed to list local models: {e}");
            handle.error(e.to_string());
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
            handle.error(e.to_string());
        }
    }
}

async fn pick_images(id: usize, handle: &OllamaFlowerHandle) {
    let Some(files) = rfd::AsyncFileDialog::new()
        .add_filter(
            "Image",
            &[
                "avif", "bmp", "dds", "ff", "gif", "hdr", "ico", "jpeg", "jpg", "exr", "png",
                "pnm", "qoi", "tga", "tiff", "webp",
            ],
        )
        .pick_files()
        .await
    else {
        handle.success(OllamaResponse::Ignore);
        return;
    };

    log::info!("selected {} image(s)", files.len());

    handle.success(OllamaResponse::Images {
        id,
        files: files.iter().map(|f| f.path().to_path_buf()).collect(),
    });
}

async fn load_settings(handle: &OllamaFlowerHandle) {
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("JSON file", &["json"])
        .pick_file()
        .await
    else {
        handle.success(OllamaResponse::Toast(Toast::info("No file selected")));
        return;
    };

    log::info!("reading settings from `{}`", file.path().display());
    let Ok(f) = std::fs::File::open(file.path()).map_err(|e| {
        log::error!("failed to open file `{}`: {e}", file.path().display());
        handle.success(OllamaResponse::Toast(Toast::error(e.to_string())));
    }) else {
        return;
    };

    let settings = serde_json::from_reader(std::io::BufReader::new(f));
    if let Ok(settings) = settings {
        handle.success(OllamaResponse::Settings(settings));
    } else if let Err(e) = settings {
        log::error!("failed to load settings: {e}");
        handle.success(OllamaResponse::Toast(Toast::error(e.to_string())));
    }
}

impl Sessions {
    pub fn new(ollama: Ollama) -> Self {
        let mut sessions = Self::default();
        sessions.list_models(ollama);
        sessions
    }

    pub fn list_models(&mut self, ollama: Ollama) {
        let handle = self.flower.handle();
        self.flower_activity = OllamaFlowerActivity::ListModels;
        self.last_request_time = Instant::now();
        tokio::spawn(async move {
            handle.activate();
            list_local_models(ollama, &handle).await;
        });
    }

    fn request_model_info(&mut self, model_name: String, ollama: Ollama) {
        // check if any chats have the info of this model
        let handle = self.flower.handle();
        for chat in &self.chats {
            if chat.model_picker.selected_model() == model_name {
                if let Some(info) = chat.model_picker.info.clone() {
                    handle.activate();
                    handle.success(OllamaResponse::ModelInfo {
                        name: model_name.clone(),
                        info,
                    });
                    return;
                }
            }
        }

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
        let settings_modal =
            Modal::new(ctx, "global_settings_modal").with_close_on_outside_click(true);

        // if self.edit_modal_open {
        //     let mut open = self.edit_modal_open;
        //     egui::Window::new("Edit Chat")
        //         .collapsible(false)
        //         .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        //         .open(&mut open)
        //         .show(ctx, |ui| {
        //             self.show_edit_modal_inner(ui, ollama);
        //         });
        //     self.edit_modal_open = open;
        // }

        // show dialogs created on the previous frame, if we move this into the end of the function
        // it won't be located in the center of the window but in the center of the centralpanel instead
        chat_modal.show_dialog();
        modal.show_dialog();
        self.settings.show_modal(&settings_modal);

        let avail_width = ctx.available_rect().width();
        egui::SidePanel::left("sessions_panel")
            .resizable(true)
            .max_width(avail_width * 0.5)
            .show(ctx, |ui| {
                self.show_left_panel(ui);
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

        if self.settings_open {
            self.edited_chat = None;
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                    let mut request_info_for: Option<String> = None;
                    let mut list_models = false;

                    self.settings.show(
                        ui,
                        if self.is_loading_models() {
                            None
                        } else {
                            Some(&self.models)
                        },
                        &mut |typ| match typ {
                            RequestInfoType::ModelInfo(name) => {
                                if !self.pending_model_infos.contains_key(name) {
                                    request_info_for = Some(name.to_string());
                                }
                            }
                            RequestInfoType::Models => {
                                list_models = true;
                            }
                            RequestInfoType::LoadSettings => {
                                let handle = self.flower.handle();
                                tokio::spawn(async move {
                                    handle.activate();
                                    load_settings(&handle).await;
                                });
                            }
                        },
                        &settings_modal,
                    );

                    if let Some(name) = request_info_for {
                        self.request_model_info(name, ollama.clone());
                    }
                    if list_models {
                        self.list_models(ollama.clone());
                    }
                });
            });
        } else if let Some(edited_chat) = self.edited_chat {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                    self.show_chat_edit_panel(ui, edited_chat, ollama);
                })
            });
        } else {
            self.show_selected_chat(ctx, ollama, prev_is_speaking && !self.is_speaking)
        }

        // display toast queue
        self.toasts.show(ctx);
    }

    fn show_selected_chat(&mut self, ctx: &egui::Context, ollama: &Ollama, stopped_talking: bool) {
        let action = self.chats[self.selected_chat].show(
            ctx,
            ollama,
            self.tts.clone(),
            stopped_talking,
            &mut self.commonmark_cache,
        );

        match action {
            ChatAction::None => (),
            ChatAction::PickImages { id } => {
                let handle = self.flower.handle();
                tokio::spawn(async move {
                    handle.activate();
                    pick_images(id, &handle).await;
                });
            }
        }
    }

    fn show_remove_chat_modal_inner(&mut self, ui: &mut egui::Ui, modal: &Modal) {
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
                if modal.button(ui, "No").clicked() {
                    modal.close();
                }
                let summary = self
                    .chats
                    .get(self.chat_marked_for_deletion)
                    .map(|c| {
                        if c.summary.is_empty() {
                            "New Chat"
                        } else {
                            c.summary.as_str()
                        }
                    })
                    .unwrap_or("New Chat");
                if modal
                    .caution_button(ui, "Yes")
                    .on_hover_text(format!("Remove chat \"{summary}\"",))
                    .clicked()
                {
                    modal.close();
                    self.remove_chat(self.chat_marked_for_deletion);
                }
            });
        });
    }

    fn show_chat_edit_panel(&mut self, ui: &mut egui::Ui, chat_idx: usize, ollama: &Ollama) {
        ui.horizontal(|ui| {
            let Some(chat) = self.chats.get(chat_idx) else {
                return;
            };
            if chat.summary.is_empty() {
                ui.heading("Editing Chat \"New Chat\"");
            } else {
                ui.heading(format!("Editing Chat \"{}\"", chat.summary));
            }

            ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
                if ui
                    .add(
                        egui::Button::new("❌")
                            .fill(Color32::TRANSPARENT)
                            .frame(false),
                    )
                    .on_hover_text("Close")
                    .clicked()
                {
                    self.edited_chat = None;
                }
            });
        });

        egui::CollapsingHeader::new("Model")
            .default_open(true)
            .show(ui, |ui| {
                let mut request_info_for: Option<String> = None;
                let is_loading_models = self.is_loading_models();
                let Some(chat) = self.chats.get_mut(chat_idx) else {
                    return;
                };
                let mut list_models = false;
                chat.model_picker.show(
                    ui,
                    if is_loading_models {
                        None
                    } else {
                        Some(&self.models)
                    },
                    &mut |typ| match typ {
                        RequestInfoType::ModelInfo(name) => {
                            if !self.pending_model_infos.contains_key(name) {
                                request_info_for = Some(name.to_string());
                            }
                        }
                        RequestInfoType::Models => {
                            list_models = true;
                        }
                        RequestInfoType::LoadSettings => (), // can't be called from here
                    },
                );
                if let Some(name) = request_info_for {
                    if self.settings.inherit_chat_picker
                        && (name != self.settings.model_picker.selected_model())
                    {
                        self.settings.model_picker.selected = chat.model_picker.selected.clone();
                    }

                    self.request_model_info(name, ollama.clone());
                }
                if list_models {
                    self.list_models(ollama.clone());
                }
            });
        ui.collapsing("Export", |ui| {
            ui.label("Export chat history to a file");
            let format = self.chat_export_format;
            egui::ComboBox::from_label("Export Format")
                .selected_text(format.to_string())
                .show_ui(ui, |ui| {
                    for format in ChatExportFormat::ALL {
                        ui.selectable_value(
                            &mut self.chat_export_format,
                            format,
                            format.to_string(),
                        );
                    }
                });
            if ui.button("Save As…").clicked() {
                let task = rfd::AsyncFileDialog::new()
                    .add_filter(format!("{format:?} file"), format.extensions())
                    .save_file();
                let Some(chat) = self.chats.get_mut(chat_idx) else {
                    return;
                };
                let messages = chat.messages.clone();
                let handle = self.flower.handle();
                tokio::spawn(async move {
                    let toast = crate::chat::export_messages(messages, format, task)
                        .await
                        .map_err(|e| {
                            log::error!("failed to export messages: {e}");
                            e
                        });

                    handle.activate();
                    if let Ok(toast) = toast {
                        handle.success(OllamaResponse::Toast(toast))
                    } else if let Err(e) = toast {
                        handle.success(OllamaResponse::Toast(Toast::error(e.to_string())))
                    };
                });
            }
        });
    }

    fn show_left_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(ui.style().spacing.window_margin.top);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, SessionTab::Chats, "Chats");
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut self.settings_open, "⚙")
                    .on_hover_text("Settings");
            });
        });

        ui.add_space(8.0);

        match self.tab {
            SessionTab::Chats => {
                let modal = Modal::new(ui.ctx(), "remove_chat_modal");
                self.show_chats(ui, &modal);
                modal.show(|ui| {
                    self.show_remove_chat_modal_inner(ui, &modal);
                });
            }
        }
    }

    #[inline]
    pub fn model_picker(&self) -> &ModelPicker {
        &self.settings.model_picker
    }

    fn poll_ollama_flower(&mut self, modal: &Modal) {
        self.flower.extract(|()| ()).finalize(|resp| {
            self.flower_activity = OllamaFlowerActivity::Idle;
            match resp {
                Ok(OllamaResponse::Ignore) => (),
                Ok(OllamaResponse::Models(models)) => {
                    self.models = models;
                    if !self.settings.model_picker.has_selection() {
                        self.settings.model_picker.select_best_model(&self.models);

                        // for each chat with unselected models, select the best model
                        for chat in self.chats.iter_mut() {
                            if !chat.model_picker.has_selection() {
                                chat.model_picker.selected =
                                    self.settings.model_picker.selected.clone();
                            }
                        }
                    }
                }
                Ok(OllamaResponse::ModelInfo { name, info }) => {
                    self.pending_model_infos.remove(&name);
                    self.settings.model_picker.on_new_model_info(&name, &info);
                    for chat in self.chats.iter_mut() {
                        chat.model_picker.on_new_model_info(&name, &info);
                    }
                }
                Ok(OllamaResponse::Toast(toast)) => {
                    self.toasts.add(toast);
                }
                Ok(OllamaResponse::Images { id, files }) => {
                    if let Some(chat) = self.chats.iter_mut().find(|c| c.id() == id) {
                        log::debug!("adding {} image(s)", files.len());
                        chat.images.extend(files);
                    }
                }
                Ok(OllamaResponse::Settings(settings)) => {
                    self.settings = settings;
                }
                Err(flowync::error::Compact::Suppose(e)) => {
                    modal
                        .dialog()
                        .with_icon(Icon::Error)
                        .with_title("Ollama request failed")
                        .with_body(e)
                        .open();
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
        });
    }

    #[inline]
    fn is_loading_models(&self) -> bool {
        self.flower.is_active() && self.flower_activity == OllamaFlowerActivity::ListModels
    }

    #[inline]
    fn add_default_chat(&mut self) {
        // id 1 is already used, and we (probably) don't want to reuse ids for flowers
        self.chats
            .push(Chat::new(self.chats.len() + 2, self.model_picker().clone()));
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
            if summary.is_empty() {
                ui.add(
                    egui::Label::new("New Chat")
                        .selectable(false)
                        .truncate(true),
                );
            } else {
                EmojiLabel::new(summary)
                    .selectable(false)
                    .truncate(true)
                    .show(ui);
            }

            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
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
                    if self.chats[idx].messages.is_empty() || ui.input(|i| i.modifiers.shift) {
                        self.remove_chat(idx);
                    } else {
                        self.chat_marked_for_deletion = idx;
                        self.edited_chat = None;
                        modal.open();
                    }
                    ignore_click = true;
                }
                if ui
                    .add(
                        egui::Button::new("\u{270f}")
                            .small()
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                    )
                    .on_hover_text("Edit")
                    .clicked()
                {
                    ignore_click = true;

                    // toggle editing
                    self.edited_chat = if self.edited_chat == Some(idx) {
                        None
                    } else {
                        Some(idx)
                    };
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
        let (primary_clicked, hovered) = if modal.is_open() {
            (false, false)
        } else {
            ui.input(|i| {
                (
                    i.pointer.primary_clicked(),
                    i.pointer
                        .interact_pos()
                        .map(|p| resp.rect.contains(p))
                        .unwrap_or(false),
                )
            })
        };

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

        let vlist = self.virtual_list.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            vlist
                .borrow_mut()
                .ui_custom_layout(ui, self.chats.len(), |ui, i| {
                    if self.show_chat_in_sidepanel(ui, i, modal) {
                        self.selected_chat = i;
                        self.settings_open = false;
                    }
                    ui.add_space(2.0);
                    1
                });
        });
    }
}

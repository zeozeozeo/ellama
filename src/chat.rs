use crate::{easymark::MemoizedEasymarkHighlighter, sessions::SharedTts, widgets::ModelPicker};
use anyhow::{Context, Result};
use eframe::egui::{
    self, pos2, vec2, Align, Color32, Frame, Key, KeyboardShortcut, Layout, Margin, Modifiers,
    Pos2, Rect, Rounding, Stroke,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_modal::{Icon, Modal};
use egui_virtual_list::VirtualList;
use flowync::{error::Compact, CompactFlower, CompactHandle};
use ollama_rs::{
    generation::{
        chat::{request::ChatMessageRequest, ChatMessage, ChatMessageResponseStream},
        options::GenerationOptions,
    },
    Ollama,
};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio_stream::StreamExt;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum Role {
    User,
    Assistant,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Message {
    model_name: String,
    content: String,
    role: Role,
    #[serde(skip)]
    is_generating: bool,
    #[serde(skip)]
    requested_at: Instant,
    time: chrono::DateTime<chrono::Utc>,
    #[serde(skip)]
    clicked_copy: bool,
    is_error: bool,
    #[serde(skip)]
    is_speaking: bool,
    images: Vec<PathBuf>,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            content: String::new(),
            role: Role::User,
            is_generating: false,
            requested_at: Instant::now(),
            time: chrono::Utc::now(),
            clicked_copy: false,
            is_error: false,
            is_speaking: false,
            model_name: String::new(),
            images: Vec::new(),
        }
    }
}

fn tts_control(tts: SharedTts, text: String, speak: bool) {
    std::thread::spawn(move || {
        if let Some(tts) = tts {
            if speak {
                let _ = tts
                    .write()
                    .speak(text, true)
                    .map_err(|e| log::error!("failed to speak: {e}"));
            } else {
                let _ = tts
                    .write()
                    .stop()
                    .map_err(|e| log::error!("failed to stop tts: {e}"));
            }
        }
    });
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

impl Message {
    #[inline]
    fn user(content: String, model_name: String, images: Vec<PathBuf>) -> Self {
        Self {
            content,
            role: Role::User,
            is_generating: false,
            model_name,
            images,
            ..Default::default()
        }
    }

    #[inline]
    fn assistant(content: String, model_name: String) -> Self {
        Self {
            content,
            role: Role::Assistant,
            is_generating: true,
            model_name,
            ..Default::default()
        }
    }

    #[inline]
    const fn is_user(&self) -> bool {
        matches!(self.role, Role::User)
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        commonmark_cache: &mut CommonMarkCache,
        tts: SharedTts,
        idx: usize,
    ) -> bool {
        // message role
        let message_offset = ui
            .horizontal(|ui| {
                if self.is_user() {
                    let f = ui.label("👤").rect.left();
                    ui.label("You").rect.left() - f
                } else {
                    let f = ui.label("🐱").rect.left();
                    let offset = ui
                        .label(make_short_name(&self.model_name))
                        .on_hover_text(&self.model_name)
                        .rect
                        .left()
                        - f;
                    ui.add_enabled(false, egui::Label::new(&self.model_name));
                    offset
                }
            })
            .inner;

        // for some reason commonmark creates empty space above it when created,
        // compensate for that
        let is_commonmark = !self.content.is_empty() && !self.is_error;
        if is_commonmark {
            ui.add_space(-24.0);
        }

        // message content / spinner
        let mut retry = false;
        ui.horizontal(|ui| {
            ui.add_space(message_offset);
            if self.content.is_empty() && self.is_generating && !self.is_error {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());

                    // show time spent waiting for response
                    ui.add_enabled(
                        false,
                        egui::Label::new(format!(
                            "{:.1}s",
                            self.requested_at.elapsed().as_secs_f64()
                        )),
                    )
                });
            } else if self.is_error {
                ui.label("An error occurred while requesting completion");
                retry = ui
                    .button("Retry")
                    .on_hover_text(
                        "Try to generate a response again. Make sure you have Ollama running",
                    )
                    .clicked();
            } else {
                CommonMarkViewer::new(format!("message_{idx}_commonmark"))
                    .max_image_width(Some(512))
                    .show(ui, commonmark_cache, &self.content);
            }
        });

        // images
        if !self.images.is_empty() {
            if is_commonmark {
                ui.add_space(-8.0);
            }
            ui.horizontal(|ui| {
                ui.add_space(message_offset);
                show_images(ui, &self.images);
            });
            ui.add_space(8.0);
        }

        // copy buttons and such
        let shift_held = !ui.ctx().wants_keyboard_input() && ui.input(|i| i.modifiers.shift);
        if !self.is_generating
            && !self.content.is_empty()
            && (!self.is_user() || shift_held)
            && !self.is_error
        {
            ui.add_space(-12.0);
            ui.horizontal(|ui| {
                ui.add_space(message_offset);
                let copy = ui
                    .add(
                        egui::Button::new(if self.clicked_copy { "✔" } else { "🗐" })
                            .small()
                            .fill(egui::Color32::TRANSPARENT),
                    )
                    .on_hover_text(if self.clicked_copy {
                        "Copied!"
                    } else {
                        "Copy message"
                    });
                if copy.clicked() {
                    ui.ctx().copy_text(self.content.clone());
                    self.clicked_copy = true;
                }
                self.clicked_copy = self.clicked_copy && copy.hovered();

                let speak = ui
                    .add(
                        egui::Button::new(if self.is_speaking { "…" } else { "🔊" })
                            .small()
                            .fill(egui::Color32::TRANSPARENT),
                    )
                    .on_hover_text("Read the message out loud. Right click to repeat");

                if speak.clicked() {
                    if self.is_speaking {
                        self.is_speaking = false;
                        tts_control(tts, String::new(), false);
                    } else {
                        self.is_speaking = true;
                        tts_control(tts, self.content.clone(), true);
                    }
                } else if speak.secondary_clicked() {
                    self.is_speaking = true;
                    tts_control(tts, self.content.clone(), true);
                }
            });
            ui.add_space(8.0);
        }

        retry
    }
}

// <completion progress, final completion, error>
type CompletionFlower = CompactFlower<String, String, String>;
type CompletionFlowerHandle = CompactHandle<String, String, String>;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Chat {
    chatbox: String,
    #[serde(skip)]
    chatbox_height: f32,
    pub messages: Vec<Message>,
    pub context_messages: Vec<ChatMessage>,
    #[serde(skip)]
    flower: CompletionFlower,
    #[serde(skip)]
    retry_message_idx: Option<usize>,
    pub summary: String,
    #[serde(skip)]
    chatbox_highlighter: MemoizedEasymarkHighlighter,
    stop_generating: Arc<AtomicBool>,
    #[serde(skip)]
    virtual_list: VirtualList,
    pub model_picker: ModelPicker,
    pub images: Vec<PathBuf>,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            chatbox: String::new(),
            chatbox_height: 0.0,
            messages: Vec::new(),
            context_messages: Vec::new(),
            flower: CompletionFlower::new(1),
            retry_message_idx: None,
            summary: String::new(),
            chatbox_highlighter: MemoizedEasymarkHighlighter::default(),
            stop_generating: Arc::new(AtomicBool::new(false)),
            virtual_list: VirtualList::new(),
            model_picker: ModelPicker::default(),
            images: Vec::new(),
        }
    }
}

async fn request_completion(
    ollama: Ollama,
    messages: Vec<ChatMessage>,
    handle: &CompletionFlowerHandle,
    stop_generating: Arc<AtomicBool>,
    selected_model: String,
    options: GenerationOptions,
    template: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    log::info!(
        "requesting completion... (history length: {})",
        messages.len()
    );
    let mut request = ChatMessageRequest::new(selected_model, messages).options(options);
    if let Some(template) = template {
        request = request.template(template);
    }
    let mut stream: ChatMessageResponseStream = ollama.send_chat_messages_stream(request).await?;

    log::info!("reading response...");

    let mut response = String::new();
    let mut is_whitespace = true;

    while let Some(Ok(res)) = stream.next().await {
        if let Some(msg) = res.message {
            if is_whitespace && msg.content.trim().is_empty() {
                continue;
            }
            let content = if is_whitespace {
                msg.content.trim_start()
            } else {
                &msg.content
            };
            is_whitespace = false;

            // send message to gui thread
            handle.send(content.to_string());
            response += content;

            if stop_generating.load(Ordering::SeqCst) {
                log::info!("stopping generation");
                drop(stream);
                stop_generating.store(false, Ordering::SeqCst);
                break;
            }
        }
    }

    log::info!(
        "completion request complete, response length: {}",
        response.len()
    );
    handle.success(response.trim().to_string());
    Ok(())
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy, serde::Deserialize, serde::Serialize)]
pub enum ChatExportFormat {
    #[default]
    Plaintext,
    Json,
    Ron,
}

impl ToString for ChatExportFormat {
    fn to_string(&self) -> String {
        format!("{self:?}")
    }
}

impl ChatExportFormat {
    pub const ALL: [Self; 3] = [Self::Plaintext, Self::Json, Self::Ron];

    #[inline]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Plaintext => &["txt"],
            Self::Json => &["json"],
            Self::Ron => &["ron"],
        }
    }
}

pub async fn export_messages(
    messages: Vec<Message>,
    format: ChatExportFormat,
    task: impl std::future::Future<Output = Option<rfd::FileHandle>>,
) -> Result<egui_notify::Toast> {
    let Some(file) = task.await else {
        log::info!("export cancelled");
        return Ok(egui_notify::Toast::info("Export cancelled"));
    };
    log::info!(
        "exporting {} messages to {file:?} (format: {format:?})...",
        messages.len()
    );

    let f = std::fs::File::create(file.path())?;
    let mut f = std::io::BufWriter::new(f);

    match format {
        ChatExportFormat::Plaintext => {
            for msg in &messages {
                writeln!(
                    f,
                    "{} - {:?} ({}): {}",
                    msg.time.to_rfc3339(),
                    msg.role,
                    msg.model_name,
                    msg.content
                )?;
            }
        }
        ChatExportFormat::Json => {
            serde_json::to_writer_pretty(&mut f, &messages)?;
        }
        ChatExportFormat::Ron => {
            ron::ser::to_writer_pretty(&mut f, &messages, ron::ser::PrettyConfig::default())?;
        }
    }

    f.flush().context("failed to flush writer")?;

    log::info!("export complete");
    Ok(egui_notify::Toast::success(format!(
        "Exported {} messages to {}",
        messages.len(),
        file.file_name(),
    )))
}

fn make_summary(prompt: &str) -> String {
    const MAX_SUMMARY_LENGTH: usize = 24;
    let mut summary = String::with_capacity(MAX_SUMMARY_LENGTH);
    for (i, ch) in prompt.chars().enumerate() {
        if i >= MAX_SUMMARY_LENGTH {
            summary.push('…');
            break;
        }
        if ch == '\n' {
            break;
        }
        if i == 0 {
            summary += &ch.to_uppercase().to_string();
        } else {
            summary.push(ch);
        }
    }
    summary
}

#[derive(Debug, Clone, Copy)]
pub enum ChatAction {
    None,
    PickImages { id: usize },
}

fn show_images(ui: &mut egui::Ui, images: &[PathBuf]) {
    const MAX_IMAGE_HEIGHT: f32 = 128.0;
    for image_path in images {
        let path_string = image_path.display().to_string();
        ui.add(
            egui::Image::new(format!("file://{path_string}"))
                .max_height(MAX_IMAGE_HEIGHT)
                .fit_to_original_size(1.0),
        )
        .on_hover_text(path_string);
    }
}

impl Chat {
    #[inline]
    pub fn new(id: usize, model_picker: ModelPicker) -> Self {
        Self {
            flower: CompletionFlower::new(id),
            model_picker,
            ..Default::default()
        }
    }

    #[inline]
    pub fn id(&self) -> usize {
        self.flower.id()
    }

    fn send_message(&mut self, ollama: &Ollama) {
        // don't send empty messages
        if self.chatbox.is_empty() {
            return;
        }

        // remove old error messages
        self.messages.retain(|m| !m.is_error);

        let prompt = self.chatbox.trim_end().to_string();
        let model_name = self.model_picker.selected.name.clone();
        self.messages.push(Message::user(
            prompt.clone(),
            model_name.clone(),
            self.images.clone(),
        ));

        if self.summary.is_empty() {
            self.summary = make_summary(&prompt);
        }

        // clear chatbox
        self.chatbox.clear();

        // push prompt to ollama context messages
        let mut message = ChatMessage::user(prompt);
        if !self.images.is_empty() {
            message.images = Some(
                self.images
                    .iter()
                    .map(|i| crate::image::convert_image(i).unwrap())
                    .collect(),
            );
            self.images.clear();
        }
        self.context_messages.push(message);
        let context_messages = self.context_messages.clone();

        // get ready for assistant response
        self.messages
            .push(Message::assistant(String::new(), model_name.clone()));

        // spawn a new thread to generate the completion
        let handle = self.flower.handle(); // recv'd by gui thread
        let ollama = ollama.clone();
        let stop_generation = self.stop_generating.clone();
        let generation_options = self.model_picker.get_generation_options();
        let template = self.model_picker.template.clone();
        tokio::spawn(async move {
            handle.activate();
            let _ = request_completion(
                ollama,
                context_messages,
                &handle,
                stop_generation,
                model_name,
                generation_options,
                template,
            )
            .await
            .map_err(|e| {
                log::error!("failed to request completion: {e}");
                handle.error(e.to_string());
            });
        });
    }

    fn show_chatbox(
        &mut self,
        ui: &mut egui::Ui,
        is_max_height: bool,
        is_generating: bool,
        ollama: &Ollama,
    ) -> ChatAction {
        let mut action = ChatAction::None;
        if let Some(idx) = self.retry_message_idx.take() {
            self.chatbox = self.messages[idx].content.clone();
            self.messages.remove(idx + 1);
            self.messages.remove(idx);
            self.send_message(ollama);
        }

        if is_max_height {
            ui.add_space(8.0);
        }

        let images_height = if !self.images.is_empty() {
            ui.add_space(8.0);
            let height = egui::ScrollArea::horizontal()
                .show(ui, |ui| {
                    let height = ui
                        .horizontal(|ui| {
                            show_images(ui, &self.images);
                        })
                        .response
                        .rect
                        .height();
                    ui.add_space(8.0);
                    height
                })
                .inner;
            height + 16.0
        } else {
            0.0
        };

        ui.horizontal_centered(|ui| {
            // ui.add_enabled_ui(!is_generating, |ui| {
            //     if !is_max_height
            //         && ui
            //             .button("▶ Send")
            //             .on_disabled_hover_text("Please wait…")
            //             .clicked()
            //         && !is_generating
            //     {
            //         self.send_message(ollama);
            //     }
            // });
            if ui
                .add(
                    egui::Button::new("➕")
                        .min_size(vec2(32.0, 32.0))
                        .rounding(Rounding::same(f32::INFINITY)),
                )
                .on_hover_text_at_pointer("Pick Images")
                .clicked()
            {
                action = ChatAction::PickImages { id: self.id() };
            }
            ui.with_layout(
                Layout::left_to_right(Align::Center).with_main_justify(true),
                |ui| {
                    let Self {
                        chatbox_highlighter: highlighter,
                        ..
                    } = self;
                    let mut layouter = |ui: &egui::Ui, easymark: &str, wrap_width: f32| {
                        let mut layout_job = highlighter.highlight(ui.style(), easymark);
                        layout_job.wrap.max_width = wrap_width;
                        ui.fonts(|f| f.layout_job(layout_job))
                    };

                    self.chatbox_height = egui::TextEdit::multiline(&mut self.chatbox)
                        .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
                        .hint_text("Ask me anything…")
                        .layouter(&mut layouter)
                        .show(ui)
                        .response
                        .rect
                        .height()
                        + images_height;
                    if !is_generating
                        && ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.is_none())
                    {
                        self.send_message(ollama);
                    }
                },
            );
        });

        if is_max_height {
            ui.add_space(8.0);
        }

        action
    }

    #[inline]
    pub fn flower_active(&self) -> bool {
        self.flower.is_active()
    }

    pub fn poll_flower(&mut self, modal: &mut Modal) {
        self.flower
            .extract(|progress| {
                self.messages.last_mut().unwrap().content += progress.as_str();
            })
            .finalize(|result| {
                let message = self.messages.last_mut().unwrap();

                if let Ok(content) = result {
                    message.content = content;
                } else if let Err(e) = result {
                    let msg = match e {
                        Compact::Panicked(e) => format!("Tokio task panicked: {e}"),
                        Compact::Suppose(e) => e,
                    };
                    message.content = msg.clone();
                    message.is_error = true;
                    modal
                        .dialog()
                        .with_body(msg)
                        .with_title("Failed to generate completion!")
                        .with_icon(Icon::Error)
                        .open();
                }
                message.is_generating = false;
            });
    }

    pub fn last_message_contents(&self) -> Option<String> {
        for message in self.messages.iter().rev() {
            if message.content.is_empty() {
                continue;
            }
            return Some(if message.is_user() {
                format!("You: {}", message.content)
            } else {
                message.content.to_string()
            });
        }
        None
    }

    fn stop_generating_button(&self, ui: &mut egui::Ui, radius: f32, pos: Pos2) {
        let rect = Rect::from_min_max(pos + vec2(-radius, -radius), pos + vec2(radius, radius));
        let (hovered, primary_clicked) = ui.input(|i| {
            (
                i.pointer
                    .interact_pos()
                    .map(|p| rect.contains(p))
                    .unwrap_or(false),
                i.pointer.primary_clicked(),
            )
        });
        if hovered && primary_clicked {
            self.stop_generating.store(true, Ordering::SeqCst);
        } else {
            ui.painter().circle(
                pos,
                radius,
                if hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    if ui.style().visuals.dark_mode {
                        let c = ui.style().visuals.faint_bg_color;
                        Color32::from_rgb(c.r(), c.g(), c.b())
                    } else {
                        Color32::WHITE
                    }
                } else {
                    ui.style().visuals.window_fill
                },
                Stroke::new(2.0, ui.style().visuals.window_stroke.color),
            );
            ui.painter().rect_stroke(
                rect.shrink(radius / 2.0 + 1.2),
                2.0,
                Stroke::new(2.0, Color32::DARK_GRAY),
            );
        }
    }

    fn show_chat_scrollarea(
        &mut self,
        ui: &mut egui::Ui,
        commonmark_cache: &mut CommonMarkCache,
        tts: SharedTts,
    ) -> Option<usize> {
        let mut new_speaker: Option<usize> = None;
        egui::ScrollArea::both()
            .stick_to_bottom(true)
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.add_space(16.0);
                self.virtual_list
                    .ui_custom_layout(ui, self.messages.len(), |ui, index| {
                        let Some(message) = self.messages.get_mut(index) else {
                            return 0;
                        };
                        let prev_speaking = message.is_speaking;
                        if message.show(ui, commonmark_cache, tts.clone(), index) {
                            self.retry_message_idx = Some(index - 1);
                        }
                        if !prev_speaking && message.is_speaking {
                            new_speaker = Some(index);
                        }
                        1 // 1 rendered item per row
                    });
            });
        new_speaker
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        ollama: &Ollama,
        tts: SharedTts,
        stopped_speaking: bool,
        commonmark_cache: &mut CommonMarkCache,
    ) -> ChatAction {
        let avail = ctx.available_rect();
        let max_height = avail.height() * 0.4 + 24.0;
        let chatbox_panel_height = self.chatbox_height + 24.0;
        let actual_chatbox_panel_height = chatbox_panel_height.min(max_height);
        let is_generating = self.flower_active();
        let mut action = ChatAction::None;

        egui::TopBottomPanel::bottom("chatbox_panel")
            .exact_height(actual_chatbox_panel_height)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    action = self.show_chatbox(
                        ui,
                        chatbox_panel_height >= max_height,
                        is_generating,
                        ollama,
                    );
                });
            });

        let mut new_speaker: Option<usize> = None;
        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(Margin {
                left: 16.0,
                right: 16.0,
                top: 0.0,
                bottom: 3.0,
            }))
            .show(ctx, |ui| {
                if let Some(new) = self.show_chat_scrollarea(ui, commonmark_cache, tts) {
                    new_speaker = Some(new);
                }

                // stop generating button
                if is_generating {
                    self.stop_generating_button(
                        ui,
                        16.0,
                        pos2(
                            ui.cursor().max.x - 32.0,
                            avail.height() - 32.0 - actual_chatbox_panel_height,
                        ),
                    );
                }
            });

        if let Some(new_idx) = new_speaker {
            log::debug!("new speaker {new_idx} appeared, updating message icons");
            for (i, msg) in self.messages.iter_mut().enumerate() {
                if i == new_idx {
                    continue;
                }
                msg.is_speaking = false;
            }
        }
        if stopped_speaking {
            log::debug!("TTS stopped speaking, updating message icons");
            for msg in self.messages.iter_mut() {
                msg.is_speaking = false;
            }
        }

        action
    }
}

use eframe::egui::{self, Align, Frame, Key, KeyboardShortcut, Layout, Margin, Modifiers};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use flowync::{CompactFlower, CompactHandle};
use ollama_rs::{
    generation::chat::{request::ChatMessageRequest, ChatMessage, ChatMessageResponseStream},
    Ollama,
};
use std::{sync::Arc, time::Instant};
use tokio_stream::StreamExt;

struct Message {
    content: String,
    is_user: bool,
    is_generating: bool,
    requested_at: Instant,
    clicked_copy: bool,
}

impl Message {
    #[inline]
    fn user(content: String) -> Self {
        Self {
            content,
            is_user: true,
            is_generating: false,
            requested_at: Instant::now(),
            clicked_copy: false,
        }
    }

    #[inline]
    fn assistant(content: String) -> Self {
        Self {
            content,
            is_user: false,
            is_generating: true,
            requested_at: Instant::now(),
            clicked_copy: false,
        }
    }

    fn show(&mut self, ui: &mut egui::Ui, commonmark_cache: &mut CommonMarkCache, idx: usize) {
        // message role
        let message_offset = ui
            .horizontal(|ui| {
                if self.is_user {
                    let f = ui.label("👤").rect.left();
                    ui.label("You").rect.left() - f
                } else {
                    let f = ui.label("🐱").rect.left();
                    ui.label("Llama").rect.left() - f
                }
            })
            .inner;

        // for some reason commonmark creates empty space above it when created,
        // compensate for that
        if !self.content.is_empty() {
            ui.add_space(-24.0);
        }

        // message content / spinner
        ui.horizontal(|ui| {
            ui.add_space(message_offset);
            if self.content.is_empty() {
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
            } else {
                CommonMarkViewer::new(format!("message_{idx}_commonmark"))
                    .max_image_width(Some(512))
                    .show(ui, commonmark_cache, &self.content);
            }
        });

        // copy buttons and such
        if !self.is_generating && !self.content.is_empty() && !self.is_user {
            ui.add_space(-12.0);
            ui.horizontal(|ui| {
                ui.add_space(message_offset);
                let copy = ui
                    .add(
                        egui::Button::new(if self.clicked_copy { "✔" } else { "🗐" })
                            .small()
                            .fill(egui::Color32::TRANSPARENT),
                    )
                    .on_hover_text("Copy message");
                if copy.clicked() {
                    ui.ctx().copy_text(self.content.clone());
                    self.clicked_copy = true;
                }
                self.clicked_copy = self.clicked_copy && copy.hovered();
            });
            ui.add_space(8.0);
        }
    }
}

// <completion progress, final completion, error>
type CompletionFlower = CompactFlower<String, String, String>;
type CompletionFlowerHandle = CompactHandle<String, String, String>;

pub struct Chat {
    chatbox: String,
    chatbox_height: f32,
    messages: Vec<Message>,
    context_messages: Vec<ChatMessage>,
    flower: CompletionFlower,
    commonmark_cache: CommonMarkCache,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            chatbox: String::new(),
            chatbox_height: 0.0,
            messages: vec![],
            context_messages: vec![],
            flower: CompletionFlower::new(1),
            commonmark_cache: CommonMarkCache::default(),
        }
    }
}

async fn request_completion(
    ollama: Arc<Ollama>,
    messages: Vec<ChatMessage>,
    handle: &CompletionFlowerHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    log::info!(
        "requesting completion... (history length: {})",
        messages.len()
    );
    let mut stream: ChatMessageResponseStream = ollama
        .send_chat_messages_stream(ChatMessageRequest::new(
            "starling-lm:7b-alpha-q5_K_S".to_string(),
            messages,
        ))
        .await?;

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
            // log::debug!("{response}");
        }
    }

    log::info!(
        "completion request complete, response length: {}",
        response.len()
    );
    handle.success(response.trim().to_string());
    Ok(())
}

impl Chat {
    fn send_message(&mut self, ollama: Arc<Ollama>) {
        // don't send empty messages
        if self.chatbox.is_empty() {
            return;
        }

        let prompt = self.chatbox.trim_end().to_string();
        self.messages.push(Message::user(prompt.clone()));

        // clear chatbox
        self.chatbox.clear();

        // push prompt to ollama context messages
        self.context_messages.push(ChatMessage::user(prompt));
        let context_messages = self.context_messages.clone();

        // get ready for assistant response
        self.messages.push(Message::assistant(String::new()));

        // spawn a new thread to generate the completion
        let handle = self.flower.handle(); // recv'd by gui thread
        tokio::spawn(async move {
            handle.activate();
            let _ = request_completion(ollama, context_messages, &handle)
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
        ollama: Arc<Ollama>,
    ) {
        if is_max_height {
            ui.add_space(8.0);
        }
        ui.horizontal_centered(|ui| {
            ui.add_enabled_ui(!is_generating, |ui| {
                if !is_max_height
                    && ui
                        .button("Send")
                        .on_disabled_hover_text("Please wait…")
                        .clicked()
                    && !is_generating
                {
                    self.send_message(ollama.clone());
                }
            });
            ui.with_layout(
                Layout::left_to_right(Align::Center).with_main_justify(true),
                |ui| {
                    self.chatbox_height = egui::TextEdit::multiline(&mut self.chatbox)
                        .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
                        .hint_text("Ask me anything…")
                        .show(ui)
                        .response
                        .rect
                        .height();
                    if !is_generating
                        && ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.is_none())
                    {
                        self.send_message(ollama.clone());
                    }
                },
            );
        });
        if is_max_height {
            ui.add_space(8.0);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, ollama: Arc<Ollama>) {
        let avail = ctx.available_rect();
        let max_height = avail.height() * 0.4 + 24.0;
        let chatbox_panel_height = self.chatbox_height + 24.0;
        let is_generating = self.flower.is_active();

        egui::TopBottomPanel::bottom("chatbox_panel")
            .exact_height(chatbox_panel_height.min(max_height))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_chatbox(
                        ui,
                        chatbox_panel_height >= max_height,
                        is_generating,
                        ollama.clone(),
                    );
                });
            });

        if is_generating {
            ctx.request_repaint();
            self.flower
                .extract(|progress| {
                    self.messages.last_mut().unwrap().content += progress.as_str();
                })
                .finalize(|result| {
                    let message = self.messages.last_mut().unwrap();

                    // TODO: remove unwrap, open modal instead
                    message.content = result.unwrap();
                    message.is_generating = false;
                });
        }

        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(Margin {
                left: 16.0,
                right: 0.0,
                top: 0.0,
                bottom: 3.0,
            }))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.add_space(16.0); // instead of centralpanel margin
                        for (i, message) in self.messages.iter_mut().enumerate() {
                            message.show(ui, &mut self.commonmark_cache, i);
                        }
                    });
            });
    }
}

use eframe::egui::{self, Align, Frame, Key, KeyboardShortcut, Layout, Margin, Modifiers};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use flowync::{CompactFlower, CompactHandle};
use ollama_rs::{
    generation::chat::{request::ChatMessageRequest, ChatMessage, ChatMessageResponseStream},
    Ollama,
};
use std::sync::Arc;
use tokio_stream::StreamExt;

struct Message {
    content: String,
    is_user: bool,
}

impl Message {
    #[inline]
    const fn user(content: String) -> Self {
        Self {
            content,
            is_user: true,
        }
    }

    #[inline]
    const fn assistant(content: String) -> Self {
        Self {
            content,
            is_user: false,
        }
    }

    fn show(
        &self,
        ui: &mut egui::Ui,
        commonmark_cache: &mut CommonMarkCache,
        scroll_to: bool,
        idx: usize,
    ) {
        egui::Grid::new(format!("message_{idx}"))
            .min_col_width(0.0)
            .min_row_height(0.0)
            .num_columns(2)
            .show(ui, |ui| {
                if self.is_user {
                    ui.label("👤");
                    ui.label("You");
                } else {
                    ui.label("🤖");
                    ui.label("Llama");
                }
                ui.end_row();
                ui.add_sized([0.0, 0.0], egui::Label::new("")); // skip first column
                if self.content.is_empty() {
                    ui.add(egui::Spinner::new());
                } else {
                    let resp = CommonMarkViewer::new(format!("message_{idx}_commonmark"))
                        .max_image_width(Some(512))
                        .show(ui, commonmark_cache, self.content.trim())
                        .response;
                    if scroll_to {
                        resp.scroll_to_me(Some(Align::Max));
                    }
                }
            });
        ui.add_space(8.0);
    }
}

// <completion progress, final completion, error>
type CompletionFlower = CompactFlower<String, String, String>;
type CompletionFlowerHandle = CompactHandle<String, String, String>;

pub struct Chat {
    chatbox: String,
    chatbox_height: f32,
    is_at_bottom: bool,
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
            is_at_bottom: false,
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
    handle.success(response);
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

    fn show_chatbox(&mut self, ui: &mut egui::Ui, is_max_height: bool, ollama: Arc<Ollama>) {
        if is_max_height {
            ui.add_space(8.0);
        }
        ui.horizontal_centered(|ui| {
            if !is_max_height && ui.button("Send").clicked() {
                self.send_message(ollama.clone());
            }
            ui.with_layout(
                Layout::left_to_right(Align::Center).with_main_justify(true),
                |ui| {
                    self.chatbox_height = egui::TextEdit::multiline(&mut self.chatbox)
                        .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
                        .hint_text("Ask me anything...")
                        .show(ui)
                        .response
                        .rect
                        .height();
                    if ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.is_none()) {
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
                    ui.add_enabled_ui(!is_generating, |ui| {
                        self.show_chatbox(ui, chatbox_panel_height >= max_height, ollama.clone());
                    });
                });
            });

        if is_generating {
            ctx.request_repaint();
            self.flower
                .extract(|progress| {
                    self.messages.last_mut().unwrap().content += progress.as_str();
                })
                .finalize(|result| {
                    // TODO: remove unwrap, open modal instead
                    self.messages.last_mut().unwrap().content = result.unwrap();
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
                let scrollarea = egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .max_height(max_height)
                    .show(ui, |ui| {
                        ui.add_space(16.0); // instead of centralpanel margin
                        for (i, message) in self.messages.iter().enumerate() {
                            message.show(
                                ui,
                                &mut self.commonmark_cache,
                                self.is_at_bottom && i == self.messages.len() - 1,
                                i,
                            );
                        }
                    });
                log::info!(
                    "contentsize: {}, avail: {}",
                    scrollarea.state.offset
                );
                self.is_at_bottom =
                    scrollarea.content_size.y > avail.height() - chatbox_panel_height - 48.0;
            });
    }
}

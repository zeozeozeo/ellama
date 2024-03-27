use eframe::egui::{self, Align, Frame, Key, KeyboardShortcut, Layout, Margin, Modifiers};
use ollama_rs::{
    generation::chat::{
        request::ChatMessageRequest, ChatMessage, ChatMessageResponseStream, MessageRole,
    },
    Ollama,
};
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;

struct Message {
    content: String,
    is_user: bool,
}

impl Message {
    fn show(&self, ui: &mut egui::Ui, idx: usize) {
        egui::Grid::new(format!("message_{idx}"))
            .min_col_width(0.0)
            .num_columns(2)
            .show(ui, |ui| {
                if self.is_user {
                    ui.label("👤");
                    ui.label("You");
                } else {
                    ui.label("🦙");
                    ui.label("Llama");
                }
                ui.end_row();
                ui.add_sized([0.0, 0.0], egui::Label::new("")); // skip first column
                ui.label(&self.content);
            });
        ui.add_space(8.0);
    }
}

#[derive(Default)]
pub struct Chat {
    chatbox: String,
    chatbox_height: f32,
    messages: Vec<Message>,
    context_messages: Arc<RwLock<Vec<ChatMessage>>>,
    is_generating: Arc<AtomicBool>,
}

async fn request_completion(
    ollama: Arc<Ollama>,
    messages: Arc<RwLock<Vec<ChatMessage>>>,
    user_message: ChatMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    log::info!("requesting completion...");

    {
        messages.write().await.push(user_message);
    }

    let mut stream: ChatMessageResponseStream = ollama
        .send_chat_messages_stream(ChatMessageRequest::new(
            "llama2-uncensored:7b-chat".to_string(),
            messages.read().await.to_vec(),
        ))
        .await?;

    log::info!("reading response...");

    messages
        .write()
        .await
        .push(ChatMessage::assistant(String::new()));

    let mut response = String::new();
    while let Some(Ok(res)) = stream.next().await {
        if let Some(assistant_message) = res.message {
            response += assistant_message.content.as_str();
            messages.write().await.last_mut().unwrap().content = response.clone();
            log::info!("{response}");
        }
    }

    Ok(())
}

impl Chat {
    fn send_message(&mut self, ollama: Arc<Ollama>) {
        if self.chatbox.is_empty() {
            return;
        }

        let prompt = self.chatbox.trim_end().to_string();
        self.messages.push(Message {
            content: prompt.clone(),
            is_user: true,
        });
        self.chatbox.clear();

        let context_messages = self.context_messages.clone();
        let is_generating = self.is_generating.clone();
        let user_message = ChatMessage {
            role: MessageRole::User,
            content: prompt,
            images: None,
        };
        tokio::spawn(async move {
            is_generating.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = request_completion(ollama, context_messages.clone(), user_message)
                .await
                .map_err(|e| log::error!("failed to request completion: {e}"));
            is_generating.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    #[inline]
    fn is_generating(&self) -> bool {
        self.is_generating.load(std::sync::atomic::Ordering::SeqCst)
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
        let is_generating = self.is_generating();

        egui::TopBottomPanel::bottom("chatbox_panel")
            .exact_height(chatbox_panel_height.min(max_height))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(!is_generating, |ui| {
                        self.show_chatbox(ui, chatbox_panel_height >= max_height, ollama.clone());
                    });
                });
            });

        if is_generating {}

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
                    .show(ui, |ui| {
                        ui.add_space(16.0); // instead of centralpanel margin
                        for (i, message) in self.messages.iter().enumerate() {
                            message.show(ui, i);
                        }
                    });
            });
    }
}

use std::{sync::Arc, time::Duration};

use chat::Chat;
use eframe::egui;
use ollama_rs::Ollama;
use tts::Tts;

mod chat;

#[tokio::main]
async fn main() {
    env_logger::init();
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Ellama",
        native_options,
        Box::new(|cc| Box::new(Ellama::new(cc))),
    )
    .expect("failed to run app");
}

struct Ellama {
    chat: Chat,
    ollama: Arc<Ollama>,
    tts: Option<Tts>,
    is_speaking: bool,
}

impl Default for Ellama {
    fn default() -> Self {
        Self {
            chat: Chat::default(),
            ollama: Arc::new(Ollama::default()),
            tts: Tts::default()
                .map_err(|e| log::error!("failed to initialize TTS: {e}"))
                .ok(),
            is_speaking: false,
        }
    }
}

impl Ellama {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MACCHIATO);
        //cc.egui_ctx.style_mut(|s| s.wrap = Some(true));
        Self::default()
    }
}

impl eframe::App for Ellama {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let prev_is_speaking = self.is_speaking;
        self.is_speaking = if let Some(tts) = &self.tts {
            tts.is_speaking().unwrap_or(false)
        } else {
            false
        };
        if self.is_speaking {
            ctx.request_repaint();
        }
        self.chat.show(
            ctx,
            self.ollama.clone(),
            &mut self.tts,
            !self.is_speaking && prev_is_speaking,
        );
    }
}

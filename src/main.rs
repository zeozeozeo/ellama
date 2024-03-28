use std::sync::Arc;

use chat::Chat;
use eframe::egui;
use ollama_rs::Ollama;

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

#[derive(Default)]
struct Ellama {
    chat: Chat,
    ollama: Arc<Ollama>,
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
        self.chat.show(ctx, self.ollama.clone());
    }
}

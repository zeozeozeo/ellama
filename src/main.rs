use eframe::egui;
use ollama_rs::Ollama;
use sessions::Sessions;

mod chat;
mod easymark;
mod sessions;
mod widgets;

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
    sessions: Sessions,
    ollama: Ollama,
}

impl Default for Ellama {
    fn default() -> Self {
        let ollama = Ollama::default();
        Self {
            sessions: Sessions::new(ollama.clone()),
            ollama,
        }
    }
}

impl Ellama {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        //catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MACCHIATO);
        //cc.egui_ctx.style_mut(|s| s.wrap = Some(true));
        cc.egui_ctx
            .style_mut(|s| s.visuals = egui::Visuals::light());
        cc.egui_ctx
            .style_mut(|s| s.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand));
        Self::default()
    }
}

impl eframe::App for Ellama {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sessions.show(ctx, &self.ollama);
    }
}

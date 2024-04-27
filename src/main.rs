use eframe::egui;
use ollama_rs::Ollama;
use sessions::Sessions;

mod chat;
mod easymark;
mod image;
mod sessions;
mod style;
mod widgets;

const TITLE: &str = "Ellama";

#[tokio::main]
async fn main() {
    env_logger::init();
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        TITLE,
        native_options,
        Box::new(|cc| Box::new(Ellama::new(cc))),
    )
    .expect("failed to run app");
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Ellama {
    sessions: Sessions,
    #[serde(skip)]
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
        // change visuals
        style::set_style(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // try to restore app
        log::debug!(
            "trying to restore app state from storage: {:?}",
            eframe::storage_dir(TITLE)
        );

        if let Some(storage) = cc.storage {
            if let Some(mut app_state) = eframe::get_value::<Self>(storage, eframe::APP_KEY) {
                log::debug!("app state successfully restored from storage");
                app_state.sessions.list_models(app_state.ollama.clone());
                app_state.ollama = app_state.sessions.settings.make_ollama();
                return app_state;
            }
        }

        log::debug!("app state is not saved in storage, using default app state");

        // default app
        Self::default()
    }
}

impl eframe::App for Ellama {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sessions.show(ctx, &self.ollama);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        log::debug!("saving app state");
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

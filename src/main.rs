#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

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
const IMAGE_FORMATS: &[&str] = &[
    "bmp", "dds", "ff", "gif", "hdr", "ico", "jpeg", "jpg", "exr", "png", "pnm", "qoi", "tga",
    "tiff", "webp",
];

fn load_icon() -> egui::IconData {
    let (icon_rgba, icon_width, icon_height) = {
        let icon = include_bytes!("../assets/icon.png");
        let image = ::image::load_from_memory(icon)
            .expect("failed to load icon")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };

    egui::IconData {
        rgba: icon_rgba,
        width: icon_width,
        height: icon_height,
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_icon(load_icon()),
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        native_options,
        Box::new(|cc| Ok(Box::new(Ellama::new(cc)))),
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

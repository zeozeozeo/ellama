use crate::chat::Chat;
use eframe::egui;
use ollama_rs::Ollama;
use std::sync::Arc;
use tts::Tts;

#[derive(Default, PartialEq)]
enum SessionTab {
    #[default]
    Chats,
    Model,
}

#[derive(Default)]
pub struct Sessions {
    tab: SessionTab,
    chats: Vec<Chat>,
    selected_chat: Option<usize>,
}

impl Sessions {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        ollama: Arc<Ollama>,
        tts: &mut Option<Tts>,
        stopped_talking: bool,
    ) {
        let avail_width = ctx.available_rect().width();
        egui::SidePanel::left("sessions_panel")
            .resizable(true)
            .max_width(avail_width * 0.5)
            .show(ctx, |ui| {
                self.show_left_panel(ui);
                ui.allocate_space(ui.available_size());
            });

        if let Some(chat) = self.get_selected_chat() {
            chat.show(ctx, ollama.clone(), tts, stopped_talking);
        }
    }

    fn show_left_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(ui.style().spacing.window_margin.top);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, SessionTab::Chats, "Chats");
            ui.selectable_value(&mut self.tab, SessionTab::Model, "Model");
        });

        ui.add_space(8.0);

        match self.tab {
            SessionTab::Chats => {
                self.show_chats(ui);
            }
            SessionTab::Model => {
                ui.label("Model");
            }
        }
    }

    #[inline]
    fn get_selected_chat(&mut self) -> Option<&mut Chat> {
        self.chats.get_mut(self.selected_chat?)
    }

    fn show_chats(&mut self, ui: &mut egui::Ui) {
        if ui.button("➕ New Chat").clicked() {
            self.chats.push(Chat::default());
        }
        for (i, chat) in self.chats.iter().enumerate() {
            if ui
                .button(if chat.summary.is_empty() {
                    "Empty chat"
                } else {
                    &chat.summary
                })
                .clicked()
            {
                self.selected_chat = Some(i);
            }
        }
    }
}

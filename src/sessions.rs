use crate::chat::Chat;
use eframe::egui::{self, Color32, Frame, Layout, Rounding, Stroke};
use egui_commonmark::CommonMarkCache;
use egui_modal::{Icon, Modal};
use ollama_rs::Ollama;
use parking_lot::RwLock;
use std::sync::Arc;
use tts::Tts;

#[derive(Default, PartialEq)]
enum SessionTab {
    #[default]
    Chats,
    Model,
}

pub type SharedTts = Option<Arc<RwLock<Tts>>>;

pub struct Sessions {
    tab: SessionTab,
    chats: Vec<Chat>,
    selected_chat: usize,
    chat_marked_for_deletion: usize,
    is_speaking: bool,
    tts: SharedTts,
    commonmark_cache: CommonMarkCache,
}

impl Default for Sessions {
    fn default() -> Self {
        Self {
            tab: SessionTab::Chats,
            chats: vec![Chat::default()],
            selected_chat: 0,
            chat_marked_for_deletion: 0,
            is_speaking: false,
            tts: Tts::default()
                .map_err(|e| log::error!("failed to initialize TTS: {e}"))
                .map(|tts| Arc::new(RwLock::new(tts)))
                .ok(),
            commonmark_cache: CommonMarkCache::default(),
        }
    }
}

impl Sessions {
    pub fn show(&mut self, ctx: &egui::Context, ollama: &Ollama) {
        // check if tts stopped speaking
        let prev_is_speaking = self.is_speaking;
        self.is_speaking = if let Some(tts) = &self.tts {
            tts.read().is_speaking().unwrap_or(false)
        } else {
            false
        };

        // if speaking, continuously check if stopped
        if self.is_speaking {
            ctx.request_repaint();
        }

        let mut modal = Modal::new(ctx, "sessions_main_modal");
        let mut chat_modal = Modal::new(ctx, "chat_main_modal").with_close_on_outside_click(true);

        // show dialogs created on the previous frame, if we move this into the end of the function
        // it won't be located in the center of the window but in the center of the centralpanel instead
        modal.show_dialog();
        chat_modal.show_dialog();

        let avail_width = ctx.available_rect().width();
        egui::SidePanel::left("sessions_panel")
            .resizable(true)
            .max_width(avail_width * 0.5)
            .show(ctx, |ui| {
                self.show_left_panel(ui);
                ui.allocate_space(ui.available_size());
            });

        // poll all flowers
        let mut requested_repaint = false;
        for chat in self.chats.iter_mut() {
            if chat.flower_active() {
                if !requested_repaint {
                    ctx.request_repaint();
                    requested_repaint = true;
                }
                chat.poll_flower(&mut chat_modal);
            }
        }

        self.chats[self.selected_chat].show(
            ctx,
            ollama,
            self.tts.clone(),
            prev_is_speaking && !self.is_speaking, // stopped_talking
            &mut self.commonmark_cache,
        );
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
                let modal = Modal::new(ui.ctx(), "left_panel_chats_modal");
                self.show_chats(ui, &modal);
                modal.show(|ui| {
                    modal.title(ui, "Remove Chat");
                    modal.frame(ui, |ui| {
                        modal.body_and_icon(
                            ui,
                            "Do you really want to remove this chat? \
                            You cannot undo this action later.\n\
                            Hold Shift to surpass this warning.",
                            Icon::Warning,
                        );
                        modal.buttons(ui, |ui| {
                            if ui.button("No").clicked() {
                                modal.close();
                            }
                            if ui.button("Yes").clicked() {
                                modal.close();
                                self.remove_chat(self.chat_marked_for_deletion);
                            }
                        });
                    });
                });
            }
            SessionTab::Model => {
                ui.label("Model");
            }
        }
    }

    #[inline]
    fn add_default_chat(&mut self) {
        self.chats.push(Chat::default());
    }

    fn remove_chat(&mut self, idx: usize) {
        self.chats.remove(idx);
        if self.chats.is_empty() {
            self.add_default_chat();
            self.selected_chat = 0;
        } else if self.selected_chat >= self.chats.len() {
            self.selected_chat = self.chats.len() - 1;
        }
    }

    /// Returns whether any chat was removed
    fn show_chat_frame(&mut self, ui: &mut egui::Ui, idx: usize, modal: &Modal) -> bool {
        let Some(chat) = &self.chats.get(idx) else {
            return false;
        };
        let mut chat_removed = false;

        let last_message = chat
            .last_message_contents()
            .unwrap_or_else(|| "No recent messages".to_string());

        ui.horizontal(|ui| {
            ui.add(egui::Label::new("Chat").selectable(false));
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new("❌")
                            .small()
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                    )
                    .on_hover_text("Remove chat")
                    .clicked()
                {
                    if ui.input(|i| i.modifiers.shift) {
                        self.remove_chat(idx);
                    } else {
                        self.chat_marked_for_deletion = idx;
                        modal.open();
                    }
                    chat_removed = true;
                }
            });
        });

        ui.add_enabled(
            false,
            egui::Label::new(last_message)
                .selectable(false)
                .truncate(true),
        );
        chat_removed
    }

    /// Returns whether the chat should be selected as the current one
    fn show_sidepanel_chat(&mut self, ui: &mut egui::Ui, idx: usize, modal: &Modal) -> bool {
        let mut chat_removed = false;
        let resp = Frame::group(ui.style())
            .rounding(Rounding::same(6.0))
            .stroke(Stroke::new(2.0, ui.style().visuals.window_stroke.color))
            .fill(if self.selected_chat == idx {
                ui.style().visuals.faint_bg_color
            } else {
                ui.style().visuals.window_fill
            })
            .show(ui, |ui| {
                chat_removed = self.show_chat_frame(ui, idx, modal);
            })
            .response;

        // very hacky way to determine if the group has been clicked, for some reason
        // egui doens't register clicked() events on it
        let (primary_clicked, hovered) = ui.input(|i| {
            (
                i.pointer.primary_clicked(),
                i.pointer
                    .interact_pos()
                    .map(|p| resp.rect.contains(p))
                    .unwrap_or(false),
            )
        });

        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        !chat_removed && primary_clicked && hovered
    }

    fn show_chats(&mut self, ui: &mut egui::Ui, modal: &Modal) {
        // TODO: use show_rows() instead of show()
        egui::ScrollArea::vertical().show(ui, |ui| {
            if ui.button("➕ New Chat").clicked() {
                self.add_default_chat();
                self.selected_chat = self.chats.len() - 1;
            }
            ui.separator();
            for i in 0..self.chats.len() {
                if self.show_sidepanel_chat(ui, i, modal) {
                    self.selected_chat = i;
                }
                ui.add_space(2.0);
            }
        });
    }
}

use eframe::egui::{self, FontTweak};

pub fn set_style(ctx: &egui::Context) {
    ctx.style_mut(|s| {
        s.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    });

    let mut fonts = egui::FontDefinitions::empty();

    // install custom fonts
    log::info!("installing custom fonts");
    fonts.font_data.insert(
        "Inter-Regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/Inter-Regular.ttf")),
    );
    fonts.font_data.insert(
        "JetBrainsMono-Regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/JetBrainsMono-Regular.ttf")),
    );
    fonts.font_data.insert(
        "NotoEmoji-Regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/NotoEmoji-Regular.ttf")).tweak(
            FontTweak {
                scale: 0.81, // make it smaller
                ..Default::default()
            },
        ),
    );
    fonts.font_data.insert(
        "emoji-icon-font".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/emoji-icon-font.ttf")).tweak(
            FontTweak {
                scale: 0.88, // make it smaller

                // probably not correct, but this does make texts look better
                y_offset_factor: 0.11, // move glyphs down to better align with common fonts
                baseline_offset_factor: -0.11, // ...now the entire row is a bit down so shift it back
                ..Default::default()
            },
        ),
    );

    fonts.families.insert(
        egui::FontFamily::Proportional,
        vec![
            "Inter-Regular".to_owned(),
            "NotoEmoji-Regular".to_owned(),
            "emoji-icon-font".to_owned(),
        ],
    );
    fonts.families.insert(
        egui::FontFamily::Monospace,
        vec![
            "JetBrainsMono-Regular".to_owned(),
            "NotoEmoji-Regular".to_owned(),
            "emoji-icon-font".to_owned(),
        ],
    );

    ctx.set_zoom_factor(1.15);
    ctx.set_fonts(fonts);
}

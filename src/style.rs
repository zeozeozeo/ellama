use eframe::egui;

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
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter-Regular".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono-Regular".to_owned());

    ctx.set_zoom_factor(1.08);
    ctx.set_fonts(fonts);
}

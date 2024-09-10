use anyhow::Result;
use base64_stream::ToBase64Reader;
use eframe::egui::{self, vec2, Color32, Rect, RichText, Stroke};
use image::ImageFormat;
use ollama_rs::generation::images::Image;
use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::{Path, PathBuf},
};

pub fn convert_image(path: &Path) -> Result<Image> {
    let f = BufReader::new(File::open(path)?);

    // ollama only supports png and jpeg, we have to convert to png
    // whenever needed
    let format = ImageFormat::from_path(path)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        log::debug!("got {format:?} image, converting to png");
        let img = image::load(f, format)?;
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
        let mut reader = ToBase64Reader::new(buf.as_slice());
        let mut base64 = String::new();
        reader.read_to_string(&mut base64)?;
        log::debug!("converted to {} bytes of base64", base64.len());
        return Ok(Image::from_base64(&base64));
    }

    // otherwise, ollama can handle it
    let mut reader = ToBase64Reader::new(f);
    let mut base64 = String::new();
    reader.read_to_string(&mut base64)?;
    log::debug!("read image to {} bytes of base64", base64.len());
    Ok(Image::from_base64(&base64))
}

pub fn show_images(ui: &mut egui::Ui, images: &mut Vec<PathBuf>, mutate: bool) {
    const MAX_IMAGE_HEIGHT: f32 = 128.0;
    let pointer_pos = ui.input(|i| i.pointer.interact_pos());
    let mut showing_x = false;

    images.retain_mut(|image_path| {
        let path_string = image_path.display().to_string();
        let resp = ui
            .group(|ui| {
                ui.vertical(|ui| {
                    ui.add(
                        egui::Image::new(format!("file://{path_string}"))
                            .max_height(MAX_IMAGE_HEIGHT)
                            .fit_to_original_size(1.0),
                    )
                    .on_hover_text(path_string);

                    let file_name = image_path.file_name().unwrap_or_default().to_string_lossy();
                    ui.add(egui::Label::new(RichText::new(file_name).small()).truncate());
                });
            })
            .response;

        if !mutate || showing_x {
            return true;
        }

        if let Some(pos) = pointer_pos {
            if resp.rect.expand(8.0).contains(pos) {
                showing_x = true;

                // render an ❌ in a red circle
                let top = resp.rect.right_top();
                let x_rect = Rect::from_center_size(top, vec2(16.0, 16.0));
                let contains_pointer = x_rect.contains(pos);

                ui.painter()
                    .circle_filled(top, 10.0, ui.visuals().window_fill);
                ui.painter().circle_filled(
                    top,
                    8.0,
                    if contains_pointer {
                        ui.visuals().gray_out(ui.visuals().error_fg_color)
                    } else {
                        ui.visuals().error_fg_color
                    },
                );
                ui.painter().line_segment(
                    [top - vec2(3.0, 3.0), top + vec2(3.0, 3.0)],
                    Stroke::new(2.0, Color32::WHITE),
                );
                ui.painter().line_segment(
                    [top - vec2(3.0, -3.0), top + vec2(3.0, -3.0)],
                    Stroke::new(2.0, Color32::WHITE),
                );

                if contains_pointer && ui.input(|i| i.pointer.primary_clicked()) {
                    return false;
                }
            }
        }

        true
    });
}

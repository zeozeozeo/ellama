use anyhow::Result;
use base64_stream::ToBase64Reader;
use image::ImageFormat;
use ollama_rs::generation::images::Image;
use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
};

pub fn convert_image(path: &Path) -> Result<Image> {
    let f = BufReader::new(File::open(path)?);

    // ollama only supports png and jpeg, we have to convert to png
    // whenever needed
    let format = ImageFormat::from_path(path)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        let img = image::load(f, format)?;
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
        let mut reader = ToBase64Reader::new(buf.as_slice());
        let mut base64 = String::new();
        reader.read_to_string(&mut base64)?;
        return Ok(Image::from_base64(&base64));
    }

    // otherwise, ollama can handle it
    let mut reader = ToBase64Reader::new(f);
    let mut base64 = String::new();
    reader.read_to_string(&mut base64)?;
    Ok(Image::from_base64(&base64))
}

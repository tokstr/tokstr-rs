use anyhow::Result;
use std::fs::File;
use std::io::BufWriter;
use std::net::TcpListener;
use std::path::Path;
use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, ExtendedColorType};
use log::error;

pub(crate) fn write_image_to_jpeg(
    image_data: &[u8],
    output_path: &Path,
) -> Result<()> {
    // Decode the `image_data` into a DynamicImage:
    let dynamic_image = image::load_from_memory(image_data)?;

    // Convert DynamicImage to RGB8:
    let rgb_image = dynamic_image.to_rgb8();

    // Create output file/buffer:
    let file = File::create(output_path)?;
    let buf_writer = BufWriter::new(file);

    // Initialize JPEG encoder with quality = 80:
    let mut encoder = JpegEncoder::new_with_quality(buf_writer, 80);

    // Encode the RGB image into the file:
    encoder.encode(
        &rgb_image,
        rgb_image.width(),
        rgb_image.height(),
        ExtendedColorType::from(ColorType::Rgb8),
    )?;

    Ok(())
}


pub fn find_available_port() -> Result<TcpListener> {
    if let Ok(listener) = TcpListener::bind("0.0.0.0:0") {
        return Ok(listener);
    }
    if let Ok(listener) = TcpListener::bind("127.0.0.1:0") {
        return Ok(listener);
    }

    for port in 8000..9000 {
        let addr = format!("127.0.0.1:{}", port);
        error!("Trying port {}", port);
        if let Ok(listener) = TcpListener::bind(&addr) {
            return Ok(listener);
        }
    }
    Err(anyhow::anyhow!("No available ports found"))
}
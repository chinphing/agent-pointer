use anyhow::Result;
use base64::Engine as _;
use image::ImageEncoder;
use qrcode::QrCode;

pub fn qrcode_png_base64(data: &str) -> Result<String> {
    let code = QrCode::new(data.as_bytes())?;
    let image = code.render::<image::Luma<u8>>().build();
    let mut bytes: Vec<u8> = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder.write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::L8,
    )?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

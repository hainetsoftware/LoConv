//! Still-image backend implementation using the Rust `image` crate.
//!
//! Provides bounded PNG, JPEG, and WebP inspection and conversion with explicit
//! decoder limits. Animated and oversized images are rejected safely.

use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use image::{
    codecs::jpeg::JpegEncoder, ExtendedColorType, ImageFormat, ImageReader, Limits, Rgb, RgbImage,
};

use crate::errors::{self, EngineResult};

/// Maximum allowed still-image resolution in pixels (100 megapixels).
pub const MAX_IMAGE_MEGAPIXELS: u64 = 100_000_000;

/// Maximum allocation ceiling for image decoding (512 MiB).
pub const MAX_DECODED_ALLOCATION_BYTES: u64 = 512 * 1024 * 1024;

/// Default JPEG quality preset for Sprint 1.
pub const JPEG_QUALITY_PRESET: u8 = 85;

/// Whether a bounded image codec backend is configured and available.
pub const fn available() -> bool {
    true
}

/// Supported still-image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl SupportedImageFormat {
    pub fn container_name(&self) -> &'static str {
        match self {
            SupportedImageFormat::Png => "png",
            SupportedImageFormat::Jpeg => "jpeg",
            SupportedImageFormat::Webp => "webp",
        }
    }

    pub fn codec_name(&self) -> &'static str {
        match self {
            SupportedImageFormat::Png => "png",
            SupportedImageFormat::Jpeg => "jpeg",
            SupportedImageFormat::Webp => "webp",
        }
    }

    pub fn to_image_format(&self) -> ImageFormat {
        match self {
            SupportedImageFormat::Png => ImageFormat::Png,
            SupportedImageFormat::Jpeg => ImageFormat::Jpeg,
            SupportedImageFormat::Webp => ImageFormat::WebP,
        }
    }
}

/// Detect image format by inspecting header bytes rather than trusting file extensions.
pub fn detect_format_from_bytes(bytes: &[u8]) -> EngineResult<SupportedImageFormat> {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Png) => Ok(SupportedImageFormat::Png),
        Ok(ImageFormat::Jpeg) => Ok(SupportedImageFormat::Jpeg),
        Ok(ImageFormat::WebP) => Ok(SupportedImageFormat::Webp),
        _ => Err(errors::unsupported_input()),
    }
}

/// Check if a PNG stream contains an Animation Control (`acTL`) chunk (APNG).
pub fn is_animated_png<R: Read + Seek>(reader: &mut R) -> bool {
    let mut sig = [0u8; 8];
    if reader.read_exact(&mut sig).is_err() || sig != [137, 80, 78, 71, 13, 10, 26, 10] {
        return false;
    }

    let mut chunk_header = [0u8; 8];
    while reader.read_exact(&mut chunk_header).is_ok() {
        let length = u32::from_be_bytes([
            chunk_header[0],
            chunk_header[1],
            chunk_header[2],
            chunk_header[3],
        ]) as i64;
        let chunk_type = &chunk_header[4..8];

        if chunk_type == b"acTL" {
            return true;
        }
        // acTL must appear before the first IDAT chunk in valid APNG
        if chunk_type == b"IDAT" || chunk_type == b"IEND" {
            return false;
        }

        // Skip chunk data + 4 bytes CRC
        if reader.seek(SeekFrom::Current(length + 4)).is_err() {
            return false;
        }
    }
    false
}

/// Check if a WebP stream contains animation (`VP8X` animation flag or `ANIM` chunk).
pub fn is_animated_webp<R: Read + Seek>(reader: &mut R) -> bool {
    let mut riff_header = [0u8; 12];
    if reader.read_exact(&mut riff_header).is_err() {
        return false;
    }
    if &riff_header[0..4] != b"RIFF" || &riff_header[8..12] != b"WEBP" {
        return false;
    }

    let mut chunk_header = [0u8; 8];
    while reader.read_exact(&mut chunk_header).is_ok() {
        let fourcc = &chunk_header[0..4];
        let size = u32::from_le_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]) as i64;

        if fourcc == b"ANIM" {
            return true;
        }

        if fourcc == b"VP8X" {
            let mut flags = [0u8; 4];
            if reader.read_exact(&mut flags).is_ok() {
                // Bit 1 is the Animation flag
                if (flags[0] & 0x02) != 0 {
                    return true;
                }
                // Skip remainder of VP8X chunk data (size - 4) plus padding if odd
                let pad = if size % 2 != 0 { 1 } else { 0 };
                let rem = (size - 4) + pad;
                if rem > 0 && reader.seek(SeekFrom::Current(rem)).is_err() {
                    return false;
                }
                continue;
            } else {
                return false;
            }
        }

        let pad = if size % 2 != 0 { 1 } else { 0 };
        if reader.seek(SeekFrom::Current(size + pad)).is_err() {
            return false;
        }
    }
    false
}

/// Facts discovered during image inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageDetails {
    pub format: SupportedImageFormat,
    pub width: u32,
    pub height: u32,
}

/// Inspect a local image file safely without decoding pixel buffers.
pub fn inspect_image_file(path: &Path) -> EngineResult<ImageDetails> {
    let mut file = File::open(path).map_err(|_| errors::inspection_failed())?;
    let mut header = [0u8; 64];
    let bytes_read = file
        .read(&mut header)
        .map_err(|_| errors::inspection_failed())?;
    if bytes_read < 12 {
        return Err(errors::unsupported_input());
    }

    let format = detect_format_from_bytes(&header[..bytes_read])?;

    // Check for animated images and reject safely
    file.seek(SeekFrom::Start(0))
        .map_err(|_| errors::inspection_failed())?;
    match format {
        SupportedImageFormat::Png => {
            if is_animated_png(&mut file) {
                return Err(errors::unsupported_input());
            }
        }
        SupportedImageFormat::Webp => {
            if is_animated_webp(&mut file) {
                return Err(errors::unsupported_input());
            }
        }
        SupportedImageFormat::Jpeg => {}
    }

    // Inspect dimensions with strict limits without decoding full raster
    file.seek(SeekFrom::Start(0))
        .map_err(|_| errors::inspection_failed())?;
    let mut reader = ImageReader::new(BufReader::new(file));
    reader.set_format(format.to_image_format());
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODED_ALLOCATION_BYTES);
    limits.max_image_width = Some(MAX_IMAGE_MEGAPIXELS as u32);
    limits.max_image_height = Some(MAX_IMAGE_MEGAPIXELS as u32);
    reader.limits(limits);

    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| errors::inspection_failed())?;

    if width == 0 || height == 0 {
        return Err(errors::inspection_failed());
    }

    let total_pixels = (width as u64)
        .checked_mul(height as u64)
        .ok_or_else(errors::resource_limit)?;
    if total_pixels > MAX_IMAGE_MEGAPIXELS {
        return Err(errors::resource_limit());
    }

    Ok(ImageDetails {
        format,
        width,
        height,
    })
}

/// Composite RGBA pixels onto a white background (for JPEG encoding without alpha channel).
pub fn composite_alpha_onto_white(rgba: &image::RgbaImage) -> RgbImage {
    let (width, height) = rgba.dimensions();
    let mut rgb = RgbImage::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let out_r = if a == 255 {
            r
        } else if a == 0 {
            255
        } else {
            let alpha = a as u32;
            let inv_alpha = 255 - alpha;
            ((r as u32 * alpha + 255 * inv_alpha + 127) / 255) as u8
        };
        let out_g = if a == 255 {
            g
        } else if a == 0 {
            255
        } else {
            let alpha = a as u32;
            let inv_alpha = 255 - alpha;
            ((g as u32 * alpha + 255 * inv_alpha + 127) / 255) as u8
        };
        let out_b = if a == 255 {
            b
        } else if a == 0 {
            255
        } else {
            let alpha = a as u32;
            let inv_alpha = 255 - alpha;
            ((b as u32 * alpha + 255 * inv_alpha + 127) / 255) as u8
        };
        rgb.put_pixel(x, y, Rgb([out_r, out_g, out_b]));
    }

    rgb
}

/// Execute real image conversion to JPEG with fixed quality preset (85) and composite alpha onto white.
pub fn convert_image_to_jpeg(
    input_path: &Path,
    temp_output_path: &Path,
    cancel_flag: &AtomicBool,
) -> EngineResult<()> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err(errors::internal());
    }

    // 1. Inspect first to verify safe boundaries and supported format
    let details = inspect_image_file(input_path)?;

    if cancel_flag.load(Ordering::SeqCst) {
        return Err(errors::internal());
    }

    // 2. Decode with strict limits
    let file = File::open(input_path).map_err(|_| errors::input_unavailable())?;
    let mut reader = ImageReader::new(BufReader::new(file));
    reader.set_format(details.format.to_image_format());
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODED_ALLOCATION_BYTES);
    limits.max_image_width = Some(details.width);
    limits.max_image_height = Some(details.height);
    reader.limits(limits);

    let dynamic_img = reader.decode().map_err(|_| errors::conversion_failed())?;

    if cancel_flag.load(Ordering::SeqCst) {
        return Err(errors::internal());
    }

    // 3. Composite alpha onto white to produce clean RGB8
    let rgba = dynamic_img.to_rgba8();
    let rgb = composite_alpha_onto_white(&rgba);
    let (width, height) = rgb.dimensions();

    if cancel_flag.load(Ordering::SeqCst) {
        return Err(errors::internal());
    }

    // 4. Encode to the safe temporary file
    let temp_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_output_path)
        .map_err(|_| errors::internal())?;

    let mut writer = BufWriter::new(temp_file);
    let mut encoder = JpegEncoder::new_with_quality(&mut writer, JPEG_QUALITY_PRESET);
    encoder
        .encode(rgb.as_raw(), width, height, ExtendedColorType::Rgb8)
        .map_err(|_| errors::conversion_failed())?;

    writer.flush().map_err(|_| errors::internal())?;

    if cancel_flag.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(temp_output_path);
        return Err(errors::internal());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn image_backend_is_available() {
        assert!(available());
    }

    #[test]
    fn detects_png_jpeg_webp_signatures() {
        let png_bytes = [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13];
        assert_eq!(
            detect_format_from_bytes(&png_bytes).unwrap(),
            SupportedImageFormat::Png
        );

        let jpeg_bytes = [0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1];
        assert_eq!(
            detect_format_from_bytes(&jpeg_bytes).unwrap(),
            SupportedImageFormat::Jpeg
        );

        let webp_bytes = [
            b'R', b'I', b'F', b'F', 30, 0, 0, 0, b'W', b'E', b'B', b'P', b'V', b'P', b'8', b' ',
        ];
        assert_eq!(
            detect_format_from_bytes(&webp_bytes).unwrap(),
            SupportedImageFormat::Webp
        );

        // Reject GIF, BMP, text, etc.
        let gif_bytes = b"GIF89a...";
        assert!(detect_format_from_bytes(gif_bytes).is_err());

        let text_bytes = b"Hello world!";
        assert!(detect_format_from_bytes(text_bytes).is_err());
    }

    #[test]
    fn detects_apng_animation_chunk() {
        // Build a synthetic PNG with an acTL chunk before IDAT
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]); // signature
                                                                      // IHDR chunk: 13 bytes data + 4 bytes type + 4 bytes CRC
        buffer.extend_from_slice(&13u32.to_be_bytes());
        buffer.extend_from_slice(b"IHDR");
        buffer.extend_from_slice(&[0; 13]);
        buffer.extend_from_slice(&[0; 4]);
        // acTL chunk (8 bytes data)
        buffer.extend_from_slice(&8u32.to_be_bytes());
        buffer.extend_from_slice(b"acTL");
        buffer.extend_from_slice(&[0; 8]);
        buffer.extend_from_slice(&[0; 4]);
        // IDAT
        buffer.extend_from_slice(&0u32.to_be_bytes());
        buffer.extend_from_slice(b"IDAT");
        buffer.extend_from_slice(&[0; 4]);

        let mut cursor = Cursor::new(buffer);
        assert!(is_animated_png(&mut cursor));
    }

    #[test]
    fn regular_png_is_not_flagged_as_animated() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]); // signature
        buffer.extend_from_slice(&13u32.to_be_bytes());
        buffer.extend_from_slice(b"IHDR");
        buffer.extend_from_slice(&[0; 13]);
        buffer.extend_from_slice(&[0; 4]);
        buffer.extend_from_slice(&0u32.to_be_bytes());
        buffer.extend_from_slice(b"IDAT");
        buffer.extend_from_slice(&[0; 4]);

        let mut cursor = Cursor::new(buffer);
        assert!(!is_animated_png(&mut cursor));
    }

    #[test]
    fn detects_animated_webp_vp8x_flag() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"RIFF");
        buffer.extend_from_slice(&32u32.to_le_bytes());
        buffer.extend_from_slice(b"WEBP");
        buffer.extend_from_slice(b"VP8X");
        buffer.extend_from_slice(&10u32.to_le_bytes()); // VP8X chunk payload is 10 bytes
        buffer.extend_from_slice(&[0x02, 0, 0, 0]); // Flags: bit 1 is animation
        buffer.extend_from_slice(&[0; 6]); // remainder of VP8X

        let mut cursor = Cursor::new(buffer);
        assert!(is_animated_webp(&mut cursor));
    }

    #[test]
    fn composite_alpha_onto_white_works_accurately() {
        let mut rgba = image::RgbaImage::new(2, 2);
        // Opaque red
        rgba.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        // Fully transparent
        rgba.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
        // 50% semi-transparent black over white -> 127/128 grey
        rgba.put_pixel(0, 1, image::Rgba([0, 0, 0, 128]));
        // 50% semi-transparent blue over white
        rgba.put_pixel(1, 1, image::Rgba([0, 0, 255, 128]));

        let rgb = composite_alpha_onto_white(&rgba);
        assert_eq!(rgb.get_pixel(0, 0), &Rgb([255, 0, 0]));
        assert_eq!(rgb.get_pixel(1, 0), &Rgb([255, 255, 255]));
        // alpha composite 0 with 50% on 255: (0*128 + 255*127 + 127)/255 = 127
        let p = rgb.get_pixel(0, 1);
        assert!(p[0] >= 126 && p[0] <= 128);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
    }
}

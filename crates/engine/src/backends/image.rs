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
    codecs::jpeg::JpegEncoder, codecs::png::PngEncoder, codecs::webp::WebPEncoder, DynamicImage,
    ExtendedColorType, GrayAlphaImage, GrayImage, ImageEncoder, ImageFormat, ImageReader, Limits,
    Rgb, RgbImage, RgbaImage,
};

use loconv_contracts::{AppError, FormatId};

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

impl TryFrom<&FormatId> for SupportedImageFormat {
    type Error = AppError;

    fn try_from(id: &FormatId) -> Result<Self, Self::Error> {
        match id {
            FormatId::Png => Ok(SupportedImageFormat::Png),
            FormatId::Jpeg => Ok(SupportedImageFormat::Jpeg),
            FormatId::Webp => Ok(SupportedImageFormat::Webp),
            _ => Err(errors::unsupported_format()),
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

/// Check if a PNG stream contains an Animation Control (`acTL`) or frame chunk (APNG).
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

        if chunk_type == b"acTL" || chunk_type == b"fcTL" || chunk_type == b"fdAT" {
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

/// Check if a WebP stream contains animation (`VP8X` animation flag or `ANIM`/`ANMF` chunk).
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

        if fourcc == b"ANIM" || fourcc == b"ANMF" {
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

    let (width, height) = match reader.into_dimensions() {
        Ok(dims) => dims,
        Err(image::ImageError::Limits(_)) => return Err(errors::resource_limit()),
        Err(_) => return Err(errors::inspection_failed()),
    };

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
pub fn composite_alpha_onto_white(rgba: &RgbaImage) -> RgbImage {
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

/// Composite grayscale-with-alpha pixels onto white (for JPEG encoding without alpha channel).
pub fn composite_luma_alpha_onto_white(la: &GrayAlphaImage) -> GrayImage {
    let (width, height) = la.dimensions();
    let mut gray = GrayImage::new(width, height);

    for (x, y, pixel) in la.enumerate_pixels() {
        let [l, a] = pixel.0;
        let out_l = if a == 255 {
            l
        } else if a == 0 {
            255
        } else {
            let alpha = a as u32;
            let inv_alpha = 255 - alpha;
            ((l as u32 * alpha + 255 * inv_alpha + 127) / 255) as u8
        };
        gray.put_pixel(x, y, image::Luma([out_l]));
    }

    gray
}

/// Normalized in-memory pixel buffer ready for encoding.
enum ProcessedImageBuffer {
    Luma8(GrayImage),
    LumaA8(GrayAlphaImage),
    Rgb8(RgbImage),
    Rgba8(RgbaImage),
}

impl ProcessedImageBuffer {
    fn as_raw(&self) -> &[u8] {
        match self {
            Self::Luma8(img) => img.as_raw(),
            Self::LumaA8(img) => img.as_raw(),
            Self::Rgb8(img) => img.as_raw(),
            Self::Rgba8(img) => img.as_raw(),
        }
    }

    fn color_type(&self) -> ExtendedColorType {
        match self {
            Self::Luma8(_) => ExtendedColorType::L8,
            Self::LumaA8(_) => ExtendedColorType::La8,
            Self::Rgb8(_) => ExtendedColorType::Rgb8,
            Self::Rgba8(_) => ExtendedColorType::Rgba8,
        }
    }

    fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Luma8(img) => img.dimensions(),
            Self::LumaA8(img) => img.dimensions(),
            Self::Rgb8(img) => img.dimensions(),
            Self::Rgba8(img) => img.dimensions(),
        }
    }
}

/// Prepare decoded image pixels for the target output format, preserving color model
/// (Grayscale/RGB/RGBA) and compositing alpha onto white exclusively for JPEG.
fn prepare_image_for_encoding(
    dynamic_img: &DynamicImage,
    target_format: SupportedImageFormat,
) -> ProcessedImageBuffer {
    let color = dynamic_img.color();
    match target_format {
        SupportedImageFormat::Jpeg => {
            if color.has_alpha() {
                if !color.has_color() {
                    let la = dynamic_img.to_luma_alpha8();
                    ProcessedImageBuffer::Luma8(composite_luma_alpha_onto_white(&la))
                } else {
                    let rgba = dynamic_img.to_rgba8();
                    ProcessedImageBuffer::Rgb8(composite_alpha_onto_white(&rgba))
                }
            } else if !color.has_color() {
                ProcessedImageBuffer::Luma8(dynamic_img.to_luma8())
            } else {
                ProcessedImageBuffer::Rgb8(dynamic_img.to_rgb8())
            }
        }
        SupportedImageFormat::Png | SupportedImageFormat::Webp => {
            if !color.has_color() {
                if color.has_alpha() {
                    ProcessedImageBuffer::LumaA8(dynamic_img.to_luma_alpha8())
                } else {
                    ProcessedImageBuffer::Luma8(dynamic_img.to_luma8())
                }
            } else if color.has_alpha() {
                ProcessedImageBuffer::Rgba8(dynamic_img.to_rgba8())
            } else {
                ProcessedImageBuffer::Rgb8(dynamic_img.to_rgb8())
            }
        }
    }
}

/// Convert an input image file to a supported format (PNG, JPEG, WebP) atomically.
///
/// Bounded decode/encode respects memory and pixel limits. Auxiliary metadata is stripped.
/// Original files are read-only and never modified.
pub fn convert_image(
    input_path: &Path,
    temp_output_path: &Path,
    target_format: SupportedImageFormat,
    cancel_flag: &AtomicBool,
) -> EngineResult<()> {
    convert_image_impl(
        input_path,
        temp_output_path,
        target_format,
        cancel_flag,
        None,
    )
}

/// Encode into an exclusively created file owned by the queue worker.
pub fn convert_image_to_file(
    input_path: &Path,
    temp_output_path: &Path,
    format: &loconv_contracts::FormatId,
    cancel_flag: &AtomicBool,
    output: File,
) -> EngineResult<()> {
    let target = SupportedImageFormat::try_from(format)?;
    convert_image_impl(
        input_path,
        temp_output_path,
        target,
        cancel_flag,
        Some(output),
    )
}

fn convert_image_impl(
    input_path: &Path,
    temp_output_path: &Path,
    target_format: SupportedImageFormat,
    cancel_flag: &AtomicBool,
    output: Option<File>,
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

    let dynamic_img = match reader.decode() {
        Ok(img) => img,
        Err(image::ImageError::Limits(_)) => return Err(errors::resource_limit()),
        Err(_) => return Err(errors::conversion_failed()),
    };

    if cancel_flag.load(Ordering::SeqCst) {
        return Err(errors::internal());
    }

    let (width, height) = (dynamic_img.width(), dynamic_img.height());
    if width == 0 || height == 0 || width != details.width || height != details.height {
        return Err(errors::conversion_failed());
    }

    // 3. Process color channels based on target format requirements
    let processed = prepare_image_for_encoding(&dynamic_img, target_format);

    if cancel_flag.load(Ordering::SeqCst) {
        return Err(errors::internal());
    }

    // 4. Encode to the safe temporary file
    let temp_file = match output {
        Some(file) => file,
        None => std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temp_output_path)
            .map_err(|_| errors::internal())?,
    };

    let mut writer = BufWriter::new(temp_file);
    let (buf_width, buf_height) = processed.dimensions();
    let raw = processed.as_raw();
    let color_type = processed.color_type();

    let encode_res = match target_format {
        SupportedImageFormat::Jpeg => {
            let encoder = JpegEncoder::new_with_quality(&mut writer, JPEG_QUALITY_PRESET);
            encoder.write_image(raw, buf_width, buf_height, color_type)
        }
        SupportedImageFormat::Png => {
            let encoder = PngEncoder::new(&mut writer);
            encoder.write_image(raw, buf_width, buf_height, color_type)
        }
        SupportedImageFormat::Webp => {
            let encoder = WebPEncoder::new_lossless(&mut writer);
            encoder.write_image(raw, buf_width, buf_height, color_type)
        }
    };

    if encode_res.is_err() {
        let _ = std::fs::remove_file(temp_output_path);
        return Err(errors::conversion_failed());
    }

    if writer.flush().is_err() {
        let _ = std::fs::remove_file(temp_output_path);
        return Err(errors::internal());
    }

    drop(writer);

    if cancel_flag.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(temp_output_path);
        return Err(errors::internal());
    }

    Ok(())
}

/// Convert an image by its wire `FormatId`.
pub fn convert_image_by_format_id(
    input_path: &Path,
    temp_output_path: &Path,
    format_id: &FormatId,
    cancel_flag: &AtomicBool,
) -> EngineResult<()> {
    let target_format = SupportedImageFormat::try_from(format_id)?;
    convert_image(input_path, temp_output_path, target_format, cancel_flag)
}

/// Execute real image conversion to JPEG with fixed quality preset (85) and composite alpha onto white.
pub fn convert_image_to_jpeg(
    input_path: &Path,
    temp_output_path: &Path,
    cancel_flag: &AtomicBool,
) -> EngineResult<()> {
    convert_image(
        input_path,
        temp_output_path,
        SupportedImageFormat::Jpeg,
        cancel_flag,
    )
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
    fn format_id_conversion() {
        assert_eq!(
            SupportedImageFormat::try_from(&FormatId::Png).unwrap(),
            SupportedImageFormat::Png
        );
        assert_eq!(
            SupportedImageFormat::try_from(&FormatId::Jpeg).unwrap(),
            SupportedImageFormat::Jpeg
        );
        assert_eq!(
            SupportedImageFormat::try_from(&FormatId::Webp).unwrap(),
            SupportedImageFormat::Webp
        );
        assert!(SupportedImageFormat::try_from(&FormatId::Mp4H264Aac).is_err());
        assert!(SupportedImageFormat::try_from(&FormatId::Mp3).is_err());
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
        buffer.extend_from_slice(&13u32.to_be_bytes()); // IHDR
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
    fn detects_animated_webp_anim_chunk() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"RIFF");
        buffer.extend_from_slice(&32u32.to_le_bytes());
        buffer.extend_from_slice(b"WEBP");
        buffer.extend_from_slice(b"ANIM");
        buffer.extend_from_slice(&6u32.to_le_bytes());
        buffer.extend_from_slice(&[0; 6]);

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
        let p = rgb.get_pixel(0, 1);
        assert!((126..=128).contains(&p[0]));
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
    }

    #[test]
    fn composite_luma_alpha_onto_white_works_accurately() {
        let mut la = GrayAlphaImage::new(2, 2);
        la.put_pixel(0, 0, image::LumaA([100, 255]));
        la.put_pixel(1, 0, image::LumaA([0, 0]));
        la.put_pixel(0, 1, image::LumaA([0, 128]));
        la.put_pixel(1, 1, image::LumaA([255, 128]));

        let gray = composite_luma_alpha_onto_white(&la);
        assert_eq!(gray.get_pixel(0, 0).0[0], 100);
        assert_eq!(gray.get_pixel(1, 0).0[0], 255);
        let p01 = gray.get_pixel(0, 1).0[0];
        assert!((126..=128).contains(&p01));
        assert_eq!(gray.get_pixel(1, 1).0[0], 255);
    }

    #[test]
    fn conversion_matrix_all_combinations_and_color_models() {
        let dir = std::env::temp_dir().join(format!("loconv-matrix-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let cancel = AtomicBool::new(false);
        let formats = [
            SupportedImageFormat::Png,
            SupportedImageFormat::Jpeg,
            SupportedImageFormat::Webp,
        ];

        // Create 3 synthetic inputs: PNG, JPEG, WebP
        let png_input = dir.join("in.png");
        let jpeg_input = dir.join("in.jpg");
        let webp_input = dir.join("in.webp");

        let img_rgb = RgbImage::from_fn(16, 12, |x, y| Rgb([(x * 15) as u8, (y * 20) as u8, 120]));
        img_rgb
            .save_with_format(&png_input, ImageFormat::Png)
            .unwrap();
        img_rgb
            .save_with_format(&jpeg_input, ImageFormat::Jpeg)
            .unwrap();
        img_rgb
            .save_with_format(&webp_input, ImageFormat::WebP)
            .unwrap();

        let original_png_bytes = std::fs::read(&png_input).unwrap();
        let original_jpeg_bytes = std::fs::read(&jpeg_input).unwrap();
        let original_webp_bytes = std::fs::read(&webp_input).unwrap();

        let inputs = [
            (&png_input, &original_png_bytes),
            (&jpeg_input, &original_jpeg_bytes),
            (&webp_input, &original_webp_bytes),
        ];

        for (in_path, orig_bytes) in &inputs {
            for out_fmt in &formats {
                let out_name = format!("out-{:?}.tmp", out_fmt);
                let out_path = dir.join(&out_name);

                convert_image(in_path, &out_path, *out_fmt, &cancel).unwrap();

                assert!(out_path.exists());
                let meta = std::fs::metadata(&out_path).unwrap();
                assert!(meta.len() > 0);

                // Verify output format and dimensions
                let decoded = ImageReader::open(&out_path)
                    .unwrap()
                    .with_guessed_format()
                    .unwrap()
                    .decode()
                    .unwrap();
                assert_eq!(decoded.width(), 16);
                assert_eq!(decoded.height(), 12);

                // Verify input was not touched
                let current_in_bytes = std::fs::read(in_path).unwrap();
                assert_eq!(*orig_bytes, &current_in_bytes);

                let _ = std::fs::remove_file(out_path);
            }
        }

        // Test RGBA input to JPEG (compositing alpha over white)
        let rgba_input = dir.join("rgba.png");
        let mut rgba_img = RgbaImage::new(10, 10);
        for x in 0..10 {
            for y in 0..10 {
                // Red with 0% opacity (transparent) on left half, 100% opacity on right
                let a = if x < 5 { 0 } else { 255 };
                rgba_img.put_pixel(x, y, image::Rgba([255, 0, 0, a]));
            }
        }
        rgba_img
            .save_with_format(&rgba_input, ImageFormat::Png)
            .unwrap();

        let jpeg_out = dir.join("from_rgba.jpg");
        convert_image(&rgba_input, &jpeg_out, SupportedImageFormat::Jpeg, &cancel).unwrap();
        let decoded_jpeg = ImageReader::open(&jpeg_out)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap()
            .to_rgb8();
        // Left half (alpha=0) composited over white: should be near pure white [255, 255, 255] (allowing JPEG DCT lossy variance)
        let p_left = decoded_jpeg.get_pixel(2, 5);
        assert!(p_left.0[0] >= 250);
        assert!(p_left.0[1] >= 250);
        assert!(p_left.0[2] >= 250);
        // Right half (alpha=255) should be red [255, 0, 0] (allowing JPEG lossy variance)
        let p_right = decoded_jpeg.get_pixel(7, 5);
        assert!(p_right.0[0] >= 250);
        assert!(p_right.0[1] <= 5);
        assert!(p_right.0[2] <= 5);

        // Test Grayscale (L8) input to PNG, JPEG, WebP
        let gray_input = dir.join("gray.png");
        let gray_img = GrayImage::from_fn(8, 8, |x, y| image::Luma([(x * 30 + y * 5) as u8]));
        gray_img
            .save_with_format(&gray_input, ImageFormat::Png)
            .unwrap();

        for out_fmt in &formats {
            let out_file = dir.join(format!("gray_out_{:?}.tmp", out_fmt));
            convert_image(&gray_input, &out_file, *out_fmt, &cancel).unwrap();
            let decoded = ImageReader::open(&out_file)
                .unwrap()
                .with_guessed_format()
                .unwrap()
                .decode()
                .unwrap();
            assert_eq!(decoded.width(), 8);
            assert_eq!(decoded.height(), 8);
            let _ = std::fs::remove_file(out_file);
        }

        // Test Grayscale with Alpha (La8) to JPEG
        let la_input = dir.join("la.png");
        let la_img = GrayAlphaImage::from_fn(8, 8, |x, _y| {
            if x < 4 {
                image::LumaA([0, 0]) // transparent black -> white
            } else {
                image::LumaA([50, 255]) // opaque dark grey
            }
        });
        la_img
            .save_with_format(&la_input, ImageFormat::Png)
            .unwrap();

        let la_jpeg_out = dir.join("la_out.jpg");
        convert_image(&la_input, &la_jpeg_out, SupportedImageFormat::Jpeg, &cancel).unwrap();
        let decoded_la_jpeg = ImageReader::open(&la_jpeg_out)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(decoded_la_jpeg.width(), 8);
        assert_eq!(decoded_la_jpeg.height(), 8);

        // Test cancellation cleanup
        let cancel_true = AtomicBool::new(true);
        let cancel_out = dir.join("cancelled.png");
        let res = convert_image(
            &png_input,
            &cancel_out,
            SupportedImageFormat::Png,
            &cancel_true,
        );
        assert!(res.is_err());
        assert!(
            !cancel_out.exists(),
            "Cancelled job must clean up temporary output"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

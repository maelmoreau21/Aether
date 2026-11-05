//! Bounded raster import shared by artwork, avatars and provider portraits.
//! CPU work runs on blocking workers; callers acquire DECODERS before scheduling it.
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum ImportError {
    #[error("unsupported raster format or mismatched MIME")]
    Unsupported,
    #[error("malformed raster, colour profile or allocation/encoding limit")]
    Invalid,
}
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;
#[cfg(test)]
mod tests;

pub static DECODERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Reuse an already validated static WebP without recompressing it. Metadata,
/// animation and unknown RIFF chunks require normalization instead.
pub fn reusable_webp(bytes: &[u8], lossless: bool) -> bool {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return false;
    }
    let Some(length) = bytes
        .get(4..8)
        .and_then(|v| <[u8; 4]>::try_from(v).ok())
        .map(u32::from_le_bytes)
    else {
        return false;
    };
    if u64::from(length) + 8 != bytes.len() as u64 {
        return false;
    }
    let mut offset = 12usize;
    let mut pixels = 0;
    let mut is_lossless = false;
    while offset < bytes.len() {
        let Some(header) = bytes.get(offset..offset + 8) else {
            return false;
        };
        match &header[..4] {
            b"VP8L" => {
                is_lossless = true;
                pixels += 1;
            }
            b"VP8 " => {
                pixels += 1;
            }
            b"VP8X" | b"ALPH" => {}
            _ => return false,
        }
        let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let Some(next) = offset
            .checked_add(8)
            .and_then(|v| v.checked_add(length))
            .and_then(|v| v.checked_add(length % 2))
        else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        offset = next;
    }
    pixels == 1 && (!lossless || is_lossless)
}

pub fn decode(bytes: &[u8], content: &str) -> Result<DynamicImage, ImportError> {
    let invalid = ImportError::Invalid;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid)?;
    let expected = match reader.format() {
        Some(ImageFormat::Png) => "image/png",
        Some(ImageFormat::Jpeg) => "image/jpeg",
        Some(ImageFormat::WebP) => "image/webp",
        _ => return Err(ImportError::Unsupported),
    };
    if content != expected {
        return Err(ImportError::Unsupported);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| invalid)?;
    if decoder.total_bytes() > 32 * 1024 * 1024 {
        return Err(invalid);
    }
    let orientation = decoder.orientation().map_err(|_| invalid)?;
    let profile = decoder.icc_profile().map_err(|_| invalid)?;
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(|_| invalid)?;
    decoded.apply_orientation(orientation);
    if let Some(profile) = profile {
        if profile.len() > 1024 * 1024 {
            return Err(invalid);
        }
        // Normalize tagged RGB to sRGB before dropping metadata. Never silently
        // strip an unrecognised profile and display its pixels as sRGB.
        let source = lcms2::Profile::new_icc(&profile).map_err(|_| invalid)?;
        let destination = lcms2::Profile::new_srgb();
        let transform = lcms2::Transform::<u8, u8>::new(
            &source,
            lcms2::PixelFormat::RGB_8,
            &destination,
            lcms2::PixelFormat::RGB_8,
            lcms2::Intent::Perceptual,
        )
        .map_err(|_| invalid)?;
        let mut rgb = decoded.to_rgb8();
        transform.transform_in_place(rgb.as_mut());
        if decoded.color().has_alpha() {
            let mut rgba = decoded.to_rgba8();
            for (pixel, colour) in rgba.pixels_mut().zip(rgb.pixels()) {
                pixel.0[..3].copy_from_slice(&colour.0);
            }
            decoded = DynamicImage::ImageRgba8(rgba);
        } else {
            decoded = DynamicImage::ImageRgb8(rgb);
        }
    }
    Ok(decoded)
}

pub fn encode(image: &DynamicImage, lossless: bool) -> Result<Vec<u8>, ImportError> {
    let invalid = ImportError::Invalid;
    let bytes = if lossless || image.color().has_alpha() {
        // The Rust encoder preserves alpha and the RGB of transparent pixels.
        let mut output = Cursor::new(Vec::new());
        image
            .write_to(&mut output, ImageFormat::WebP)
            .map_err(|_| invalid)?;
        output.into_inner()
    } else {
        let rgb = image.to_rgb8();
        webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height())
            .encode_simple(false, 82.0)
            .map_err(|_| invalid)?
            .to_vec()
    };
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(invalid);
    }
    // Validate before publishing. No unvalidated original survives the import.
    let (width, height) = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| invalid)?
        .into_dimensions()
        .map_err(|_| invalid)?;
    if (width, height) != (image.width(), image.height()) {
        return Err(invalid);
    }
    Ok(bytes)
}

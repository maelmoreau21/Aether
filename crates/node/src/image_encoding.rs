//! Bounded raster import shared by artwork, avatars and provider portraits.
//! CPU work runs on blocking workers; callers acquire DECODERS before scheduling it.
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum ImportError {
    #[error("unsupported raster format or mismatched MIME")]
    Unsupported,
    #[error("malformed raster, colour profile or allocation/encoding limit")]
    Invalid,
}
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, imageops::FilterType};
use std::{io::Cursor, sync::LazyLock};
#[cfg(test)]
mod tests;

/// Target concurrency for blocking image decode/encode work, scaled to CPU cores.
pub fn decoder_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(4, 16))
        .unwrap_or(8)
}

pub static DECODERS: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(decoder_concurrency()));

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

/// Aspect-fit dimensions within `(max_width, max_height)` without upscaling.
pub fn fit_dimensions(
    src_width: u32,
    src_height: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32) {
    if src_width == 0
        || src_height == 0
        || max_width == 0
        || max_height == 0
        || (src_width <= max_width && src_height <= max_height)
    {
        return (src_width, src_height);
    }
    let wratio = f64::from(max_width) / f64::from(src_width);
    let hratio = f64::from(max_height) / f64::from(src_height);
    let ratio = f64::min(wratio, hratio);
    let target_w = u32::try_from(
        ((f64::from(src_width) * ratio).round() as u64).clamp(1, u64::from(max_width)),
    )
    .unwrap_or(1);
    let target_h = u32::try_from(
        ((f64::from(src_height) * ratio).round() as u64).clamp(1, u64::from(max_height)),
    )
    .unwrap_or(1);
    (target_w, target_h)
}

/// Resize `image` to fit within `(max_width, max_height)` without upscaling.
/// Uses faster filters (`CatmullRom`, with a `Triangle` pre-reduction step above 4x)
/// when downscaling heavily so 4K/8K artwork encodes without stalling CPU cores.
pub fn resize_bounded(image: &DynamicImage, max_width: u32, max_height: u32) -> DynamicImage {
    let src_w = image.width();
    let src_h = image.height();
    let (target_w, target_h) = fit_dimensions(src_w, src_h, max_width, max_height);
    if target_w == src_w && target_h == src_h {
        return image.clone();
    }
    let downscale = (f64::from(src_w) / f64::from(target_w))
        .max(f64::from(src_h) / f64::from(target_h));
    if downscale > 4.0 {
        let mid_w = target_w.saturating_mul(2).min(src_w);
        let mid_h = target_h.saturating_mul(2).min(src_h);
        if mid_w > target_w && mid_h > target_h && mid_w < src_w && mid_h < src_h {
            let reduced = image.resize_exact(mid_w, mid_h, FilterType::Triangle);
            return reduced.resize_exact(target_w, target_h, FilterType::CatmullRom);
        }
    }
    let filter = if downscale > 2.0 {
        FilterType::CatmullRom
    } else {
        FilterType::Lanczos3
    };
    image.resize_exact(target_w, target_h, filter)
}

pub fn encode(image: &DynamicImage, lossless: bool) -> Result<Vec<u8>, ImportError> {
    let invalid = ImportError::Invalid;
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err(invalid);
    }
    let bytes = if lossless || image.color().has_alpha() {
        // The Rust encoder preserves alpha and the RGB of transparent pixels.
        let mut output = Cursor::new(Vec::new());
        image
            .write_to(&mut output, ImageFormat::WebP)
            .map_err(|_| invalid)?;
        output.into_inner()
    } else if let Some(rgb) = image.as_rgb8() {
        webp::Encoder::from_rgb(rgb.as_raw(), width, height)
            .encode_simple(false, 82.0)
            .map_err(|_| invalid)?
            .to_vec()
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
    // Dimensions are fixed by the input raster; check only the WebP RIFF signature.
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return Err(invalid);
    }
    Ok(bytes)
}

#[cfg(test)]
mod optimization_tests {
    use super::*;

    #[tokio::test]
    async fn decoder_semaphore_scales_above_two() {
        let concurrency = decoder_concurrency();
        assert!((4..=16).contains(&concurrency));
        let permits = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            DECODERS.acquire_many(3),
        )
        .await
        .expect("decoder semaphore should allow > 2 concurrent permits")
        .expect("decoder semaphore should remain open");
        drop(permits);
    }

    #[test]
    fn resize_bounded_uses_fast_downscale_without_upscaling() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2400,
            3600,
            image::Rgb([12, 34, 56]),
        ));
        let thumb = resize_bounded(&img, 320, 640);
        assert_eq!((thumb.width(), thumb.height()), (320, 480));
        let webp = encode(&thumb, false).unwrap();
        assert_eq!(webp.get(..4), Some(b"RIFF".as_slice()));
        assert_eq!(webp.get(8..12), Some(b"WEBP".as_slice()));

        let small = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            200,
            300,
            image::Rgb([90, 10, 20]),
        ));
        let unchanged = resize_bounded(&small, 320, 640);
        assert_eq!((unchanged.width(), unchanged.height()), (200, 300));
    }
}

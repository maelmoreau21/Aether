use super::*;
use image::{GenericImageView, ImageEncoder};

#[test]
fn jpeg_png_and_webp_import_as_valid_webp_with_correct_colour_and_ratio() {
    let image = DynamicImage::ImageRgb8(image::RgbImage::from_fn(120, 180, |x, y| {
        image::Rgb([(x * 2) as u8, y as u8, 80])
    }));
    for (format, mime) in [
        (ImageFormat::Jpeg, "image/jpeg"),
        (ImageFormat::Png, "image/png"),
        (ImageFormat::WebP, "image/webp"),
    ] {
        let mut input = Cursor::new(Vec::new());
        image.write_to(&mut input, format).unwrap();
        let decoded = decode(input.get_ref(), mime).unwrap();
        let bytes = encode(&decoded, false).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::WebP);
        let output = image::load_from_memory(&bytes).unwrap();
        assert_eq!(output.dimensions(), (120, 180));
        let (before, after) = (decoded.to_rgb8(), output.to_rgb8());
        let mse = before
            .as_raw()
            .iter()
            .zip(after.as_raw())
            .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
            .sum::<f64>()
            / before.as_raw().len() as f64;
        assert!(mse < 40.0, "mse={mse}");
    }
    assert!(decode(b"broken", "image/png").is_err());
    assert!(decode(b"<svg/>", "image/svg+xml").is_err());
}
#[test]
fn lossless_webp_preserves_every_rgba_channel_including_fully_transparent_pixels() {
    let source = image::RgbaImage::from_fn(32, 48, |x, y| {
        image::Rgba([
            (x * 7) as u8,
            (y * 5) as u8,
            200,
            if x < 8 { 0 } else { (y * 5) as u8 },
        ])
    });
    let encoded = encode(&DynamicImage::ImageRgba8(source.clone()), true).unwrap();
    assert_eq!(
        image::load_from_memory(&encoded).unwrap().to_rgba8(),
        source
    );
}
#[test]
fn tagged_srgb_is_normalized_and_malformed_icc_is_refused() {
    let rgb = image::RgbImage::from_pixel(16, 24, image::Rgb([240, 80, 50]));
    for valid in [true, false] {
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
        encoder
            .set_icc_profile(if valid {
                lcms2::Profile::new_srgb().icc().unwrap()
            } else {
                b"invalid colour profile".to_vec()
            })
            .unwrap();
        encoder
            .write_image(rgb.as_raw(), 16, 24, image::ExtendedColorType::Rgb8)
            .unwrap();
        let decoded = decode(&bytes, "image/png");
        if valid {
            assert_eq!(decoded.unwrap().to_rgb8(), rgb);
        } else {
            assert!(decoded.is_err());
        }
    }
}
#[test]
fn exif_orientation_is_applied_before_dimensions_and_metadata_are_removed() {
    let source = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        12,
        18,
        image::Rgb([30, 90, 180]),
    ));
    let mut jpeg = Cursor::new(Vec::new());
    source.write_to(&mut jpeg, ImageFormat::Jpeg).unwrap();
    // TIFF little-endian IFD0 Orientation=6 (90 degrees clockwise).
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut tagged = vec![0xff, 0xd8, 0xff, 0xe1];
    tagged.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    tagged.extend_from_slice(exif);
    tagged.extend_from_slice(&jpeg.get_ref()[2..]);
    assert_eq!(
        decode(&tagged, "image/jpeg").unwrap().dimensions(),
        (18, 12)
    );
}

#[test]
#[ignore = "explicit synthetic before/after benchmark; run via scripts/measure-artwork-webp.ps1"]
fn measure_artwork_encoding() {
    let mode = std::env::var("MEDIA_ARTWORK_BENCH_MODE").unwrap_or_else(|_| "webp".into());
    let dir = std::path::PathBuf::from(std::env::var("MEDIA_ARTWORK_BENCH_DIR").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let photo = DynamicImage::ImageRgb8(image::RgbImage::from_fn(960, 1440, |x, y| {
        let texture = ((x.wrapping_mul(13) ^ y.wrapping_mul(7)) % 19) as u8;
        image::Rgb([
            ((x * 150 / 960) as u8).saturating_add(texture),
            ((y * 180 / 1440) as u8).saturating_add(texture),
            ((x + y) * 100 / 2400) as u8 + texture,
        ])
    }));
    let logo = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(640, 320, |x, y| {
        image::Rgba([
            30,
            120,
            220,
            if x > 40 && x < 600 && y > 40 && y < 280 {
                255
            } else {
                0
            },
        ])
    }));
    let started = std::time::Instant::now();
    let mut report = Vec::new();
    for (name, source) in [("photo", photo), ("logo", logo)] {
        let input = {
            let mut buffer = Cursor::new(Vec::new());
            source.write_to(&mut buffer, ImageFormat::Png).unwrap();
            buffer.into_inner()
        };
        let mut total = 0;
        let mut unique = std::collections::HashMap::new();
        let begin = std::time::Instant::now();
        let mut psnr = 0.0;
        for iteration in 0..3 {
            let image = decode(&input, "image/png").unwrap();
            let mut original_webp = None::<Vec<u8>>;
            for width in [0, 320, 640, 1280] {
                let resized = if width == 0 || width >= image.width() {
                    image.clone()
                } else {
                    image.thumbnail(width, 4096)
                };
                let (bytes, extension) = if mode == "legacy" {
                    if resized.color().has_alpha() {
                        let mut out = Cursor::new(Vec::new());
                        resized.write_to(&mut out, ImageFormat::Png).unwrap();
                        (out.into_inner(), "png")
                    } else {
                        let mut out = Vec::new();
                        image::codecs::jpeg::JpegEncoder::new_with_quality(
                            &mut out,
                            if width == 0 { 90 } else { 78 },
                        )
                        .encode_image(&resized)
                        .unwrap();
                        (out, "jpg")
                    }
                } else {
                    let bytes = if width > 0 && width >= image.width() {
                        original_webp.clone().unwrap()
                    } else {
                        encode(&resized, name == "logo").unwrap()
                    };
                    if width == 0 {
                        original_webp = Some(bytes.clone());
                    }
                    (bytes, "webp")
                };
                total += bytes.len();
                unique.insert(media_contracts::protocol::sha256_hex(&bytes), bytes.len());
                if iteration == 0 && width == 0 {
                    std::fs::write(dir.join(format!("{name}-{mode}.{extension}")), &bytes).unwrap();
                    let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
                    let original = resized.to_rgb8();
                    let mse = original
                        .as_raw()
                        .iter()
                        .zip(decoded.as_raw())
                        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
                        .sum::<f64>()
                        / original.as_raw().len() as f64;
                    psnr = if mse == 0.0 {
                        100.0
                    } else {
                        10.0 * (255.0f64.powi(2) / mse).log10()
                    };
                }
            }
        }
        report.push(serde_json::json!({"fixture":name,"threeImportRepresentationBytes":total,"uniqueStoredBytes":unique.values().sum::<usize>(),"wallMs":begin.elapsed().as_secs_f64()*1000.0,"originalRgbPsnrDb":psnr}));
    }
    let report = serde_json::json!({"mode":mode,"wallMs":started.elapsed().as_secs_f64()*1000.0,"iterations":3,"representations":[0,320,640,1280],"results":report});
    std::fs::write(
        dir.join(format!("{mode}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}

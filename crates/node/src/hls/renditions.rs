//! Reduced video renditions (`hls-v{v}-h{height}…`, or `-t{height}` with HDR
//! tone mapped to SDR): H.264 re-encodes cut on the same keyframes as the
//! packet-copied rendition, so they share its audio renditions and segment
//! numbering. They let a phone keep a film without storing a 50 GB original.

/// A bounding box and the H.264 bitrate spent on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rendition {
    pub width: u32,
    pub height: u32,
    pub kbps: u32,
}

/// Largest first, the order the master playlist lists them in.
pub const RENDITIONS: [Rendition; 2] = [
    Rendition {
        width: 1920,
        height: 1080,
        kbps: 8_000,
    },
    Rendition {
        width: 1280,
        height: 720,
        kbps: 4_000,
    },
];

pub fn rendition(height: u32) -> Option<Rendition> {
    RENDITIONS.into_iter().find(|r| r.height == height)
}

/// FFmpeg `scale` arguments that fit the picture in the box, keeping its aspect.
pub fn scale_filter(rendition: Rendition) -> String {
    format!(
        "scale=w='min(iw,{})':h='min(ih,{})':force_original_aspect_ratio=decrease:force_divisible_by=2",
        rendition.width, rendition.height
    )
}

fn rescale(a: u64, b: u64, c: u64) -> u64 {
    (a * b + c / 2) / c
}

/// The picture size `scale_filter` produces from a `width`×`height` source
/// (FFmpeg's `ff_scale_adjust_dimensions`, decrease mode, divisible by two).
pub fn fit(width: u32, height: u32, rendition: Rendition) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let (source_w, source_h) = (u64::from(width), u64::from(height));
    let w = source_w.min(u64::from(rendition.width));
    let h = source_h.min(u64::from(rendition.height));
    let even_w = rescale(h, source_w, source_h * 2) * 2;
    let even_h = rescale(w, source_h, source_w * 2) * 2;
    let w = even_w.min(w) / 2 * 2;
    let h = even_h.min(h) / 2 * 2;
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_fit_the_box_and_keep_their_aspect() {
        let [full_hd, hd] = RENDITIONS;
        assert_eq!(fit(3840, 2160, full_hd), Some((1920, 1080)));
        assert_eq!(fit(3840, 2160, hd), Some((1280, 720)));
        // Scope: bounded by the width, not stretched to 1080 lines.
        assert_eq!(fit(3840, 1600, full_hd), Some((1920, 800)));
        assert_eq!(fit(1920, 804, hd), Some((1280, 536)));
        // 4:3 is bounded by the height.
        assert_eq!(fit(1440, 1080, hd), Some((960, 720)));
        // Never enlarged.
        assert_eq!(fit(640, 360, hd), Some((640, 360)));
        assert_eq!(fit(0, 1080, hd), None);
        assert_eq!(rendition(720), Some(hd));
        assert_eq!(rendition(1440), None);
    }
}

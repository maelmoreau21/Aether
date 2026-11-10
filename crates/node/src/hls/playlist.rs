//! VOD playlist cut on real keyframes, so every segment can be produced on
//! its own by a packet copy, in any order, without a running session.
use super::keyframes::Keyframes;
use std::fmt::Write;

/// Apple's recommended segment length.
pub const TARGET_MS: u64 = 6_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Greedy grouping: a segment ends on the first keyframe at least `target_ms`
/// after its start. A last segment shorter than a third of the target is
/// folded into the previous one.
pub fn segments(keyframes: &Keyframes, target_ms: u64) -> Vec<Segment> {
    let mut result = Vec::new();
    let Some(&first) = keyframes.times_ms.first() else {
        return result;
    };
    let mut start = first;
    for &time in &keyframes.times_ms[1..] {
        if time >= start + target_ms && time < keyframes.duration_ms {
            result.push(Segment {
                start_ms: start,
                end_ms: time,
            });
            start = time;
        }
    }
    let end = keyframes.duration_ms.max(start + 1);
    match result.last_mut() {
        Some(previous) if end - start < target_ms / 3 => previous.end_ms = end,
        _ => result.push(Segment {
            start_ms: start,
            end_ms: end,
        }),
    }
    result
}

/// HLS v7 VOD playlist with an fMP4 initialization section. URIs are relative,
/// so the playlist keeps whatever path-embedded authorization it was served from.
pub fn render(
    segments: &[Segment],
    init_uri: &str,
    segment_uri: impl Fn(usize) -> String,
) -> String {
    let target = segments
        .iter()
        .map(|s| (s.end_ms - s.start_ms).div_ceil(1000))
        .max()
        .unwrap_or(1)
        .max(1);
    let mut out = String::with_capacity(96 + segments.len() * 48);
    let _ = write!(
        out,
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:0\n\
         #EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MAP:URI=\"{init_uri}\"\n"
    );
    for (index, segment) in segments.iter().enumerate() {
        let millis = segment.end_ms - segment.start_ms;
        let _ = write!(
            out,
            "#EXTINF:{}.{:03},\n{}\n",
            millis / 1000,
            millis % 1000,
            segment_uri(index)
        );
    }
    out.push_str("#EXT-X-ENDLIST\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(times: &[u64], duration: u64) -> Keyframes {
        Keyframes {
            times_ms: times.to_vec(),
            duration_ms: duration,
            hevc: false,
        }
    }

    #[test]
    fn segments_end_on_keyframes_and_cover_the_whole_duration() {
        let every_two = frames(&(0..=30).map(|i| i * 2_000).collect::<Vec<_>>(), 61_000);
        let cut = segments(&every_two, TARGET_MS);
        assert_eq!(cut.first().unwrap().start_ms, 0);
        assert_eq!(cut.last().unwrap().end_ms, 61_000);
        for pair in cut.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms);
            assert!(every_two.times_ms.contains(&pair[1].start_ms));
        }
        assert!(
            cut[..cut.len() - 1]
                .iter()
                .all(|s| s.end_ms - s.start_ms == 6_000)
        );
    }

    #[test]
    fn sparse_keyframes_give_longer_segments_and_a_matching_target_duration() {
        let sparse = frames(&[0, 1_000, 11_000, 12_000, 30_000], 30_500);
        let cut = segments(&sparse, TARGET_MS);
        assert_eq!(
            cut,
            vec![
                Segment {
                    start_ms: 0,
                    end_ms: 11_000
                },
                Segment {
                    start_ms: 11_000,
                    end_ms: 30_500
                },
            ]
        );
        let text = render(&cut, "init.mp4", |i| format!("{i}.m4s"));
        assert!(text.starts_with("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:20\n"));
        assert!(text.contains(
            "#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:11.000,\n0.m4s\n#EXTINF:19.500,\n1.m4s\n"
        ));
        assert!(text.ends_with("#EXT-X-ENDLIST\n"));
    }

    #[test]
    fn a_single_keyframe_is_one_segment() {
        assert_eq!(
            segments(&frames(&[0], 4_200), TARGET_MS),
            vec![Segment {
                start_ms: 0,
                end_ms: 4_200
            }]
        );
    }
}

//! Media Node local fixture and root inventory management.
//! Completely independent of Core users, sessions, roles, and SQLite.

use crate::{
    files::{OpenedFile, validate_library_root},
    probe,
};
pub use media_contracts::inventory::{Descriptor, LanguageRemux, RepresentativeCase};
use std::{
    io,
    path::{Path, PathBuf},
};

// Explicit synthetic inventory for the fixture bench. These are stable fixture IDs,
// not work/source identities or a persistent production catalogue.
pub const FIXTURES: [(&str, &str, &str); 4] = [
    ("fixture-mkv", "av1-opus.mkv", "video/x-matroska"),
    ("fixture-mp4", "av1-opus.mp4", "video/mp4"),
    ("fixture-webm", "av1-opus.webm", "video/webm"),
    ("fixture-fmp4", "av1-opus-fragmented.mp4", "video/mp4"),
];

pub const REMUX_FIXTURES: [(&str, &str, &str); 4] = [
    (
        "fixture-mkv-fr",
        "language-remux/av1-opus-mkv-fr.mkv",
        "video/x-matroska",
    ),
    (
        "fixture-mkv-en",
        "language-remux/av1-opus-mkv-en.mkv",
        "video/x-matroska",
    ),
    (
        "fixture-mp4-fr",
        "language-remux/av1-opus-mp4-fr.mp4",
        "video/mp4",
    ),
    (
        "fixture-mp4-en",
        "language-remux/av1-opus-mp4-en.mp4",
        "video/mp4",
    ),
];

pub const DEMO_FIXTURES: [(&str, &str, &str); 4] = [
    ("demo-mkv-fr", "p1/clip-mkv-fr.mkv", "video/x-matroska"),
    ("demo-mkv-en", "p1/clip-mkv-en.mkv", "video/x-matroska"),
    ("demo-mp4-fr", "p1/clip-mp4-fr.mp4", "video/mp4"),
    ("demo-mp4-en", "p1/clip-mp4-en.mp4", "video/mp4"),
];

pub const REPRESENTATIVE_FIXTURES: [(&str, &str, &str); 4] = [
    (
        "fixture-representative-mkv",
        "representative/representative.mkv",
        "video/x-matroska",
    ),
    (
        "fixture-representative-mp4",
        "representative/representative.mp4",
        "video/mp4",
    ),
    (
        "fixture-representative-mkv-en",
        "representative/representative-en.mkv",
        "video/x-matroska",
    ),
    (
        "fixture-representative-mp4-en",
        "representative/representative-en.mp4",
        "video/mp4",
    ),
];

struct Asset {
    descriptor: Descriptor,
    relative: PathBuf,
}

pub struct Inventory {
    root: PathBuf,
    assets: Vec<Asset>,
}

impl Inventory {
    /// Standalone local Node without loading or probing experimental fixtures.
    pub async fn local(root: PathBuf) -> io::Result<Self> {
        let root = validate_library_root(&root, &root).await?;
        Ok(Self {
            root,
            assets: Vec::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_fixture_bench(&self) -> bool {
        !self.assets.is_empty()
    }

    pub async fn load(root: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let root = tokio::fs::canonicalize(root).await?;
        let mut assets = Vec::new();
        let mut fixtures = FIXTURES.to_vec();
        if tokio::fs::try_exists(root.join("language-remux")).await? {
            fixtures.extend(REMUX_FIXTURES);
        }
        if tokio::fs::try_exists(root.join("representative")).await? {
            fixtures.extend(REPRESENTATIVE_FIXTURES);
        }
        if tokio::fs::try_exists(root.join("p1")).await? {
            fixtures.extend(DEMO_FIXTURES);
        }
        for (id, name, mime) in fixtures {
            let relative = PathBuf::from(name);
            let opened = OpenedFile::open_in_root(&root, &relative).await?;
            let probe = probe::probe_opened(&opened).await?;
            let language_remux = match id {
                "demo-mkv-fr" | "demo-mp4-fr" | "demo-mkv-en" | "demo-mp4-en" => {
                    Some(LanguageRemux {
                        source_asset: if id.contains("mkv") {
                            "fixture-representative-mkv"
                        } else {
                            "fixture-representative-mp4"
                        },
                        language: if id.ends_with("fr") { "fr" } else { "en" },
                        source_audio_stream: if id.ends_with("fr") { 1 } else { 2 },
                    })
                }
                "fixture-mkv-fr" => Some(LanguageRemux {
                    source_asset: "fixture-mkv",
                    language: "fr",
                    source_audio_stream: 1,
                }),
                "fixture-mkv-en" => Some(LanguageRemux {
                    source_asset: "fixture-mkv",
                    language: "en",
                    source_audio_stream: 2,
                }),
                "fixture-mp4-fr" => Some(LanguageRemux {
                    source_asset: "fixture-mp4",
                    language: "fr",
                    source_audio_stream: 1,
                }),
                "fixture-mp4-en" => Some(LanguageRemux {
                    source_asset: "fixture-mp4",
                    language: "en",
                    source_audio_stream: 2,
                }),
                _ => None,
            };
            if let Some(remux) = &language_remux {
                let expected_language = if remux.language == "fr" { "fra" } else { "eng" };
                if probe.streams.len() != 2
                    || probe.streams[0].codec_name.as_deref() != Some("av1")
                    || probe.streams[1].codec_name.as_deref() != Some("opus")
                    || probe.streams[1]
                        .tags
                        .get("language")
                        .and_then(|v| v.as_str())
                        != Some(expected_language)
                {
                    return Err("language remux does not match its declared tracks".into());
                }
            }
            if id.starts_with("demo-")
                && (probe.streams[0]
                    .details
                    .get("pix_fmt")
                    .and_then(|v| v.as_str())
                    != Some("yuv420p10le")
                    || probe.streams[1]
                        .details
                        .get("channels")
                        .and_then(|v| v.as_u64())
                        != Some(if id.ends_with("fr") { 6 } else { 2 }))
            {
                return Err("demo profile must be AV1 10-bit / FR Opus 5.1 or EN stereo".into());
            }
            let representative = if id.starts_with("fixture-representative-") {
                let english = id.ends_with("-en");
                let audio_tracks: Vec<_> = probe
                    .streams
                    .iter()
                    .filter(|t| t.codec_type.as_deref() == Some("audio"))
                    .collect();
                let video_tracks: Vec<_> = probe
                    .streams
                    .iter()
                    .filter(|t| t.codec_type.as_deref() == Some("video"))
                    .collect();
                let subtitle_tracks: Vec<_> = probe
                    .streams
                    .iter()
                    .filter(|t| t.codec_type.as_deref() == Some("subtitle"))
                    .collect();
                if video_tracks.len() != 1
                    || subtitle_tracks.len() != 3
                    || audio_tracks.len() != if english { 1 } else { 2 }
                    || video_tracks[0].codec_name.as_deref() != Some("av1")
                    || video_tracks[0]
                        .details
                        .get("pix_fmt")
                        .and_then(|v| v.as_str())
                        != Some("yuv420p10le")
                    || audio_tracks
                        .iter()
                        .any(|t| t.codec_name.as_deref() != Some("opus"))
                    || audio_tracks[0]
                        .details
                        .get("channels")
                        .and_then(|v| v.as_u64())
                        != Some(if english { 2 } else { 6 })
                    || audio_tracks[0]
                        .tags
                        .get("language")
                        .and_then(|v| v.as_str())
                        != Some(if english { "eng" } else { "fra" })
                {
                    return Err(
                        "representative fixture does not match declared AV1 10-bit/Opus/subtitle profile"
                            .into(),
                    );
                }
                if !english
                    && (audio_tracks[1]
                        .details
                        .get("channels")
                        .and_then(|v| v.as_u64())
                        != Some(2)
                        || audio_tracks[1]
                            .tags
                            .get("language")
                            .and_then(|v| v.as_str())
                            != Some("eng"))
                {
                    return Err(
                        "representative EN track does not match declared stereo profile".into(),
                    );
                }
                Some(RepresentativeCase {
                    expected_audio_language: if english { "en" } else { "fr" },
                    expected_audio_channels: if english { 2 } else { 6 },
                    method: if english {
                        "prepared-en-remux-native-url"
                    } else {
                        "multi-track-native-url"
                    },
                })
            } else {
                None
            };
            assets.push(Asset {
                relative,
                descriptor: Descriptor {
                    id,
                    mime,
                    probe,
                    playback_status: "unverified-on-client",
                    language_remux: if id.starts_with("demo-") {
                        None
                    } else {
                        language_remux
                    },
                    representative,
                },
            });
        }
        Ok(Self { root, assets })
    }

    pub fn descriptors(&self) -> Vec<Descriptor> {
        self.assets
            .iter()
            .filter(|a| !a.descriptor.id.starts_with("demo-"))
            .map(|a| a.descriptor.clone())
            .collect()
    }

    pub fn demo_descriptors(&self) -> Vec<Descriptor> {
        self.assets
            .iter()
            .filter(|a| a.descriptor.id.starts_with("demo-"))
            .map(|a| a.descriptor.clone())
            .collect()
    }

    /// Look up a fixture/demo asset by ID and open a confined `OpenedFile` handle.
    pub async fn open_fixture_asset(
        &self,
        id: &str,
    ) -> Result<Option<(OpenedFile, &'static str)>, io::Error> {
        let Some(asset) = self.assets.iter().find(|a| a.descriptor.id == id) else {
            return Ok(None);
        };
        let opened = OpenedFile::open_in_root(&self.root, &asset.relative).await?;
        Ok(Some((opened, asset.descriptor.mime)))
    }

    /// Open a relative path inside the Node root as a confined `OpenedFile`.
    pub async fn open_relative(&self, relative: &Path) -> io::Result<OpenedFile> {
        OpenedFile::open_in_root(&self.root, relative).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_inventory_confinement_and_fixture_bench_flag() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("media");
        let outside = temp.path().join("outside");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(root.join("sample.mp4"), b"sample-bytes")
            .await
            .unwrap();
        tokio::fs::write(outside.join("secret.mp4"), b"secret-bytes")
            .await
            .unwrap();

        let inv = Inventory::local(root.clone()).await.unwrap();
        assert!(!inv.is_fixture_bench());
        assert!(inv.descriptors().is_empty());
        assert!(inv.demo_descriptors().is_empty());
        assert!(
            inv.open_fixture_asset("fixture-mp4")
                .await
                .unwrap()
                .is_none()
        );

        let opened = inv.open_relative(Path::new("sample.mp4")).await.unwrap();
        assert_eq!(opened.size(), 12);
        assert!(
            inv.open_relative(Path::new("../outside/secret.mp4"))
                .await
                .is_err()
        );
    }
}

//! Recognising a film that was renamed or moved inside its library, so the existing work (manual
//! corrections, translations, prepared caches keyed by asset id) follows the file instead of a
//! fresh, empty work being created at the new path.
//!
//! Hard rules (each one is covered by a test):
//! - Same library only. Libraries carry their own permissions; nothing is ever matched, moved or
//!   merged across libraries.
//! - Only after a *complete* scan, so a path that is merely unreadable is never mistaken for a
//!   path that vanished.
//! - A partial content fingerprint is a candidate, not an identity. It must be unique on both
//!   sides (one vanished asset, one appeared file, no other staged file sharing it), the sizes
//!   must be equal, the external identifiers must not contradict, and one independent signal must
//!   agree (same modification time, same file name, or a shared reliable identifier).
//! - Without a usable fingerprint (the file was re-encoded, e.g. a Radarr upgrade), a *reliable*
//!   identifier (tmdb/imdb/tvdb, never contradicted, never also carried by a work whose files are
//!   still present) can pair the single vanished asset with the single appeared file.
//! - Ambiguity of any kind (duplicates, two candidates, conflicting ids) relocates nothing: the
//!   normal publication then creates a new work, which is always the safe outcome.
use crate::database::{query, query_as, query_scalar};
use crate::node::nfo::reconcile_ids;
use media_contracts::media::FilmMetadata;
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Providers whose identifiers are comparable across files; anything else is opaque.
const RELIABLE: [&str; 3] = ["tmdb", "imdb", "tvdb"];

/// An asset known to the Core whose path is no longer in the scan.
#[derive(Clone, Debug)]
pub(super) struct Vanished {
    pub asset_id: String,
    pub work_id: String,
    pub relative_path: String,
    pub size: i64,
    pub modified_ns: String,
    pub fingerprint: Option<String>,
    pub ids: BTreeMap<String, String>,
    /// `(season, episode)` for series files. A move never changes the episode a file is.
    pub slot: Option<(i64, i64)>,
}

/// A staged file whose path has no asset yet.
#[derive(Clone, Debug)]
pub(super) struct Appeared {
    pub relative_path: String,
    pub size: i64,
    pub modified_ns: String,
    pub fingerprint: Option<String>,
    pub ids: BTreeMap<String, String>,
    pub slot: Option<(i64, i64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Basis {
    /// Same size and sampled fingerprint, corroborated by an independent signal.
    Content,
    /// A single reliable identifier shared by exactly one vanished asset and one appeared file.
    Identifier,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Relocation {
    pub asset_id: String,
    pub work_id: String,
    pub from_path: String,
    pub to_path: String,
    pub basis: Basis,
}

fn reliable(ids: &BTreeMap<String, String>) -> impl Iterator<Item = (&String, &String)> {
    ids.iter().filter(|(p, _)| RELIABLE.contains(&p.as_str()))
}
fn contradicts(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> bool {
    reliable(a).any(|(p, v)| b.get(p).is_some_and(|other| other != v))
}
fn shares(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> bool {
    reliable(a).any(|(p, v)| b.get(p) == Some(v))
}
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Pure decision: which vanished assets the appeared files are moves of.
///
/// `duplicated` holds every fingerprint carried by two or more staged files (moved or not), and
/// `present_ids` the reliable identifiers of works that still have a file in the scan.
pub(super) fn plan(
    vanished: &[Vanished],
    appeared: &[Appeared],
    duplicated: &HashSet<String>,
    present_ids: &HashSet<(String, String)>,
) -> Vec<Relocation> {
    let mut used_vanished = vec![false; vanished.len()];
    let mut used_appeared = vec![false; appeared.len()];
    let mut plan = Vec::new();

    // Pass 1: content.
    for (ai, a) in appeared.iter().enumerate() {
        let Some(fingerprint) = a.fingerprint.as_deref() else {
            continue;
        };
        if duplicated.contains(fingerprint) {
            continue;
        }
        let candidates: Vec<usize> = vanished
            .iter()
            .enumerate()
            .filter(|(i, v)| {
                !used_vanished[*i]
                    && v.slot == a.slot
                    && v.size == a.size
                    && v.fingerprint.as_deref() == Some(fingerprint)
            })
            .map(|(i, _)| i)
            .collect();
        let [vi] = candidates[..] else {
            continue;
        };
        let v = &vanished[vi];
        let appeared_twins = appeared
            .iter()
            .filter(|other| {
                other.size == a.size && other.fingerprint.as_deref() == Some(fingerprint)
            })
            .count();
        let corroborated = v.modified_ns == a.modified_ns
            || file_name(&v.relative_path).eq_ignore_ascii_case(file_name(&a.relative_path))
            || shares(&v.ids, &a.ids);
        if appeared_twins == 1 && corroborated && !contradicts(&v.ids, &a.ids) {
            used_vanished[vi] = true;
            used_appeared[ai] = true;
            plan.push(Relocation {
                asset_id: v.asset_id.clone(),
                work_id: v.work_id.clone(),
                from_path: v.relative_path.clone(),
                to_path: a.relative_path.clone(),
                basis: Basis::Content,
            });
        }
    }

    // Pass 2: reliable identifiers, for what the content pass could not place (a re-encode keeps
    // its NFO identifiers but not its bytes).
    for (ai, a) in appeared.iter().enumerate() {
        if used_appeared[ai] || reliable(&a.ids).next().is_none() {
            continue;
        }
        let candidates: Vec<usize> = vanished
            .iter()
            .enumerate()
            .filter(|(i, v)| {
                !used_vanished[*i]
                    && v.slot == a.slot
                    && shares(&v.ids, &a.ids)
                    && !contradicts(&v.ids, &a.ids)
            })
            .map(|(i, _)| i)
            .collect();
        let [vi] = candidates[..] else {
            continue;
        };
        let v = &vanished[vi];
        let rivals = appeared
            .iter()
            .enumerate()
            .filter(|(i, other)| {
                !used_appeared[*i]
                    && v.slot == other.slot
                    && shares(&v.ids, &other.ids)
                    && !contradicts(&v.ids, &other.ids)
            })
            .count();
        let carried_elsewhere =
            reliable(&a.ids).any(|(p, value)| present_ids.contains(&(p.clone(), value.clone())));
        if rivals == 1 && !carried_elsewhere {
            used_vanished[vi] = true;
            used_appeared[ai] = true;
            plan.push(Relocation {
                asset_id: v.asset_id.clone(),
                work_id: v.work_id.clone(),
                from_path: v.relative_path.clone(),
                to_path: a.relative_path.clone(),
                basis: Basis::Identifier,
            });
        }
    }
    plan
}

fn ids_of(json: &str) -> BTreeMap<String, String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// What the publication needs to know about a staged file to carry a relocation through.
struct AppearedInfo {
    identity_key: String,
    series_key: Option<String>,
    slot: Option<(i64, i64)>,
}

/// What the Core already holds about a vanished asset's place in the catalogue.
struct VanishedInfo {
    work_key: String,
    series_work_id: Option<String>,
    series_work_key: Option<String>,
}

/// Detect and apply relocations for a complete scan of a films or series library, inside the
/// publication transaction. Returns the applied relocations and the works whose editorial data
/// must be refreshed afterwards.
///
/// Films: the work's key follows the file. Series: the episode keeps its (season, episode); its
/// work key and, when no other file of the scan still uses the old series key, the series work
/// key follow the new location, so seasons, episode links, corrections and caches stay put.
pub(super) async fn apply(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    scan_id: &str,
    library_id: &str,
    root_snapshot: &str,
    series: bool,
) -> Result<(Vec<Relocation>, BTreeSet<String>), sqlx::Error> {
    let nothing = || Ok((Vec::new(), BTreeSet::new()));
    let kind_filter = if series {
        "f.series_identity_key IS NOT NULL"
    } else {
        "f.series_identity_key IS NULL"
    };
    // Files of this scan that no asset occupies yet.
    let appeared_rows = query(sqlx::AssertSqlSafe(format!(
        "SELECT f.relative_path,f.size,f.modified_ns,f.external_ids_json,f.editorial_json,f.identity_key,\
                f.series_identity_key,f.season_number,f.episode_number,fp.fingerprint \
         FROM scan_staging_files f \
         LEFT JOIN scan_staging_fingerprints fp ON fp.scan_id=f.scan_id AND fp.relative_path=f.relative_path \
         WHERE f.scan_id=? AND {kind_filter} AND NOT EXISTS \
               (SELECT 1 FROM inventory_assets a WHERE a.library_id=? AND a.relative_path=f.relative_path) \
         ORDER BY f.relative_path LIMIT 20000"
    )))
    .bind(scan_id)
    .bind(library_id)
    .fetch_all(&mut **tx)
    .await?;
    if appeared_rows.is_empty() {
        return nothing();
    }
    let mut info: HashMap<String, AppearedInfo> = HashMap::new();
    let mut appeared = Vec::with_capacity(appeared_rows.len());
    for row in &appeared_rows {
        let path: String = row.get("relative_path");
        let name_ids = ids_of(&row.get::<String, _>("external_ids_json"));
        let ids = match row
            .get::<Option<String>, _>("editorial_json")
            .and_then(|json| serde_json::from_str::<FilmMetadata>(&json).ok())
        {
            // Name and NFO identifiers agree or are one-sided; contradictions are dropped.
            Some(nfo) => reconcile_ids(&name_ids, &nfo).0,
            None => name_ids,
        };
        let slot = match (
            row.get::<Option<i64>, _>("season_number"),
            row.get::<Option<i64>, _>("episode_number"),
        ) {
            (Some(season), Some(episode)) if series => Some((season, episode)),
            _ => None,
        };
        if series && slot.is_none() {
            // A series file without a parsed season/episode has no slot to keep: leave it alone.
            continue;
        }
        info.insert(
            path.clone(),
            AppearedInfo {
                identity_key: row.get("identity_key"),
                series_key: row.get::<Option<String>, _>("series_identity_key"),
                slot,
            },
        );
        appeared.push(Appeared {
            relative_path: path,
            size: row.get("size"),
            modified_ns: row.get("modified_ns"),
            fingerprint: row.get::<Option<String>, _>("fingerprint"),
            ids,
            slot,
        });
    }

    // Assets of this library missing from the scan, under the current root.
    let (work_kind, extra_select, extra_join) = if series {
        (
            "episode",
            ",ss.season_number AS season,ie.episode_number AS episode,sw.id AS series_work_id,sw.identity_key AS series_work_key",
            "JOIN inventory_episodes ie ON ie.work_id=w.id \
             JOIN inventory_seasons ss ON ss.id=ie.season_id \
             JOIN inventory_works sw ON sw.id=ss.series_id ",
        )
    } else {
        ("film", "", "")
    };
    let vanished_rows = query(sqlx::AssertSqlSafe(format!(
        "SELECT a.id,a.relative_path,a.size,a.modified_ns,w.id AS work_id,w.identity_key AS work_key,\
                w.external_ids_json,f.fingerprint{extra_select} \
         FROM inventory_assets a \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN inventory_versions v ON v.id=s.version_id \
         JOIN inventory_editions e ON e.id=v.edition_id \
         JOIN inventory_works w ON w.id=e.work_id \
         {extra_join}\
         LEFT JOIN inventory_asset_fingerprints f ON f.asset_id=a.id AND f.size=a.size AND f.modified_ns=a.modified_ns \
         WHERE a.library_id=? AND s.root_snapshot=? AND w.kind='{work_kind}' AND NOT EXISTS \
               (SELECT 1 FROM scan_staging_files t WHERE t.scan_id=? AND t.relative_path=a.relative_path) \
         LIMIT 100000"
    )))
    .bind(library_id)
    .bind(root_snapshot)
    .bind(scan_id)
    .fetch_all(&mut **tx)
    .await?;
    if vanished_rows.is_empty() {
        return nothing();
    }
    let mut vanished_info: HashMap<String, VanishedInfo> = HashMap::new();
    let vanished: Vec<Vanished> = vanished_rows
        .iter()
        .map(|row| {
            let asset_id: String = row.get("id");
            vanished_info.insert(
                asset_id.clone(),
                VanishedInfo {
                    work_key: row.get("work_key"),
                    series_work_id: series.then(|| row.get("series_work_id")),
                    series_work_key: series.then(|| row.get("series_work_key")),
                },
            );
            Vanished {
                asset_id,
                work_id: row.get("work_id"),
                relative_path: row.get("relative_path"),
                size: row.get("size"),
                modified_ns: row.get("modified_ns"),
                fingerprint: row.get::<Option<String>, _>("fingerprint"),
                ids: ids_of(&row.get::<String, _>("external_ids_json")),
                slot: series.then(|| (row.get("season"), row.get("episode"))),
            }
        })
        .collect();

    let duplicated: HashSet<String> = query_scalar::<_, String>(
        "SELECT fingerprint FROM scan_staging_fingerprints WHERE scan_id=? GROUP BY fingerprint HAVING COUNT(*)>1",
    )
    .bind(scan_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .collect();
    // Works that still have a file in this scan: their identifiers are not "free" to be claimed.
    let present_rows: Vec<(String,)> = query_as(
        "SELECT DISTINCT w.external_ids_json FROM inventory_works w \
         JOIN inventory_editions e ON e.work_id=w.id \
         JOIN inventory_versions v ON v.edition_id=e.id \
         JOIN inventory_sources s ON s.version_id=v.id \
         JOIN inventory_assets a ON a.source_id=s.id \
         JOIN scan_staging_files t ON t.scan_id=? AND t.relative_path=a.relative_path \
         WHERE a.library_id=?",
    )
    .bind(scan_id)
    .bind(library_id)
    .fetch_all(&mut **tx)
    .await?;
    let present_ids: HashSet<(String, String)> = present_rows
        .iter()
        .flat_map(|(json,)| ids_of(json))
        .filter(|(p, _)| RELIABLE.contains(&p.as_str()))
        .collect();
    // Series keys still used by a file of this scan (such a series cannot be re-keyed).
    let staged_series_keys: HashSet<String> = if series {
        query_scalar::<_, String>(
            "SELECT DISTINCT series_identity_key FROM scan_staging_files WHERE scan_id=? AND series_identity_key IS NOT NULL",
        )
        .bind(scan_id)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|key| format!("{root_snapshot}\n{key}"))
        .collect()
    } else {
        HashSet::new()
    };

    let planned = plan(&vanished, &appeared, &duplicated, &present_ids);
    let mut applied = Vec::new();
    let mut works = BTreeSet::new();
    // series work id -> the scoped key it was moved to in this publication.
    let mut moved_series: HashMap<String, String> = HashMap::new();
    for relocation in planned {
        let (Some(new), Some(old)) = (
            info.get(&relocation.to_path),
            vanished_info.get(&relocation.asset_id),
        ) else {
            continue;
        };
        // Decide every key first: a relocation that cannot be carried through completely (a key
        // already owned by another work, a series still in use under its old key) is skipped, and
        // the publication then treats the file as new, which is always safe.
        let new_work_key = match (&new.series_key, new.slot) {
            (Some(series_key), Some((season, episode))) => {
                format!("{root_snapshot}\n{series_key}/s{season:02}e{episode:02}")
            }
            _ => format!("{root_snapshot}\n{}", new.identity_key),
        };
        if old.work_key != new_work_key
            && query_scalar::<_, String>(
                "SELECT id FROM inventory_works WHERE library_id=? AND identity_key=? AND id<>?",
            )
            .bind(library_id)
            .bind(&new_work_key)
            .bind(&relocation.work_id)
            .fetch_optional(&mut **tx)
            .await?
            .is_some()
        {
            continue;
        }
        let mut series_rekey: Option<(String, String)> = None;
        if let (Some(series_key), Some(series_work_id), Some(old_series_key)) =
            (&new.series_key, &old.series_work_id, &old.series_work_key)
        {
            let new_series_key = format!("{root_snapshot}\n{series_key}");
            if let Some(already) = moved_series.get(series_work_id) {
                if *already != new_series_key {
                    continue; // The old series split into two new locations: ambiguous.
                }
            } else if *old_series_key != new_series_key {
                let in_use = staged_series_keys.contains(old_series_key);
                let taken = query_scalar::<_, String>(
                    "SELECT id FROM inventory_works WHERE library_id=? AND identity_key=? AND id<>?",
                )
                .bind(library_id)
                .bind(&new_series_key)
                .bind(series_work_id)
                .fetch_optional(&mut **tx)
                .await?
                .is_some();
                if in_use || taken {
                    continue;
                }
                series_rekey = Some((series_work_id.clone(), new_series_key));
            }
        }

        // The asset keeps its id, so its source/version/edition/work chain, its tracks and every
        // cache keyed by asset id stay attached; only its location changes.
        let moved = query(
            "UPDATE inventory_assets SET relative_path=? WHERE id=? AND library_id=? AND relative_path=?",
        )
        .bind(&relocation.to_path)
        .bind(&relocation.asset_id)
        .bind(library_id)
        .bind(&relocation.from_path)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        if moved != 1 {
            continue;
        }
        if old.work_key != new_work_key {
            query("UPDATE inventory_works SET identity_key=? WHERE id=?")
                .bind(&new_work_key)
                .bind(&relocation.work_id)
                .execute(&mut **tx)
                .await?;
        }
        if let Some((series_work_id, new_series_key)) = series_rekey {
            query("UPDATE inventory_works SET identity_key=? WHERE id=?")
                .bind(&new_series_key)
                .bind(&series_work_id)
                .execute(&mut **tx)
                .await?;
            works.insert(series_work_id.clone());
            moved_series.insert(series_work_id, new_series_key);
        } else if let Some(series_work_id) = &old.series_work_id {
            works.insert(series_work_id.clone());
        }
        works.insert(relocation.work_id.clone());
        applied.push(relocation);
    }
    Ok((applied, works))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(p, v)| ((*p).to_owned(), (*v).to_owned()))
            .collect()
    }
    fn gone(asset: &str, path: &str, size: i64, mtime: &str, fp: Option<&str>) -> Vanished {
        Vanished {
            asset_id: asset.into(),
            work_id: format!("work-{asset}"),
            relative_path: path.into(),
            size,
            modified_ns: mtime.into(),
            fingerprint: fp.map(Into::into),
            ids: BTreeMap::new(),
            slot: None,
        }
    }
    fn new(path: &str, size: i64, mtime: &str, fp: Option<&str>) -> Appeared {
        Appeared {
            relative_path: path.into(),
            size,
            modified_ns: mtime.into(),
            fingerprint: fp.map(Into::into),
            ids: BTreeMap::new(),
            slot: None,
        }
    }
    fn run(v: &[Vanished], a: &[Appeared]) -> Vec<Relocation> {
        let mut seen = HashMap::<&str, usize>::new();
        for f in a.iter().filter_map(|f| f.fingerprint.as_deref()) {
            *seen.entry(f).or_default() += 1;
        }
        let duplicated = seen
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(f, _)| f.to_owned())
            .collect();
        plan(v, a, &duplicated, &HashSet::new())
    }

    #[test]
    fn renamed_or_moved_file_keeps_its_asset() {
        let moved = run(
            &[gone("a1", "Old (2020)/Film.mkv", 100, "7", Some("fp1"))],
            &[new("New (2020)/Film.mkv", 100, "7", Some("fp1"))],
        );
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].asset_id, "a1");
        assert_eq!(moved[0].to_path, "New (2020)/Film.mkv");
        assert_eq!(moved[0].basis, Basis::Content);
        // Renamed only: the name differs but the modification time survived the move.
        assert_eq!(
            run(
                &[gone("a1", "Film.mkv", 100, "7", Some("fp1"))],
                &[new("Film 1080p.mkv", 100, "7", Some("fp1"))]
            )
            .len(),
            1
        );
        // Copied elsewhere with a fresh mtime but the same file name still corroborates.
        assert_eq!(
            run(
                &[gone("a1", "A/Film.mkv", 100, "7", Some("fp1"))],
                &[new("B/film.MKV", 100, "9", Some("fp1"))]
            )
            .len(),
            1
        );
    }

    #[test]
    fn fingerprint_alone_is_never_enough() {
        // Same size and sampled bytes, but nothing else agrees: could be two distinct files.
        assert!(
            run(
                &[gone("a1", "A/One.mkv", 100, "7", Some("fp1"))],
                &[new("B/Two.mkv", 100, "9", Some("fp1"))]
            )
            .is_empty()
        );
        // Different size never matches, even with the same fingerprint string.
        assert!(
            run(
                &[gone("a1", "Film.mkv", 100, "7", Some("fp1"))],
                &[new("Film2.mkv", 101, "7", Some("fp1"))]
            )
            .is_empty()
        );
        // No fingerprint on either side: nothing to go on.
        assert!(
            run(
                &[gone("a1", "Film.mkv", 100, "7", None)],
                &[new("Film2.mkv", 100, "7", None)]
            )
            .is_empty()
        );
    }

    #[test]
    fn duplicates_and_ambiguity_relocate_nothing() {
        // Two new copies of the same bytes: which one is "the" move?
        assert!(
            run(
                &[gone("a1", "Film.mkv", 100, "7", Some("fp1"))],
                &[
                    new("Copy1/Film.mkv", 100, "7", Some("fp1")),
                    new("Copy2/Film.mkv", 100, "7", Some("fp1"))
                ]
            )
            .is_empty()
        );
        // Two vanished assets with the same bytes: the new file could be either.
        assert!(
            run(
                &[
                    gone("a1", "A/Film.mkv", 100, "7", Some("fp1")),
                    gone("a2", "B/Film.mkv", 100, "7", Some("fp1"))
                ],
                &[new("C/Film.mkv", 100, "7", Some("fp1"))]
            )
            .is_empty()
        );
        // A file still present at its old path with the same fingerprint makes it a duplicate.
        let v = [gone("a1", "A/Film.mkv", 100, "7", Some("fp1"))];
        let a = [new("B/Film.mkv", 100, "7", Some("fp1"))];
        let duplicated = HashSet::from(["fp1".to_owned()]);
        assert!(plan(&v, &a, &duplicated, &HashSet::new()).is_empty());
    }

    #[test]
    fn contradicting_identifiers_block_a_content_match() {
        let mut v = gone("a1", "A/Film.mkv", 100, "7", Some("fp1"));
        v.ids = ids(&[("tmdb", "1")]);
        let mut a = new("B/Film.mkv", 100, "7", Some("fp1"));
        a.ids = ids(&[("tmdb", "2")]);
        assert!(run(&[v.clone()], &[a.clone()]).is_empty());
        a.ids = ids(&[("tmdb", "1"), ("imdb", "tt1")]);
        assert_eq!(run(&[v], &[a]).len(), 1);
    }

    #[test]
    fn reliable_identifier_follows_a_reencode_but_only_when_unambiguous() {
        let mut v = gone("a1", "Film (2020)/Film.720p.mkv", 100, "7", Some("old"));
        v.ids = ids(&[("tmdb", "603")]);
        let mut a = new("Film (2020)/Film.1080p.mkv", 900, "99", Some("new"));
        a.ids = ids(&[("tmdb", "603")]);
        let upgraded = run(&[v.clone()], &[a.clone()]);
        assert_eq!(upgraded.len(), 1);
        assert_eq!(upgraded[0].basis, Basis::Identifier);
        assert_eq!(upgraded[0].work_id, "work-a1");

        // Opaque providers and contradicting ids are not reliable.
        let mut opaque_v = v.clone();
        opaque_v.ids = ids(&[("custom", "1")]);
        let mut opaque_a = a.clone();
        opaque_a.ids = ids(&[("custom", "1")]);
        assert!(run(&[opaque_v], &[opaque_a]).is_empty());
        let mut clash = a.clone();
        clash.ids = ids(&[("tmdb", "603"), ("imdb", "tt9")]);
        let mut vv = v.clone();
        vv.ids = ids(&[("tmdb", "603"), ("imdb", "tt1")]);
        assert!(run(&[vv], &[clash]).is_empty());

        // Two new files claiming the same film, or two vanished works with it: ambiguous.
        let mut second = a.clone();
        second.relative_path = "Film (2020)/Film.2160p.mkv".into();
        assert!(run(&[v.clone()], &[a.clone(), second]).is_empty());
        let mut other_v = v.clone();
        other_v.asset_id = "a2".into();
        other_v.work_id = "work-a2".into();
        assert!(run(&[v.clone(), other_v], &[a.clone()]).is_empty());

        // The id is also carried by a work whose file is still there: this is a second copy.
        let present = HashSet::from([("tmdb".to_owned(), "603".to_owned())]);
        assert!(plan(&[v], &[a], &HashSet::new(), &present).is_empty());
    }

    #[test]
    fn each_vanished_asset_is_used_once() {
        let mut v = gone("a1", "Old/Film.mkv", 100, "7", Some("fp1"));
        v.ids = ids(&[("tmdb", "5")]);
        let mut a1 = new("New/Film.mkv", 100, "7", Some("fp1"));
        a1.ids = ids(&[("tmdb", "5")]);
        let mut a2 = new("Other/Film (re-encode).mkv", 50, "8", Some("fp2"));
        a2.ids = ids(&[("tmdb", "5")]);
        let plan = run(&[v], &[a1, a2]);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].to_path, "New/Film.mkv");
    }

    #[test]
    fn an_episode_never_moves_to_another_season_or_episode_number() {
        let mut v = gone("a1", "Show/S01E01.mkv", 100, "7", Some("fp1"));
        v.slot = Some((1, 1));
        let mut same = new("Renamed/S01E01.mkv", 100, "7", Some("fp1"));
        same.slot = Some((1, 1));
        assert_eq!(run(&[v.clone()], &[same]).len(), 1);
        // Same bytes, but the user renumbered it: a different episode, so nothing is relocated.
        let mut renumbered = new("Show/S01E02.mkv", 100, "7", Some("fp1"));
        renumbered.slot = Some((1, 2));
        assert!(run(&[v.clone()], &[renumbered]).is_empty());
        // A film (no slot) and an episode never pair up either.
        assert!(run(&[v], &[new("Film.mkv", 100, "7", Some("fp1"))]).is_empty());
    }
}

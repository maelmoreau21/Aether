//! Per-library media sub-tasks of the scheduler: previews (trickplay and chapter images),
//! intros and credits, and images.
//!
//! Each sub-task already runs on its own after a scan (previews when the files are published,
//! intros when their chapters and audio are read, images once a work is identified). The
//! scheduler shows what each one is doing under its library and lets an administrator start or
//! stop it by hand. Every sub-task obeys its own library switch, independently of the others.
//!
//! The work itself stays where it always ran: `durable_tasks` (previews, fingerprints, picture
//! measurements and marker extensions) and `artwork_collection_jobs` (images).
//! `library_media_task_runs` only dates the run the scheduler reports.
use crate::{
    auth::AuthState,
    database::{begin_write, commit, query, query_as, query_scalar},
    identity::IdentityStore,
    library_options::LibraryOptions,
    task_scheduler::Admission,
    task_triggers::{LastRun, TaskStats, TaskView},
};

/// One media sub-task of a library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaTask {
    /// Online identification and metadata of the scanned media (metadata providers).
    Identify,
    /// The images of the identified works (one per kind, or the multi-image pool).
    Artwork,
    /// Trickplay sheets and chapter images.
    Trickplay,
    /// Intros and end credits (chapters, audio fingerprints, picture of a film's end).
    Intros,
}

impl MediaTask {
    /// In pipeline order: each phase waits for the previous ones of its library
    /// (`task_engine::pipeline`).
    pub const ALL: [Self; 4] = [Self::Identify, Self::Artwork, Self::Trickplay, Self::Intros];

    /// Prefix of the public task identifier and value of `library_media_task_runs.task`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Identify => "identify",
            Self::Trickplay => "trickplay",
            Self::Intros => "intros",
            Self::Artwork => "artwork",
        }
    }

    /// `trickplay-<library>`, `intros-<library>`, `artwork-<library>` → sub-task.
    pub fn parse_id(id: &str) -> Option<(Self, String)> {
        Self::ALL.into_iter().find_map(|task| {
            id.strip_prefix(task.key())
                .and_then(|rest| rest.strip_prefix('-'))
                .filter(|library| !library.is_empty() && library.len() <= 128)
                .map(|library| (task, library.to_owned()))
        })
    }

    pub fn id(self, library: &str) -> String {
        format!("{}-{library}", self.key())
    }

    /// Durable tasks of this sub-task (`None` for images, which use their own job table).
    fn durable_filter(self) -> Option<&'static str> {
        match self {
            Self::Identify => Some(
                "category='analyze' AND id IN (SELECT task_id FROM automation_task_links WHERE operation NOT LIKE 'intro.%' AND operation<>'metadata.images')",
            ),
            Self::Trickplay => Some("category='trickplay'"),
            Self::Intros => Some(
                "category='analyze' AND (component IN ('fingerprint','credits') OR id IN (SELECT task_id FROM automation_task_links WHERE operation LIKE 'intro.%'))",
            ),
            Self::Artwork => None,
        }
    }

    fn name(self, library: &str) -> String {
        match self {
            Self::Identify => format!("Identifier les médias de {library}"),
            Self::Trickplay => format!("Générer les aperçus de {library}"),
            Self::Intros => format!("Détecter les intros et génériques de {library}"),
            Self::Artwork => format!("Récupérer les images de {library}"),
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Identify => {
                "Reconnaît chaque film, série et épisode auprès des fournisseurs de métadonnées (titre, résumé, identifiants). Les images, les aperçus puis les intros suivent une fois l'identification terminée."
            }
            Self::Trickplay => {
                "Prépare les vignettes de la barre de lecture (trickplay) et les images de chapitres de chaque vidéo. Les aperçus déjà présents sont conservés, ceux qui manquent sont recréés."
            }
            Self::Intros => {
                "Lit les chapitres, calcule les empreintes audio des épisodes et mesure la fin des films pour placer les intros et génériques."
            }
            Self::Artwork => {
                "Demande aux fournisseurs d'images les affiches, fonds, logos et vignettes des œuvres identifiées ; en mode multi-image, remplit la réserve de plusieurs images par type."
            }
        }
    }

    /// Whether the sub-task also runs on its own after each scan (previews follow the
    /// library's "during scans" switches; intros and images always follow a scan).
    fn after_scan(self, options: &LibraryOptions) -> bool {
        match self {
            Self::Trickplay => {
                (options.trickplay_enabled && options.trickplay_during_scan)
                    || (options.chapter_images_enabled && options.chapter_images_during_scan)
            }
            Self::Intros => options.intro_detection_enabled,
            Self::Identify | Self::Artwork => true,
        }
    }

    /// Why the library settings keep this sub-task from running, if they do.
    fn disabled_reason(
        self,
        options: &LibraryOptions,
        marker_extension: bool,
    ) -> Option<&'static str> {
        match self {
            Self::Trickplay if !options.trickplay_enabled && !options.chapter_images_enabled => {
                Some(
                    "Les aperçus et les images de chapitres sont désactivés dans les réglages de la bibliothèque.",
                )
            }
            Self::Intros if !options.intro_detection_enabled => {
                Some("La détection des intros est désactivée dans les réglages de la bibliothèque.")
            }
            Self::Intros if !marker_extension => Some(
                "Aucune extension de détection des intros (Intro Skipper) n'est installée et activée.",
            ),
            Self::Artwork
                if !["poster", "backdrop", "logo", "thumb", "banner", "disc"]
                    .iter()
                    .any(|kind| options.image_enabled(kind)) =>
            {
                Some("Aucun type d'image n'est activé dans les réglages de la bibliothèque.")
            }
            _ => None,
        }
    }
}

/// Whether an enabled extension may report timeline markers (the intro and credits detection).
pub(crate) async fn marker_extension_enabled(store: &IdentityStore) -> Result<bool, sqlx::Error> {
    let packages: Vec<String> = query_scalar("SELECT package_json FROM extensions WHERE enabled=1")
        .fetch_all(store.pool())
        .await?;
    Ok(packages.iter().any(|package| {
        serde_json::from_str::<media_extension_sdk::Package>(package).is_ok_and(|package| {
            package
                .manifest
                .permissions
                .iter()
                .any(|permission| permission == "playback.markers")
        })
    }))
}

/// Libraries of the scope `?` (the library itself and its folders), for `durable_tasks`.
const SCOPE: &str =
    "(library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=?))";

/// Whether a run of `task` is still waiting or running for `library`. A scan asks this
/// *before* it queues its own work, so warm-ups it queues or re-arms never count.
pub(crate) async fn in_flight(
    conn: &mut sqlx::AnyConnection,
    library: &str,
    task: MediaTask,
) -> Result<bool, sqlx::Error> {
    let Some(filter) = task.durable_filter() else {
        return Ok(false);
    };
    let active: i64 = query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM durable_tasks WHERE {SCOPE} AND {filter} AND status IN ('queued','running','paused')")))
        .bind(library).bind(library).fetch_one(&mut *conn).await?;
    Ok(active > 0)
}

/// Remember when a run of `task` started. With `keep_current` (a scan adding work to a run
/// already in flight) an existing start is kept, so that run's progress keeps its origin.
pub(crate) async fn record_run(
    conn: &mut sqlx::AnyConnection,
    library: &str,
    task: MediaTask,
    now: i64,
    origin: &str,
    keep_current: bool,
) -> Result<(), sqlx::Error> {
    if keep_current {
        query("INSERT INTO library_media_task_runs(library_id,task,started_at,origin) VALUES(?,?,?,?) ON CONFLICT(library_id,task) DO NOTHING")
            .bind(library).bind(task.key()).bind(now).bind(origin).execute(&mut *conn).await?;
    } else {
        query("INSERT INTO library_media_task_runs(library_id,task,started_at,origin) VALUES(?,?,?,?) ON CONFLICT(library_id,task) DO UPDATE SET started_at=excluded.started_at,origin=excluded.origin")
            .bind(library).bind(task.key()).bind(now).bind(origin).execute(&mut *conn).await?;
    }
    Ok(())
}

/// A library that may carry media sub-tasks, as the task list reads it.
pub(crate) struct LibraryRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub kind: &'a str,
    /// Why the library itself cannot run anything now (disabled, Node unavailable).
    pub reason: Option<&'a str>,
}

/// The three sub-task views of a film or series library, or the one asked for.
pub(crate) async fn views(
    store: &IdentityStore,
    library: &LibraryRow<'_>,
    only: Option<MediaTask>,
    marker_extension: bool,
) -> Result<Vec<TaskView>, sqlx::Error> {
    if !matches!(library.kind, "films" | "series") {
        return Ok(Vec::new());
    }
    let pool = store.pool();
    let options: Option<String> = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(library.id)
        .fetch_optional(pool)
        .await?;
    let options = LibraryOptions::from_json(options.as_deref().unwrap_or("{}"));
    let mut views = Vec::new();
    // Every phase is read even for one task: a waiting phase names the earlier one it follows.
    for task in MediaTask::ALL {
        let started: Option<i64> = query_scalar(
            "SELECT started_at FROM library_media_task_runs WHERE library_id=? AND task=?",
        )
        .bind(library.id)
        .bind(task.key())
        .fetch_optional(pool)
        .await?;
        let (state, progress, last_run, stats, issues) = match task.durable_filter() {
            Some(filter) => durable_state(store, library.id, filter, started).await?,
            None => artwork_state(store, library.id).await?,
        };
        let reason = library.reason.map(str::to_owned).or_else(|| {
            task.disabled_reason(&options, marker_extension)
                .map(str::to_owned)
        });
        views.push(TaskView {
            id: task.id(library.id),
            kind: task.key(),
            name: task.name(library.name),
            description: task.description().into(),
            category: "Bibliothèques".into(),
            state,
            progress,
            next_run_at: None,
            last_run,
            reason,
            library_id: Some(library.id.to_owned()),
            library_kind: Some(library.kind.to_owned()),
            warnings_count: stats.as_ref().map_or(0, |stats| stats.failed),
            issues,
            triggers: Vec::new(),
            parent_id: Some(format!("scan-{}", library.id)),
            stats,
            after_scan: Some(task.after_scan(&options)),
            waiting_for: None,
        });
    }
    mark_waiting(&mut views);
    Ok(views
        .into_iter()
        .zip(MediaTask::ALL)
        .filter(|(_, task)| only.is_none_or(|only| only == *task))
        .map(|(view, _)| view)
        .collect())
}

/// A queued phase that an earlier phase of the same library still holds back names that
/// phase (`waiting_for`), so the scheduler can say why it has not started.
fn mark_waiting(views: &mut [TaskView]) {
    let mut ahead: Option<&'static str> = None;
    for view in views.iter_mut() {
        if view.state == "queued" {
            view.waiting_for = ahead;
        }
        if ahead.is_none() && matches!(view.state, "queued" | "running") {
            ahead = Some(view.kind);
        }
    }
}

type ViewState = (
    &'static str,
    Option<i64>,
    Option<LastRun>,
    Option<TaskStats>,
    Vec<String>,
);

/// State of a sub-task run as durable tasks: what is waiting, running, done and failed since
/// the run started (since ever when no run was recorded).
async fn durable_state(
    store: &IdentityStore,
    library: &str,
    filter: &str,
    started: Option<i64>,
) -> Result<ViewState, sqlx::Error> {
    let pool = store.pool();
    let since = started.unwrap_or(0);
    let rows: Vec<(String, i64, Option<i64>)> = query_as(sqlx::AssertSqlSafe(format!("SELECT status,COUNT(*),MAX(COALESCE(finished_at,updated_at)) FROM durable_tasks WHERE {SCOPE} AND {filter} AND (status IN ('queued','running','paused') OR COALESCE(finished_at,updated_at)>=?) GROUP BY status")))
        .bind(library).bind(library).bind(since).fetch_all(pool).await?;
    let count = |statuses: &[&str]| -> i64 {
        rows.iter()
            .filter(|(status, ..)| statuses.contains(&status.as_str()))
            .map(|(_, count, _)| *count)
            .sum()
    };
    let running = count(&["running"]);
    let paused = count(&["paused"]);
    let pending = count(&["queued", "running", "paused"]);
    let done = count(&["completed"]);
    let failed = count(&["error"]);
    let cancelled = count(&["cancelled"]);
    let end = rows
        .iter()
        .filter(|(status, ..)| matches!(status.as_str(), "completed" | "error" | "cancelled"))
        .filter_map(|(.., at)| *at)
        .max();
    let state = if running > 0 {
        "running"
    } else if pending > paused {
        "queued"
    } else if paused > 0 {
        "paused"
    } else {
        "idle"
    };
    let total = done + failed + cancelled + pending;
    // Without a recorded start, the history of every past run would count as progress.
    let progress = (matches!(state, "running" | "paused") && started.is_some() && total > 0)
        .then(|| (done + failed + cancelled) * 100 / total);
    let issues: Vec<String> = if failed > 0 {
        query_scalar(sqlx::AssertSqlSafe(format!("SELECT DISTINCT error FROM durable_tasks WHERE {SCOPE} AND {filter} AND status='error' AND error<>'' AND COALESCE(finished_at,updated_at)>=? LIMIT 10")))
            .bind(library).bind(library).bind(since).fetch_all(pool).await?
    } else {
        Vec::new()
    };
    let last_run = (state == "idle" && done + failed + cancelled > 0).then(|| LastRun {
        start: started,
        end,
        outcome: if done == 0 && failed > 0 {
            "failed"
        } else if done == 0 && failed == 0 {
            "cancelled"
        } else {
            "succeeded"
        }
        .into(),
        message: issues.first().cloned().filter(|_| done == 0 && failed > 0),
        error_kind: None,
        error_scope: None,
    });
    let stats = (total > 0).then_some(TaskStats {
        done,
        failed,
        pending,
        images: None,
    });
    Ok((state, progress, last_run, stats, issues))
}

/// State of the image pass of a library (its active job, else its last finished one).
async fn artwork_state(store: &IdentityStore, library: &str) -> Result<ViewState, sqlx::Error> {
    type JobRow = (String, i64, i64, i64, String, i64, i64, String, String);
    let job: Option<JobRow> = query_as("SELECT status,processed,downloaded,errors,last_error,created_at,updated_at,cursor_work,parameters FROM artwork_collection_jobs WHERE library_id=? ORDER BY CASE WHEN status IN ('queued','running','paused') THEN 0 ELSE 1 END,created_at DESC,id DESC LIMIT 1")
        .bind(library).fetch_optional(store.pool()).await?;
    let Some((
        status,
        processed,
        downloaded,
        errors,
        last_error,
        created,
        updated,
        cursor,
        parameters,
    )) = job
    else {
        return Ok(("idle", None, None, None, Vec::new()));
    };
    // A pass goes back to `queued` between two works: once one work is done it is running.
    let state = match status.as_str() {
        "running" => "running",
        "queued" if processed > 0 => "running",
        "queued" => "queued",
        "paused" => "paused",
        _ => "idle",
    };
    // Works still ahead of the pass cursor (works are taken in id order); an automatic pass
    // only takes identified works.
    let pending = if state == "idle" {
        0
    } else {
        let automatic = serde_json::from_str::<serde_json::Value>(&parameters)
            .ok()
            .and_then(|value| value.get("auto").map(|auto| !auto.is_null()))
            .unwrap_or(false);
        query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM inventory_works w WHERE (w.library_id=? OR w.library_id IN \
             (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND w.id>? {}",
            if automatic {
                "AND w.kind IN ('film','series') AND w.external_ids_json NOT IN ('','{}')"
            } else {
                ""
            }
        )))
        .bind(library)
        .bind(library)
        .bind(&cursor)
        .fetch_one(store.pool())
        .await?
    };
    let progress = (state != "idle" && processed + pending > 0)
        .then(|| processed * 100 / (processed + pending));
    let stats = Some(TaskStats {
        done: processed,
        failed: errors,
        pending,
        images: Some(downloaded),
    });
    let issues = if last_error.is_empty() {
        Vec::new()
    } else {
        vec![last_error]
    };
    let last_run = (state == "idle").then(|| LastRun {
        start: Some(created),
        end: Some(updated),
        outcome: if status == "cancelled" {
            "cancelled"
        } else {
            "succeeded"
        }
        .into(),
        message: None,
        error_kind: None,
        error_scope: None,
    });
    Ok((state, progress, last_run, stats, issues))
}

/// Start a run of `task` for `library` on an administrator's request.
pub(crate) async fn admit(
    auth: &AuthState,
    task: MediaTask,
    library: &str,
    actor: &str,
    now: i64,
) -> Result<Admission, String> {
    let store = &auth.store;
    let options: Option<String> = query_scalar("SELECT l.options_json FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND l.kind IN ('films','series') AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id)")
        .bind(library).fetch_optional(store.pool()).await.map_err(|e| e.to_string())?;
    let Some(options) = options else {
        return Ok(Admission::Unavailable);
    };
    let options = LibraryOptions::from_json(&options);
    let marker_extension = task != MediaTask::Intros
        || marker_extension_enabled(store)
            .await
            .map_err(|e| e.to_string())?;
    if task.disabled_reason(&options, marker_extension).is_some() {
        return Ok(Admission::Unavailable);
    }
    if let Some(filter) = task.durable_filter() {
        let active: i64 = query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM durable_tasks WHERE {SCOPE} AND {filter} AND status IN ('queued','running','paused')")))
            .bind(library).bind(library).fetch_one(store.pool()).await.map_err(|e| e.to_string())?;
        if active > 0 {
            return Ok(Admission::AlreadyActive);
        }
    }
    match task {
        MediaTask::Identify => {
            // A bounded sweep: the automation drain admits one provider task per asset that
            // still misses its metadata, in the identification band of the queue.
            let _writer = store.task_writes.lock().await;
            let mut conn = store.pool().acquire().await.map_err(|e| e.to_string())?;
            let mut tx = begin_write(&mut conn).await.map_err(|e| e.to_string())?;
            crate::automation::schedule_library(
                &mut tx,
                library,
                "metadata.missing",
                &format!("manual:{now}:{}", uuid::Uuid::new_v4()),
                None,
                now,
            )
            .await
            .map_err(|e| e.to_string())?;
            record_run(&mut tx, library, task, now, "manual", false)
                .await
                .map_err(|e| e.to_string())?;
            commit(tx).await.map_err(|e| e.to_string())?;
            crate::task_engine::WAKE.notify_one();
        }
        MediaTask::Trickplay => {
            let _writer = store.task_writes.lock().await;
            let mut conn = store.pool().acquire().await.map_err(|e| e.to_string())?;
            let mut tx = begin_write(&mut conn).await.map_err(|e| e.to_string())?;
            crate::trickplay::queue_previews(
                &mut tx,
                library,
                crate::trickplay::PreviewScope::Library,
                &options,
                true,
                true,
                now,
            )
            .await
            .map_err(|e| e.to_string())?;
            record_run(&mut tx, library, task, now, "manual", false)
                .await
                .map_err(|e| e.to_string())?;
            commit(tx).await.map_err(|e| e.to_string())?;
            crate::task_engine::WAKE.notify_one();
        }
        MediaTask::Intros => {
            crate::timeline_markers::queue_library_markers(auth, library)
                .await
                .map_err(|status| status.to_string())?;
            let mut conn = store.pool().acquire().await.map_err(|e| e.to_string())?;
            record_run(&mut conn, library, task, now, "manual", false)
                .await
                .map_err(|e| e.to_string())?;
        }
        MediaTask::Artwork => {
            let queued = crate::extensions::queue_artwork_pass(auth, library, actor)
                .await
                .map_err(|(status, body)| format!("{status}: {}", body.0))?;
            if !queued {
                return Ok(Admission::AlreadyActive);
            }
            let mut conn = store.pool().acquire().await.map_err(|e| e.to_string())?;
            record_run(&mut conn, library, task, now, "manual", false)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(Admission::Queued)
}

/// Pause, resume or stop (`cancel`) the active run of `task` for `library`. `false` when
/// nothing could change (nothing active, nothing paused to resume).
pub(crate) async fn control(
    auth: &AuthState,
    task: MediaTask,
    library: &str,
    action: &str,
    now: i64,
) -> Result<bool, String> {
    let Some(filter) = task.durable_filter() else {
        return crate::extensions::control_artwork_pass(auth, library, action)
            .await
            .map_err(|(status, body)| format!("{status}: {}", body.0));
    };
    let ids: Vec<String> = query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM durable_tasks WHERE {SCOPE} AND {filter} AND status IN {}",
        crate::task_engine::control_statuses(action)
    )))
    .bind(library)
    .bind(library)
    .fetch_all(auth.store.pool())
    .await
    .map_err(|e| e.to_string())?;
    let changed = crate::task_engine::control_many(&auth.store, &ids, action, now)
        .await
        .map_err(|e| e.to_string())?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_round_trip() {
        for task in MediaTask::ALL {
            assert_eq!(
                MediaTask::parse_id(&task.id("lib-1")),
                Some((task, "lib-1".into()))
            );
        }
        assert_eq!(
            MediaTask::parse_id("identify-films"),
            Some((MediaTask::Identify, "films".into()))
        );
        assert_eq!(MediaTask::parse_id("trickplay-"), None);
        assert_eq!(MediaTask::parse_id("clean-cache"), None);
        assert_eq!(MediaTask::parse_id("scan-lib"), None);
    }

    #[test]
    fn each_switch_disables_only_its_own_sub_task() {
        let mut options = LibraryOptions::default();
        assert!(
            MediaTask::ALL
                .iter()
                .all(|task| task.disabled_reason(&options, true).is_none())
        );
        options.trickplay_enabled = false;
        options.chapter_images_enabled = false;
        assert!(
            MediaTask::Trickplay
                .disabled_reason(&options, true)
                .is_some()
        );
        assert!(MediaTask::Intros.disabled_reason(&options, true).is_none());
        assert!(MediaTask::Artwork.disabled_reason(&options, true).is_none());
        let mut options = LibraryOptions {
            intro_detection_enabled: false,
            ..Default::default()
        };
        assert!(MediaTask::Intros.disabled_reason(&options, true).is_some());
        assert!(
            MediaTask::Trickplay
                .disabled_reason(&options, true)
                .is_none()
        );
        options.intro_detection_enabled = true;
        assert!(MediaTask::Intros.disabled_reason(&options, false).is_some());
    }
}

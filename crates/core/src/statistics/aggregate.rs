//! Pure, SQL-free aggregations behind the insights and the yearly Wrapped.
//!
//! Rows are light session snapshots already filtered by `reports::select`
//! (account, library permissions, parental policy). Every calendar bucket is
//! computed in the caller's time zone; a session is attributed to the local
//! day, hour and month of its start, like JellyTrack.
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

/// JellyTrack rule: shorter sessions are "zapping" and never count as plays.
pub(crate) const MIN_PLAY_SECONDS: f64 = 60.0;
/// SQL twin of [`Play::qualifies`], for aggregates computed in the database.
pub(crate) const QUALIFIED_SQL: &str =
    "(s.seconds>=60 OR s.completed=1 OR (s.ended_at IS NULL AND s.updated_at>=unixepoch()-90))";

#[derive(Clone, Debug, Default)]
pub(crate) struct Play {
    pub id: String,
    pub user_id: String,
    pub user: String,
    pub media_id: String,
    pub canonical_id: String,
    pub kind: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub device: String,
    pub started_at: i64,
    /// `ended_at`, else the last report (`updated_at`).
    pub ended_at: i64,
    /// Open and still reporting (less than 90 s without news).
    pub active: bool,
    pub seconds: f64,
    pub covered: f64,
    pub duration: f64,
    pub completed: bool,
    pub method: String,
    pub height: i64,
    pub genres: Vec<String>,
    pub series_id: Option<String>,
    pub series_title: Option<String>,
}

impl Play {
    /// A play counts when at least 60 s were consumed, when the media was
    /// completed (only matters for media shorter than about a minute), or
    /// while it is still being watched.
    pub(crate) fn qualifies(&self) -> bool {
        self.active || self.completed || self.seconds >= MIN_PLAY_SECONDS
    }
    /// Identity of the title: canonical work for video, track for music.
    fn title_key(&self) -> &str {
        if self.kind == "music" {
            &self.media_id
        } else {
            &self.canonical_id
        }
    }
    /// Episodes roll up to their show; other media stand alone.
    fn show_key(&self) -> String {
        match (&self.kind[..], &self.series_id) {
            ("episode", Some(series)) => format!("series:{series}"),
            _ => self.title_key().to_owned(),
        }
    }
    fn display_title(&self) -> String {
        match (&self.kind[..], &self.series_title) {
            ("episode", Some(series)) if !series.is_empty() => format!("{series} · {}", self.title),
            _ => self.title.clone(),
        }
    }
    fn video(&self) -> bool {
        self.kind == "film" || self.kind == "episode"
    }
}

/// Caller time zone: a fixed offset or an IANA zone (DST-aware).
#[derive(Clone, Debug)]
pub(crate) enum Zone {
    Fixed(i64),
    Named(String, jiff::tz::TimeZone),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Local {
    /// Local days since 1970-01-01.
    pub day: i64,
    pub year: i64,
    pub month: u32,
    pub dom: u32,
    pub hour: u32,
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u32,
}

impl Zone {
    /// `""`/`UTC`, minutes east of UTC (`-840..=840`, i.e. `-Date#getTimezoneOffset()`),
    /// or an IANA name such as `Europe/Paris`.
    pub(crate) fn parse(raw: &str) -> Option<Zone> {
        let raw = raw.trim();
        if raw.is_empty() || raw.eq_ignore_ascii_case("utc") {
            return Some(Zone::Fixed(0));
        }
        if let Ok(minutes) = raw.parse::<i64>() {
            return (-840..=840)
                .contains(&minutes)
                .then_some(Zone::Fixed(minutes * 60));
        }
        if raw.len() > 64
            || !raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/_+-".contains(c))
        {
            return None;
        }
        jiff::tz::TimeZone::get(raw)
            .ok()
            .map(|zone| Zone::Named(raw.to_owned(), zone))
    }
    pub(crate) fn offset(&self, epoch: i64) -> i64 {
        match self {
            Zone::Fixed(seconds) => *seconds,
            Zone::Named(_, zone) => jiff::Timestamp::from_second(epoch)
                .map(|t| i64::from(zone.to_offset(t).seconds()))
                .unwrap_or(0),
        }
    }
    pub(crate) fn local(&self, epoch: i64) -> Local {
        let t = epoch + self.offset(epoch);
        let day = t.div_euclid(86400);
        let (year, month, dom) = civil_from_days(day);
        Local {
            day,
            year,
            month,
            dom,
            hour: (t.rem_euclid(86400) / 3600) as u32,
            weekday: (day + 3).rem_euclid(7) as u32,
        }
    }
    /// UTC instant of the local midnight that starts `day`.
    pub(crate) fn midnight(&self, day: i64) -> i64 {
        let guess = day * 86400;
        guess - self.offset(guess - self.offset(guess))
    }
    pub(crate) fn year_start(&self, year: i64) -> i64 {
        self.midnight(days_from_civil(year, 1, 1))
    }
    pub(crate) fn label(&self) -> String {
        match self {
            Zone::Fixed(0) => "UTC".into(),
            Zone::Fixed(seconds) => {
                let sign = if *seconds < 0 { '-' } else { '+' };
                let minutes = seconds.abs() / 60;
                format!("UTC{sign}{:02}:{:02}", minutes / 60, minutes % 60)
            }
            Zone::Named(name, _) => name.clone(),
        }
    }
}

/// Howard Hinnant's `civil_from_days`: proleptic Gregorian (year, month, day).
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
pub(crate) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
pub(crate) fn date(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Readable client family from the stored user agent ("Chrome · Windows").
pub(crate) fn client_family(device: &str) -> String {
    let l = device.to_ascii_lowercase();
    let client = if l.contains("aether") {
        "Aether"
    } else if l.contains("edg/") || l.contains("edge/") {
        "Edge"
    } else if l.contains("opr/") || l.contains("opera") {
        "Opera"
    } else if l.contains("samsungbrowser") {
        "Samsung Internet"
    } else if l.contains("firefox") || l.contains("fxios") {
        "Firefox"
    } else if l.contains("chrome") || l.contains("crios") || l.contains("chromium") {
        "Chrome"
    } else if l.contains("safari") {
        "Safari"
    } else {
        ""
    };
    let system = if [
        "smart-tv",
        "smarttv",
        "tizen",
        "webos",
        "android tv",
        "crkey",
        "bravia",
    ]
    .iter()
    .any(|v| l.contains(v))
    {
        "TV"
    } else if l.contains("android") {
        "Android"
    } else if l.contains("iphone") || l.contains("ipad") || l.contains("ipod") {
        "iOS"
    } else if l.contains("windows") {
        "Windows"
    } else if l.contains("mac os") || l.contains("macintosh") {
        "macOS"
    } else if l.contains("cros") {
        "ChromeOS"
    } else if l.contains("linux") {
        "Linux"
    } else {
        ""
    };
    match (client, system) {
        ("", "") => {
            let trimmed: String = device.trim().chars().take(40).collect();
            if trimmed.is_empty() {
                "Appareil inconnu".into()
            } else {
                trimmed
            }
        }
        (c, "") => c.into(),
        ("", s) => s.into(),
        (c, s) => format!("{c} · {s}"),
    }
}

pub(crate) fn resolution(height: i64) -> &'static str {
    match height {
        h if h >= 2000 => "4K",
        h if h >= 1000 => "1080p",
        h if h >= 700 => "720p",
        h if h > 0 => "SD",
        _ => "unknown",
    }
}

fn transcoded(method: &str) -> Option<bool> {
    match method {
        "direct" => Some(false),
        "remux" | "separate_audio" | "prepared" => Some(true),
        _ => None,
    }
}

#[derive(Default)]
struct Rank {
    title: String,
    subtitle: String,
    image: String,
    kind: String,
    seconds: f64,
    plays: i64,
    users: HashSet<String>,
    items: HashSet<String>,
}
impl Rank {
    fn add(&mut self, play: &Play) {
        self.seconds += play.seconds;
        if play.qualifies() {
            self.plays += 1;
        }
        self.users.insert(play.user_id.clone());
    }
}
fn ranked(map: HashMap<String, Rank>, limit: usize) -> Vec<Value> {
    let mut items: Vec<(String, Rank)> = map.into_iter().collect();
    items.sort_by(|(ak, a), (bk, b)| {
        b.seconds
            .total_cmp(&a.seconds)
            .then(b.plays.cmp(&a.plays))
            .then(a.title.cmp(&b.title))
            .then(ak.cmp(bk))
    });
    items
        .into_iter()
        .take(limit)
        .map(|(id, r)| {
            json!({"id":id,"title":r.title,"subtitle":r.subtitle,"image":r.image,"kind":r.kind,
            "seconds":r.seconds,"plays":r.plays,"users":r.users.len(),"items":r.items.len()})
        })
        .collect()
}
fn poster(work: &str) -> String {
    format!("/api/v1/works/{work}/poster")
}
fn cover(track: &str) -> String {
    format!("/api/v1/music/tracks/{track}/cover")
}
fn growth(current: f64, previous: f64) -> Value {
    if previous <= 0.0 {
        Value::Null
    } else {
        json!(((current - previous) / previous * 1000.0).round() / 10.0)
    }
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Totals {
    pub seconds: f64,
    pub plays: i64,
    pub users: i64,
    pub media: i64,
    pub completed: i64,
}
impl Totals {
    pub(crate) fn of(rows: &[Play]) -> Totals {
        let watched = rows.iter().filter(|p| p.seconds > 0.0);
        Totals {
            seconds: rows.iter().map(|p| p.seconds).sum(),
            plays: rows.iter().filter(|p| p.qualifies()).count() as i64,
            users: watched
                .clone()
                .map(|p| &p.user_id)
                .collect::<HashSet<_>>()
                .len() as i64,
            media: watched.map(|p| p.title_key()).collect::<HashSet<_>>().len() as i64,
            completed: rows.iter().filter(|p| p.completed).count() as i64,
        }
    }
}

/// Weighted works (film, or the show of an episode) for people credits.
pub(crate) fn work_weights(rows: &[Play], limit: usize) -> Vec<(String, f64)> {
    let mut weights: HashMap<String, f64> = HashMap::new();
    for p in rows.iter().filter(|p| p.video() && p.seconds > 0.0) {
        let work = match (&p.kind[..], &p.series_id) {
            ("episode", Some(series)) => series.clone(),
            _ => p.media_id.clone(),
        };
        *weights.entry(work).or_default() += p.seconds;
    }
    let mut items: Vec<_> = weights.into_iter().collect();
    items.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    items.truncate(limit);
    items
}

/// Rank directors and leading actors from (credits, weight) pairs.
pub(crate) fn people(credits: &[(Value, f64)], limit: usize) -> Vec<Value> {
    let mut map: HashMap<(String, &'static str), (f64, i64)> = HashMap::new();
    for (metadata, seconds) in credits {
        let mut seen = HashSet::new();
        let directors = metadata["directors"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let actors = metadata["actors"].as_array().cloned().unwrap_or_default();
        for (role, list, take) in [("director", directors, 3), ("actor", actors, 6)] {
            for item in list.iter().take(take) {
                let name = item
                    .as_str()
                    .or_else(|| item["name"].as_str())
                    .unwrap_or("")
                    .trim();
                if name.is_empty() || name.len() > 160 || !seen.insert((name.to_owned(), role)) {
                    continue;
                }
                let entry = map.entry((name.to_owned(), role)).or_default();
                entry.0 += seconds;
                entry.1 += 1;
            }
        }
    }
    let mut items: Vec<_> = map.into_iter().collect();
    items.sort_by(|a, b| {
        b.1.1
            .cmp(&a.1.1)
            .then(b.1.0.total_cmp(&a.1.0))
            .then(a.0.cmp(&b.0))
    });
    items
        .into_iter()
        .take(limit)
        .map(|((name, role), (seconds, titles))| json!({"name":name,"role":role,"seconds":seconds,"titles":titles}))
        .collect()
}

/// Analytics for the selected period.
pub(crate) fn insights(rows: &[Play], zone: &Zone, now: i64) -> Value {
    let totals = Totals::of(rows);
    let mut weekdays = [(0i64, 0.0f64); 7];
    let mut hours = vec![(0i64, 0.0f64); 168];
    let mut monthly: BTreeMap<(i64, u32), (i64, f64, HashSet<&str>)> = BTreeMap::new();
    let mut calendar: BTreeMap<i64, (i64, f64)> = BTreeMap::new();
    let mut genres: HashMap<String, Rank> = HashMap::new();
    let mut series: HashMap<String, Rank> = HashMap::new();
    let mut titles: HashMap<String, Rank> = HashMap::new();
    let mut resolutions: BTreeMap<&str, (i64, f64)> = BTreeMap::new();
    let mut methods: BTreeMap<String, (i64, f64)> = BTreeMap::new();
    let mut clients: HashMap<String, (i64, f64, i64, i64, i64)> = HashMap::new();
    for p in rows {
        let local = zone.local(p.started_at);
        let counted = i64::from(p.qualifies());
        weekdays[local.weekday as usize].0 += counted;
        weekdays[local.weekday as usize].1 += p.seconds;
        let cell = (local.weekday * 24 + local.hour) as usize;
        hours[cell].0 += counted;
        hours[cell].1 += p.seconds;
        let month = monthly.entry((local.year, local.month)).or_default();
        month.0 += counted;
        month.1 += p.seconds;
        if p.seconds > 0.0 {
            month.2.insert(&p.user_id);
        }
        let day = calendar.entry(local.day).or_default();
        day.0 += counted;
        day.1 += p.seconds;
        let title = titles.entry(p.title_key().to_owned()).or_default();
        if title.title.is_empty() {
            title.title = p.display_title();
            title.kind = p.kind.clone();
            title.subtitle = p.artist.clone();
            title.image = if p.kind == "music" {
                cover(&p.media_id)
            } else {
                poster(&p.media_id)
            };
        }
        title.add(p);
        if p.video() {
            for genre in &p.genres {
                let rank = genres.entry(genre.to_lowercase()).or_default();
                if rank.title.is_empty() {
                    rank.title = genre.clone();
                }
                rank.add(p);
                rank.items.insert(p.show_key());
            }
            let r = resolutions.entry(resolution(p.height)).or_default();
            r.0 += counted;
            r.1 += p.seconds;
        }
        if let ("episode", Some(id)) = (&p.kind[..], &p.series_id) {
            let rank = series.entry(id.clone()).or_default();
            if rank.title.is_empty() {
                rank.title = p.series_title.clone().unwrap_or_else(|| p.title.clone());
                rank.kind = "series".into();
                rank.image = poster(id);
            }
            rank.add(p);
            rank.items.insert(p.media_id.clone());
        }
        let method = if p.method.is_empty() {
            "unknown".to_owned()
        } else {
            p.method.clone()
        };
        let m = methods.entry(method.clone()).or_default();
        m.0 += counted;
        m.1 += p.seconds;
        let c = clients.entry(client_family(&p.device)).or_default();
        c.0 += counted;
        c.1 += p.seconds;
        match transcoded(&method) {
            Some(false) => c.2 += 1,
            Some(true) => c.3 += 1,
            None => c.4 += 1,
        }
    }
    // Completion per (account, title), cumulative over sessions.
    let mut progress: HashMap<(&str, &str), (f64, f64, bool, &Play)> = HashMap::new();
    for p in rows.iter().filter(|p| p.video() && p.seconds > 0.0) {
        let entry = progress
            .entry((&p.user_id, p.title_key()))
            .or_insert((0.0, 0.0, false, p));
        entry.0 += p.covered;
        entry.1 = entry.1.max(p.duration);
        entry.2 |= p.completed;
    }
    let mut buckets: BTreeMap<&str, i64> =
        ["completed", "partial", "dropped", "abandoned", "unknown"]
            .into_iter()
            .map(|b| (b, 0))
            .collect();
    let mut dropoff: HashMap<&str, (String, String, i64, i64, i64, f64)> = HashMap::new();
    for ((_, key), (covered, length, done, sample)) in &progress {
        let ratio = if *done {
            1.0
        } else if *length > 0.0 {
            (covered / length).min(1.0)
        } else {
            -1.0
        };
        let bucket = match ratio {
            r if r < 0.0 => "unknown",
            r if r >= 0.9 => "completed",
            r if r >= 0.5 => "partial",
            r if r >= 0.25 => "dropped",
            _ => "abandoned",
        };
        *buckets.entry(bucket).or_default() += 1;
        if ratio >= 0.0 {
            let d = dropoff
                .entry(*key)
                .or_insert_with(|| (sample.display_title(), sample.kind.clone(), 0, 0, 0, 0.0));
            d.2 += 1;
            d.3 += i64::from(ratio < 0.25);
            d.4 += i64::from(ratio < 0.5);
            d.5 += ratio;
        }
    }
    let mut dropoff: Vec<_> = dropoff.into_iter().filter(|(_, d)| d.4 > 0).collect();
    dropoff.sort_by(|a, b| {
        b.1.4
            .cmp(&a.1.4)
            .then(b.1.3.cmp(&a.1.3))
            .then(b.1.2.cmp(&a.1.2))
            .then(a.0.cmp(b.0))
    });
    let dropoff: Vec<Value> = dropoff
        .into_iter()
        .take(15)
        .map(|(id, (title, kind, starters, below25, below50, sum))| {
            json!({"id":id,"title":title,"kind":kind,"starters":starters,"below25":below25,"below50":below50,"averageCompletion":sum/starters as f64})
        })
        .collect();
    let (peak, peak_at, daily_peaks) = concurrency(rows, zone, now);
    let mut clients: Vec<_> = clients.into_iter().collect();
    clients.sort_by(|a, b| b.1.1.total_cmp(&a.1.1).then(a.0.cmp(&b.0)));
    json!({
        "timezone": zone.label(),
        "minimumPlaySeconds": MIN_PLAY_SECONDS,
        "totals": {"seconds":totals.seconds,"plays":totals.plays,"opened":rows.len(),"zapped":rows.len() as i64-totals.plays,
            "users":totals.users,"media":totals.media,"completed":totals.completed},
        "weekdays": weekdays.iter().enumerate().map(|(i,(plays,seconds))| json!({"weekday":i,"plays":plays,"seconds":seconds})).collect::<Vec<_>>(),
        "hours": hours.iter().enumerate().map(|(i,(plays,seconds))| json!({"cell":i,"plays":plays,"seconds":seconds})).collect::<Vec<_>>(),
        "monthly": monthly.iter().map(|((y,m),(plays,seconds,users))| json!({"month":format!("{y:04}-{m:02}"),"plays":plays,"seconds":seconds,"users":users.len()})).collect::<Vec<_>>(),
        "calendar": calendar.iter().map(|(day,(plays,seconds))| json!({"date":date(*day),"plays":plays,"seconds":seconds})).collect::<Vec<_>>(),
        "concurrency": {"peak":peak,"peakAt":peak_at,"daily":daily_peaks},
        "genres": ranked(genres, 20),
        "series": ranked(series, 20),
        "titles": ranked(titles, 20),
        "completion": {"buckets":buckets,"dropoff":dropoff},
        "resolution": resolutions.iter().map(|(label,(plays,seconds))| json!({"label":label,"plays":plays,"seconds":seconds})).collect::<Vec<_>>(),
        "methods": methods.iter().map(|(label,(plays,seconds))| json!({"label":label,"plays":plays,"seconds":seconds})).collect::<Vec<_>>(),
        "clients": clients.iter().take(20).map(|(label,(plays,seconds,direct,transcoded,unknown))| {
            let known = direct + transcoded;
            json!({"label":label,"plays":plays,"seconds":seconds,"direct":direct,"transcoded":transcoded,"unknown":unknown,
                "transcodeShare": if known > 0 { *transcoded as f64 / known as f64 } else { 0.0 }})
        }).collect::<Vec<_>>(),
    })
}

/// Watched share of one (account, title) pair: 1 when completed, -1 when unknown.
fn watched_ratio(covered: f64, length: f64, done: bool) -> f64 {
    if done {
        1.0
    } else if length > 0.0 {
        (covered / length).clamp(0.0, 1.0)
    } else {
        -1.0
    }
}

/// Audience of one title (film, episode, track or a whole series).
///
/// `episodes` maps an episode work to its (season, episode) numbers; viewers
/// are only listed for administrators.
pub(crate) fn media(
    rows: &[Play],
    episodes: &HashMap<String, (i64, i64)>,
    with_viewers: bool,
) -> Value {
    #[derive(Default)]
    struct Pair {
        covered: f64,
        length: f64,
        done: bool,
        plays: i64,
    }
    #[derive(Default)]
    struct Viewer {
        name: String,
        seconds: f64,
        plays: i64,
        last: i64,
        ratios: Vec<f64>,
    }
    #[derive(Default)]
    struct Episode {
        title: String,
        seconds: f64,
        plays: i64,
        users: HashSet<String>,
        ratios: Vec<f64>,
    }
    let mut pairs: HashMap<(&str, &str), Pair> = HashMap::new();
    let mut viewers: HashMap<&str, Viewer> = HashMap::new();
    for p in rows {
        let viewer = viewers.entry(&p.user_id).or_default();
        if viewer.name.is_empty() {
            viewer.name = p.user.clone();
        }
        viewer.seconds += p.seconds;
        viewer.plays += i64::from(p.qualifies());
        viewer.last = viewer.last.max(p.started_at);
        if p.seconds <= 0.0 {
            continue;
        }
        let pair = pairs.entry((&p.user_id, p.title_key())).or_default();
        pair.covered += p.covered;
        pair.length = pair.length.max(p.duration);
        pair.done |= p.completed;
        pair.plays += i64::from(p.qualifies());
    }
    let mut by_episode: HashMap<&str, Episode> = HashMap::new();
    for p in rows.iter().filter(|p| p.kind == "episode") {
        let entry = by_episode.entry(&p.media_id).or_default();
        if entry.title.is_empty() {
            entry.title = p.title.clone();
        }
        entry.seconds += p.seconds;
        entry.plays += i64::from(p.qualifies());
        if p.seconds > 0.0 {
            entry.users.insert(p.user_id.clone());
        }
    }
    // Retention: share of (account, title) pairs that reached each decile.
    let mut reached = [0i64; 11];
    let (mut known, mut sum, mut finished, mut rewatched) = (0i64, 0.0f64, 0i64, 0i64);
    for ((user, title), pair) in &pairs {
        rewatched += i64::from(pair.plays > 1);
        let ratio = watched_ratio(pair.covered, pair.length, pair.done);
        if ratio < 0.0 {
            continue;
        }
        known += 1;
        sum += ratio;
        finished += i64::from(pair.done || ratio >= 0.9);
        for (decile, slot) in reached.iter_mut().enumerate() {
            // Small tolerance: a credits skip still "reaches" 100 %.
            if ratio + 1e-9 >= decile as f64 / 10.0 || (decile == 10 && ratio >= 0.95) {
                *slot += 1;
            }
        }
        if let Some(v) = viewers.get_mut(user) {
            v.ratios.push(ratio);
        }
        if let Some(e) = by_episode.get_mut(title) {
            e.ratios.push(ratio);
        }
    }
    let average = |values: &[f64]| {
        if values.is_empty() {
            Value::Null
        } else {
            json!(values.iter().sum::<f64>() / values.len() as f64)
        }
    };
    let totals = Totals::of(rows);
    let first = rows.iter().map(|p| p.started_at).min();
    let last = rows.iter().map(|p| p.started_at).max();
    let mut episode_rows: Vec<_> = by_episode.into_iter().collect();
    episode_rows.sort_by(|(a, _), (b, _)| {
        let (sa, ea) = episodes.get(*a).copied().unwrap_or((i64::MAX, i64::MAX));
        let (sb, eb) = episodes.get(*b).copied().unwrap_or((i64::MAX, i64::MAX));
        sa.cmp(&sb).then(ea.cmp(&eb)).then(a.cmp(b))
    });
    let episode_values: Vec<Value> = episode_rows
        .iter()
        .take(500)
        .map(|(id, e)| {
            let numbers = episodes.get(*id);
            json!({"id":id,"title":e.title,"season":numbers.map(|n| n.0),"episode":numbers.map(|n| n.1),
                "plays":e.plays,"seconds":e.seconds,"users":e.users.len(),"averageCompletion":average(&e.ratios)})
        })
        .collect();
    let mut viewer_rows: Vec<_> = viewers
        .into_iter()
        .filter(|(_, v)| v.seconds > 0.0)
        .collect();
    viewer_rows.sort_by(|a, b| b.1.seconds.total_cmp(&a.1.seconds).then(a.0.cmp(b.0)));
    let viewer_values: Vec<Value> = if with_viewers {
        viewer_rows
            .iter()
            .take(50)
            .map(|(id, v)| {
                json!({"id":id,"name":v.name,"plays":v.plays,"seconds":v.seconds,"lastPlay":v.last,"averageCompletion":average(&v.ratios)})
            })
            .collect()
    } else {
        Vec::new()
    };
    let plays = totals.plays.max(0);
    json!({
        "audience": {
            "viewers": totals.users,
            "sessions": rows.len(),
            "plays": plays,
            "seconds": totals.seconds,
            "averageSession": if plays > 0 { json!(rows.iter().filter(|p| p.qualifies()).map(|p| p.seconds).sum::<f64>() / plays as f64) } else { Value::Null },
            "averageCompletion": if known > 0 { json!(sum / known as f64) } else { Value::Null },
            "completionRate": if known > 0 { json!(finished as f64 / known as f64) } else { Value::Null },
            "rewatches": rewatched,
            "firstPlay": first,
            "lastPlay": last,
            "active": rows.iter().filter(|p| p.active).count(),
        },
        "retention": reached.iter().enumerate().map(|(decile, count)| json!({
            "percent": decile * 10,
            "viewers": count,
            "share": if known > 0 { *count as f64 / known as f64 } else { 0.0 },
        })).collect::<Vec<_>>(),
        "episodes": episode_values,
        "viewers": viewer_values,
    })
}

/// Maximum number of simultaneous sessions, overall and per local day.
fn concurrency(rows: &[Play], zone: &Zone, now: i64) -> (i64, Option<i64>, Vec<Value>) {
    let mut events: Vec<(i64, i64)> = Vec::with_capacity(rows.len() * 2);
    for p in rows.iter().filter(|p| p.seconds > 0.0) {
        let end = if p.active {
            now.max(p.ended_at)
        } else {
            p.ended_at
        };
        events.push((p.started_at, 1));
        events.push((end.max(p.started_at + 1), -1));
    }
    // Ends sort before starts at the same instant: back-to-back is not overlap.
    events.sort();
    let (mut current, mut peak, mut peak_at) = (0i64, 0i64, None);
    let mut daily: BTreeMap<i64, i64> = BTreeMap::new();
    for (at, delta) in events {
        let day = zone.local(at).day;
        let before = current;
        current += delta;
        let slot = daily.entry(day).or_insert(before);
        *slot = (*slot).max(current);
        if current > peak {
            peak = current;
            peak_at = Some(at);
        }
    }
    let daily = daily
        .into_iter()
        .filter(|(_, v)| *v > 0)
        .map(|(day, v)| json!({"date":date(day),"peak":v}))
        .collect();
    (peak, peak_at, daily)
}

/// Totals used by the Wrapped comparison: qualified plays only.
pub(crate) fn wrapped_totals(rows: &[Play]) -> (f64, i64, i64) {
    let plays: Vec<&Play> = rows.iter().filter(|p| p.qualifies()).collect();
    (
        plays.iter().map(|p| p.seconds).sum(),
        plays.len() as i64,
        plays
            .iter()
            .map(|p| p.show_key())
            .collect::<HashSet<_>>()
            .len() as i64,
    )
}

fn peak_index(values: &[(i64, f64)]) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.0 > 0)
        .max_by(|a, b| {
            a.1.0
                .cmp(&b.1.0)
                .then(a.1.1.total_cmp(&b.1.1))
                .then(b.0.cmp(&a.0))
        })
        .map(|(i, _)| i)
}

/// Yearly recap of one account. Sessions under 60 s are excluded entirely.
pub(crate) fn wrapped(
    rows: &[Play],
    zone: &Zone,
    year: i64,
    previous: Option<(f64, i64, i64)>,
) -> Value {
    let plays: Vec<&Play> = rows
        .iter()
        .filter(|p| p.qualifies() && zone.local(p.started_at).year == year)
        .collect();
    let mut months = vec![(0i64, 0.0f64); 12];
    let mut hours = vec![(0i64, 0.0f64); 24];
    let mut weekdays = vec![(0i64, 0.0f64); 7];
    let mut days: BTreeMap<i64, f64> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, (i64, f64)> = ["film", "episode", "music"]
        .into_iter()
        .map(|k| (k, (0, 0.0)))
        .collect();
    let mut films: HashMap<String, Rank> = HashMap::new();
    let mut shows: HashMap<String, Rank> = HashMap::new();
    let mut artists: HashMap<String, Rank> = HashMap::new();
    let mut albums: HashMap<String, Rank> = HashMap::new();
    let mut tracks: HashMap<String, Rank> = HashMap::new();
    let mut genres: HashMap<String, Rank> = HashMap::new();
    let mut devices: HashMap<String, Rank> = HashMap::new();
    let mut binges: HashMap<(String, i64), (String, HashSet<String>)> = HashMap::new();
    for p in &plays {
        let local = zone.local(p.started_at);
        for (slot, index) in [
            (&mut months, local.month as usize - 1),
            (&mut hours, local.hour as usize),
            (&mut weekdays, local.weekday as usize),
        ] {
            slot[index].0 += 1;
            slot[index].1 += p.seconds;
        }
        *days.entry(local.day).or_default() += p.seconds;
        if let Some(k) = kinds.get_mut(&p.kind[..]) {
            k.0 += 1;
            k.1 += p.seconds;
        }
        let device = devices.entry(client_family(&p.device)).or_default();
        device.title = client_family(&p.device);
        device.add(p);
        if p.video() {
            for genre in &p.genres {
                let rank = genres.entry(genre.to_lowercase()).or_default();
                if rank.title.is_empty() {
                    rank.title = genre.clone();
                }
                rank.add(p);
            }
        }
        match &p.kind[..] {
            "film" => {
                let r = films.entry(p.canonical_id.clone()).or_default();
                if r.title.is_empty() {
                    r.title = p.title.clone();
                    r.kind = "film".into();
                    r.image = poster(&p.media_id);
                }
                r.add(p);
            }
            "episode" => {
                let (key, title, image) = match &p.series_id {
                    Some(id) => (
                        id.clone(),
                        p.series_title.clone().unwrap_or_else(|| p.title.clone()),
                        poster(id),
                    ),
                    None => (p.canonical_id.clone(), p.title.clone(), poster(&p.media_id)),
                };
                let r = shows.entry(key.clone()).or_default();
                if r.title.is_empty() {
                    r.title = title.clone();
                    r.kind = "series".into();
                    r.image = image;
                }
                r.add(p);
                r.items.insert(p.media_id.clone());
                binges
                    .entry((key, local.day))
                    .or_insert_with(|| (title, HashSet::new()))
                    .1
                    .insert(p.media_id.clone());
            }
            "music" => {
                let artist = if p.artist.is_empty() {
                    "Artiste inconnu"
                } else {
                    &p.artist
                };
                let r = artists.entry(artist.to_lowercase()).or_default();
                if r.title.is_empty() {
                    r.title = artist.to_owned();
                    r.kind = "artist".into();
                    r.image = cover(&p.media_id);
                }
                r.add(p);
                r.items.insert(p.media_id.clone());
                if !p.album.is_empty() {
                    let r = albums
                        .entry(format!(
                            "{}\u{1f}{}",
                            p.artist.to_lowercase(),
                            p.album.to_lowercase()
                        ))
                        .or_default();
                    if r.title.is_empty() {
                        r.title = p.album.clone();
                        r.subtitle = p.artist.clone();
                        r.kind = "album".into();
                        r.image = cover(&p.media_id);
                    }
                    r.add(p);
                    r.items.insert(p.media_id.clone());
                }
                let r = tracks.entry(p.media_id.clone()).or_default();
                if r.title.is_empty() {
                    r.title = p.title.clone();
                    r.subtitle = p.artist.clone();
                    r.kind = "track".into();
                    r.image = cover(&p.media_id);
                }
                r.add(p);
            }
            _ => {}
        }
    }
    // Longest run of consecutive local days with at least one play.
    let (mut best, mut best_end, mut run, mut last) = (0i64, None::<i64>, 0i64, None::<i64>);
    for day in days.keys() {
        run = if last == Some(day - 1) { run + 1 } else { 1 };
        if run > best {
            best = run;
            best_end = Some(*day);
        }
        last = Some(*day);
    }
    let streak = best_end.map_or(
        json!({"days":0,"start":null,"end":null}),
        |end| json!({"days":best,"start":date(end-best+1),"end":date(end)}),
    );
    let binge = binges
        .into_iter()
        .filter(|(_, (_, episodes))| episodes.len() >= 2)
        .max_by(|a, b| {
            a.1 .1
                .len()
                .cmp(&b.1 .1.len())
                .then(b.0 .1.cmp(&a.0 .1))
                .then(b.0 .0.cmp(&a.0 .0))
        })
        .map(|((id, day), (title, episodes))| {
            json!({"id":id,"title":title,"date":date(day),"episodes":episodes.len(),"image":poster(&id)})
        });
    let seconds: f64 = plays.iter().map(|p| p.seconds).sum();
    let titles = plays
        .iter()
        .map(|p| p.show_key())
        .collect::<HashSet<_>>()
        .len() as i64;
    let moment = |p: &&Play| {
        json!({"title":p.display_title(),"kind":p.kind,"at":p.started_at,
        "image": if p.kind=="music" {cover(&p.media_id)} else {poster(p.series_id.as_deref().filter(|_|p.kind=="episode").unwrap_or(&p.media_id))}})
    };
    let first = plays
        .iter()
        .min_by_key(|p| (p.started_at, p.id.clone()))
        .map(moment);
    let last_play = plays
        .iter()
        .max_by_key(|p| (p.started_at, p.id.clone()))
        .map(moment);
    let devices_ranked = ranked(devices, 5);
    let comparison = previous.map(|(prev_seconds, prev_plays, prev_titles)| {
        json!({"seconds":prev_seconds,"plays":prev_plays,"titles":prev_titles,
            "secondsGrowth":growth(seconds,prev_seconds),"playsGrowth":growth(plays.len() as f64,prev_plays as f64),
            "titlesGrowth":growth(titles as f64,prev_titles as f64)})
    });
    json!({
        "year": year,
        "timezone": zone.label(),
        "totals": {"seconds":seconds,"hours":seconds/3600.0,"plays":plays.len(),"titles":titles,
            "days":seconds/86400.0,"activeDays":days.len()},
        "monthly": months.iter().enumerate().map(|(i,(p,s))| json!({"month":i+1,"plays":p,"seconds":s})).collect::<Vec<_>>(),
        "hours": hours.iter().map(|(p,_)| p).collect::<Vec<_>>(),
        "weekdays": weekdays.iter().map(|(p,_)| p).collect::<Vec<_>>(),
        "peaks": {"hour":peak_index(&hours),"weekday":peak_index(&weekdays),"month":peak_index(&months).map(|m|m+1)},
        "streak": streak,
        "topDevice": devices_ranked.first().map(|d| d["title"].clone()),
        "devices": devices_ranked,
        "genres": ranked(genres, 5),
        "films": ranked(films, 5),
        "series": ranked(shows, 5),
        "binge": binge,
        "artists": ranked(artists, 5),
        "albums": ranked(albums, 5),
        "tracks": ranked(tracks, 5),
        "kinds": kinds.iter().map(|(k,(p,s))| (k.to_string(), json!({"plays":p,"seconds":s}))).collect::<serde_json::Map<_,_>>(),
        "first": first,
        "last": last_play,
        "previous": comparison,
    })
}

/// Growth (in %) of each KPI against the previous window of equal length.
pub(crate) fn kpi_growth(current: Totals, previous: Totals) -> Value {
    json!({
        "previous": {"seconds":previous.seconds,"plays":previous.plays,"users":previous.users,"media":previous.media,"completed":previous.completed},
        "seconds": growth(current.seconds, previous.seconds),
        "plays": growth(current.plays as f64, previous.plays as f64),
        "users": growth(current.users as f64, previous.users as f64),
        "media": growth(current.media as f64, previous.media as f64),
        "completed": growth(current.completed as f64, previous.completed as f64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(id: &str, kind: &str, started: i64, seconds: f64) -> Play {
        Play {
            id: id.into(),
            user_id: "u".into(),
            user: "U".into(),
            media_id: format!("m-{id}"),
            canonical_id: format!("m-{id}"),
            kind: kind.into(),
            title: format!("Title {id}"),
            device: "Mozilla/5.0 (Windows NT 10.0) Chrome/130 Safari/537".into(),
            started_at: started,
            ended_at: started + seconds as i64,
            seconds,
            covered: seconds,
            duration: 3600.0,
            method: "direct".into(),
            ..Play::default()
        }
    }
    fn episode(id: &str, series: &str, started: i64) -> Play {
        Play {
            series_id: Some(series.into()),
            series_title: Some(format!("Show {series}")),
            ..play(id, "episode", started, 1500.0)
        }
    }
    // 2026-03-02T00:00:00Z (Monday)
    const MONDAY: i64 = 1_772_409_600;

    #[test]
    fn civil_round_trip_and_weekday() {
        for day in [-1000, 0, 59, 365, 11016, 20514, 40000] {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day);
        }
        assert_eq!(date(0), "1970-01-01");
        let local = Zone::Fixed(0).local(MONDAY);
        assert_eq!(
            (
                local.year,
                local.month,
                local.dom,
                local.weekday,
                local.hour
            ),
            (2026, 3, 2, 0, 0)
        );
    }

    #[test]
    fn time_zones_shift_day_hour_and_handle_dst() {
        // 23:30 UTC on Monday is Tuesday 00:30 in Paris (winter, UTC+1).
        let paris = Zone::parse("Europe/Paris").expect("bundled tzdb");
        let late = MONDAY + 23 * 3600 + 1800;
        let l = paris.local(late);
        assert_eq!((l.weekday, l.hour, l.dom), (1, 0, 3));
        assert_eq!(Zone::Fixed(0).local(late).weekday, 0);
        // Summer: UTC+2 (2026-07-01T12:00Z → 14 h).
        assert_eq!(paris.local(1_782_907_200).hour, 14);
        assert_eq!(Zone::parse("-300").unwrap().local(MONDAY).dom, 1);
        assert!(Zone::parse("900").is_none());
        assert!(Zone::parse("Mars/Olympus").is_none());
        assert!(Zone::parse("../etc").is_none());
        // Local midnight of 2026-01-01 in Paris is 23:00 UTC the day before.
        assert_eq!(
            paris.year_start(2026),
            days_from_civil(2026, 1, 1) * 86400 - 3600
        );
    }

    #[test]
    fn short_sessions_never_count_as_plays() {
        let rows = vec![
            play("a", "film", MONDAY, 59.0),
            play("b", "film", MONDAY + 10, 60.0),
            Play {
                completed: true,
                duration: 40.0,
                ..play("c", "music", MONDAY + 20, 38.0)
            },
            Play {
                active: true,
                ..play("d", "film", MONDAY + 30, 5.0)
            },
        ];
        let totals = Totals::of(&rows);
        assert_eq!(totals.plays, 3);
        let value = insights(&rows, &Zone::Fixed(0), MONDAY + 100);
        assert_eq!(value["totals"]["opened"], 4);
        assert_eq!(value["totals"]["zapped"], 1);
        let w = wrapped(&rows, &Zone::Fixed(0), 2026, None);
        assert_eq!(w["totals"]["plays"], 3);
        assert_eq!(w["totals"]["seconds"], 103.0);
    }

    #[test]
    fn wrapped_filters_year_rolls_up_series_and_finds_streak_and_binge() {
        let day = 86400;
        let rows = vec![
            // Last year in Paris (2025-12-31 23:30 local) is excluded.
            play(
                "old",
                "film",
                days_from_civil(2025, 12, 31) * day + 22 * 3600 + 1800,
                4000.0,
            ),
            // 2026-01-01 00:30 Paris = 2025-12-31 23:30 UTC: counted in 2026 locally.
            play(
                "new",
                "film",
                days_from_civil(2025, 12, 31) * day + 23 * 3600 + 1800,
                5000.0,
            ),
            episode("e1", "s1", MONDAY),
            episode("e2", "s1", MONDAY + 3600),
            episode("e3", "s1", MONDAY + 7200),
            episode("e1", "s1", MONDAY + day),
            episode("x1", "s2", MONDAY + 2 * day),
            episode("x2", "s2", MONDAY + 5 * day),
        ];
        let paris = Zone::parse("Europe/Paris").unwrap();
        let w = wrapped(&rows, &paris, 2026, Some((1500.0 * 2.0, 2, 1)));
        assert_eq!(w["totals"]["plays"], 7);
        assert_eq!(w["films"].as_array().unwrap().len(), 1);
        assert_eq!(w["films"][0]["title"], "Title new");
        let series = w["series"].as_array().unwrap();
        assert_eq!(series.len(), 2);
        assert_eq!(series[0]["id"], "s1");
        assert_eq!(series[0]["title"], "Show s1");
        assert_eq!(series[0]["items"], 3);
        assert_eq!(series[0]["plays"], 4);
        assert_eq!(series[0]["image"], "/api/v1/works/s1/poster");
        assert_eq!(w["binge"]["id"], "s1");
        assert_eq!(w["binge"]["episodes"], 3);
        assert_eq!(w["binge"]["date"], "2026-03-02");
        assert_eq!(w["streak"]["days"], 3);
        assert_eq!(w["streak"]["start"], "2026-03-02");
        assert_eq!(w["streak"]["end"], "2026-03-04");
        assert_eq!(w["totals"]["titles"], 3);
        assert_eq!(w["first"]["title"], "Title new");
        assert_eq!(w["peaks"]["month"], 3);
        assert_eq!(w["topDevice"], "Chrome · Windows");
        assert_eq!(w["previous"]["playsGrowth"], 250.0);
        // In UTC the borderline film belongs to 2025.
        assert_eq!(
            wrapped(&rows, &Zone::Fixed(0), 2026, None)["films"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn insights_buckets_completion_concurrency_and_resolution() {
        let rows = vec![
            Play {
                covered: 3500.0,
                completed: true,
                height: 2160,
                genres: vec!["Drame".into()],
                ..play("a", "film", MONDAY, 3500.0)
            },
            Play {
                covered: 600.0,
                height: 1080,
                user_id: "v".into(),
                method: "remux".into(),
                ..play("b", "film", MONDAY + 600, 600.0)
            },
            Play {
                covered: 2000.0,
                height: 720,
                ..play("c", "film", MONDAY + 700, 2000.0)
            },
        ];
        let v = insights(&rows, &Zone::Fixed(0), MONDAY + 10_000);
        assert_eq!(v["completion"]["buckets"]["completed"], 1);
        assert_eq!(v["completion"]["buckets"]["partial"], 1);
        assert_eq!(v["completion"]["buckets"]["abandoned"], 1);
        assert_eq!(v["completion"]["dropoff"][0]["below25"], 1);
        assert_eq!(v["concurrency"]["peak"], 3);
        assert_eq!(v["concurrency"]["peakAt"], MONDAY + 700);
        assert_eq!(v["resolution"].as_array().unwrap().len(), 3);
        assert_eq!(v["genres"][0]["title"], "Drame");
        assert_eq!(v["clients"][0]["transcoded"], 1);
        assert_eq!(v["weekdays"][0]["plays"], 3);
        assert_eq!(v["hours"][0]["plays"], 3);
    }

    #[test]
    fn people_rank_directors_and_lead_actors() {
        let credits = vec![
            (
                json!({"directors":["Agnès"],"actors":[{"name":"Lou"},{"name":"Max"}]}),
                100.0,
            ),
            (json!({"directors":["Agnès"],"actors":["Lou"]}), 50.0),
        ];
        let ranked = people(&credits, 3);
        assert_eq!(ranked[0]["titles"], 2);
        assert_eq!(ranked.len(), 3);
    }

    #[test]
    fn client_families_are_readable() {
        assert_eq!(
            client_family("Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0"),
            "Firefox · Linux"
        );
        assert_eq!(
            client_family(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit Version/17.0 Mobile Safari/604.1"
            ),
            "Safari · iOS"
        );
        assert_eq!(client_family(""), "Appareil inconnu");
    }
}

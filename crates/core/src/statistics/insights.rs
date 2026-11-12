//! Insights, per-account profiles and the yearly Wrapped (HTTP layer).
//!
//! Every query goes through `reports::select`, so account isolation, library
//! permissions and parental policy stay identical to the existing reports.
use super::aggregate::{self, Local, Play, Totals, Zone};
use super::reports::{Filter, NAME, Selection, select};
use super::*;
use axum::extract::Query;

/// Bound on the sessions aggregated in memory for one response.
const ROW_LIMIT: usize = 100_000;

fn genres(csv: &str) -> Vec<String> {
    csv.split('|')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .take(16)
        .map(|g| {
            let mut chars = g.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect()
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

async fn fetch(auth: &AuthState, selection: &Selection) -> Outcome<(Vec<Play>, bool)> {
    let epoch = now();
    let rows = selection
        .statement(
            &format!(
                "SELECT s.id,s.user_id,{NAME} AS \
                    user_name,s.media_id,s.canonical_id,s.kind,s.title,s.artist,s.album,s.device,\
                    s.started_at,s.updated_at,s.ended_at,s.seconds,s.covered_seconds,s.duration,\
                    s.completed,COALESCE(json_extract(s.state_json,'$.method'),'unknown') AS \
                    method,CAST(COALESCE(w.max_height,0) AS BIGINT) AS \
                    height,COALESCE(w.genres_csv,'|') AS genres,(SELECT se.series_id \
                    FROM inventory_episodes ep JOIN inventory_seasons se ON se.id=ep.season_id \
                    WHERE ep.work_id=s.media_id) AS series_id"
            ),
            &format!("ORDER BY s.started_at,s.id LIMIT {}", ROW_LIMIT + 1),
        )
        .fetch_all(&auth.store.pool)
        .await
        .map_err(db)?;
    let truncated = rows.len() > ROW_LIMIT;
    let mut plays: Vec<Play> = rows
        .iter()
        .take(ROW_LIMIT)
        .map(|r| {
            let ended: Option<i64> = r.get("ended_at");
            let updated: i64 = r.get("updated_at");
            Play {
                id: r.get("id"),
                user_id: r.get("user_id"),
                user: r.get("user_name"),
                media_id: r.get("media_id"),
                canonical_id: r.get("canonical_id"),
                kind: r.get("kind"),
                title: r.get("title"),
                artist: r.get("artist"),
                album: r.get("album"),
                device: r.get("device"),
                started_at: r.get("started_at"),
                ended_at: ended.unwrap_or(updated),
                active: ended.is_none() && updated >= epoch - 90,
                seconds: r.get("seconds"),
                covered: r.get("covered_seconds"),
                duration: r.get("duration"),
                completed: r.get::<i64, _>("completed") != 0,
                method: r.get("method"),
                height: r.get("height"),
                genres: genres(&r.get::<String, _>("genres")),
                series_id: r.get("series_id"),
                series_title: None,
            }
        })
        .collect();
    // Episodes roll up to their show: title and genres come from the series work.
    let mut series: Vec<String> = plays.iter().filter_map(|p| p.series_id.clone()).collect();
    series.sort();
    series.dedup();
    let mut shows = std::collections::HashMap::new();
    for chunk in series.chunks(400) {
        let sql = format!(
            "SELECT id,COALESCE(json_extract(manual_json,'$.title'),title) AS title,genres_csv FROM inventory_works WHERE id IN ({})",
            placeholders(chunk.len())
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(&sql)));
        for id in chunk {
            q = q.bind(id);
        }
        for row in q.fetch_all(&auth.store.pool).await.map_err(db)? {
            shows.insert(
                row.get::<String, _>("id"),
                (
                    row.get::<String, _>("title"),
                    genres(&row.get::<String, _>("genres_csv")),
                ),
            );
        }
    }
    for play in &mut plays {
        if let Some((title, show_genres)) = play.series_id.as_ref().and_then(|id| shows.get(id)) {
            play.series_title = Some(title.clone());
            if play.genres.is_empty() {
                play.genres = show_genres.clone();
            }
        }
    }
    Ok((plays, truncated))
}

/// Editorial credits (manual overrides first) of the most watched works.
async fn credits(auth: &AuthState, weights: &[(String, f64)]) -> Outcome<Vec<(Value, f64)>> {
    let mut out = Vec::new();
    for chunk in weights.chunks(100) {
        let sql = format!(
            "SELECT id,editorial_json,manual_json FROM inventory_works WHERE id IN ({})",
            placeholders(chunk.len())
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(&sql)));
        for (id, _) in chunk {
            q = q.bind(id);
        }
        for row in q.fetch_all(&auth.store.pool).await.map_err(db)? {
            let id: String = row.get("id");
            let mut metadata: Value = serde_json::from_str(&row.get::<String, _>("editorial_json"))
                .unwrap_or(Value::Null);
            if let Some(manual) = row
                .get::<Option<String>, _>("manual_json")
                .and_then(|m| serde_json::from_str::<Value>(&m).ok())
                .and_then(|m| m.as_object().cloned())
                && let Some(object) = metadata.as_object_mut()
            {
                object.extend(manual);
            }
            if let Some((_, seconds)) = chunk.iter().find(|(work, _)| *work == id) {
                out.push((metadata, *seconds));
            }
        }
    }
    Ok(out)
}

/// (season, episode) numbers of the episodes present in `rows`.
async fn episode_numbers(
    auth: &AuthState,
    rows: &[Play],
) -> Outcome<std::collections::HashMap<String, (i64, i64)>> {
    let mut ids: Vec<&str> = rows
        .iter()
        .filter(|p| p.kind == "episode")
        .map(|p| p.media_id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut out = std::collections::HashMap::new();
    for chunk in ids.chunks(400) {
        let sql = format!(
            "SELECT ep.work_id,CAST(se.season_number AS BIGINT) AS season,CAST(ep.episode_number AS BIGINT) AS episode FROM inventory_episodes ep JOIN inventory_seasons se ON se.id=ep.season_id WHERE ep.work_id IN ({})",
            placeholders(chunk.len())
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(&sql)));
        for id in chunk {
            q = q.bind(*id);
        }
        for row in q.fetch_all(&auth.store.pool).await.map_err(db)? {
            out.insert(
                row.get::<String, _>("work_id"),
                (row.get::<i64, _>("season"), row.get::<i64, _>("episode")),
            );
        }
    }
    Ok(out)
}

async fn previous_totals(auth: &AuthState, selection: &Selection) -> Outcome<Totals> {
    let row = selection
        .statement(&format!("SELECT COALESCE(SUM(s.seconds),0.0) AS seconds,CAST(COALESCE(SUM(CASE WHEN {} THEN 1 \
                ELSE 0 END),0) AS BIGINT) AS plays,COUNT(DISTINCT CASE WHEN s.seconds>0 \
                THEN s.user_id ELSE NULL END) AS users,COUNT(DISTINCT CASE WHEN s.seconds>0 \
                THEN s.canonical_id ELSE NULL END) AS media,CAST(COALESCE(SUM(s.completed),0) AS \
                BIGINT) AS completed", aggregate::QUALIFIED_SQL), "")
        .fetch_one(&auth.store.pool)
        .await
        .map_err(db)?;
    Ok(Totals {
        seconds: row.get("seconds"),
        plays: row.get("plays"),
        users: row.get("users"),
        media: row.get("media"),
        completed: row.get("completed"),
    })
}

async fn analyse(
    auth: &AuthState,
    headers: &HeaderMap,
    f: Filter,
    admin: bool,
) -> Outcome<(Value, Vec<Play>)> {
    let zone = Zone::parse(&f.tz).ok_or(StatusCode::BAD_REQUEST)?;
    let (_, selection) = select(auth, headers, &f, admin).await?;
    let (rows, truncated) = fetch(auth, &selection).await?;
    let epoch = now();
    let mut value = aggregate::insights(&rows, &zone, epoch);
    let from = f.from.unwrap_or(epoch - 30 * 86400);
    let to = f.to.unwrap_or(epoch + 1);
    let previous = if from > 0 {
        let window = Filter {
            from: Some((from - (to - from)).max(0)),
            to: Some(from),
            ..f.clone()
        };
        let (_, selection) = select(auth, headers, &window, admin).await?;
        previous_totals(auth, &selection).await?
    } else {
        Totals::default()
    };
    value["growth"] = aggregate::kpi_growth(Totals::of(&rows), previous);
    value["people"] = json!(aggregate::people(
        &credits(auth, &aggregate::work_weights(&rows, 50)).await?,
        10
    ));
    value["truncated"] = json!(truncated);
    value["period"] = json!({"from":from,"to":to});
    if !f.media_id.is_empty() {
        let numbers = episode_numbers(auth, &rows).await?;
        value["media"] = aggregate::media(&rows, &numbers, admin);
    }
    Ok((value, rows))
}

pub(super) async fn admin(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(f): Query<Filter>,
) -> Outcome<Json<Value>> {
    Ok(Json(analyse(&auth, &headers, f, true).await?.0))
}
pub(super) async fn personal(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(f): Query<Filter>,
) -> Outcome<Json<Value>> {
    Ok(Json(analyse(&auth, &headers, f, false).await?.0))
}

async fn account(store: &IdentityStore, id: &str) -> Outcome<Option<(String, String)>> {
    let row = query(sqlx::AssertSqlSafe(format!("SELECT {NAME} AS name,u.role FROM users u LEFT JOIN local_credentials c ON c.user_id=u.id WHERE u.id=?")))
        .bind(id)
        .fetch_optional(&store.pool)
        .await
        .map_err(db)?;
    Ok(row.map(|r| (r.get("name"), r.get("role"))))
}

/// Administrator drill-down: one account's analytics over the filtered period.
pub(super) async fn profile(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(mut f): Query<Filter>,
) -> Outcome<Json<Value>> {
    administrator(&auth, &headers, false).await?;
    if id.is_empty() || id.len() > 160 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (name, role) = account(&auth.store, &id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    f.user_id = id.clone();
    let (mut value, rows) = analyse(&auth, &headers, f, true).await?;
    value["user"] = json!({"id":id,"name":name,"role":role});
    value["recent"] = json!(rows.iter().rev().take(10).map(|p| json!({"id":p.id,"title":p.title,"kind":p.kind,"startedAt":p.started_at,
        "seconds":p.seconds,"device":aggregate::client_family(&p.device),"completed":p.completed,"user":p.user})).collect::<Vec<_>>());
    value["firstPlay"] = json!(rows.first().map(|p| p.started_at));
    value["lastPlay"] = json!(rows.last().map(|p| p.started_at));
    Ok(Json(value))
}

#[derive(Clone, Copy, sqlx::FromRow)]
pub(super) struct WrappedSettings {
    visible: i64,
    period_enabled: i64,
    start_month: i64,
    start_day: i64,
    end_month: i64,
    end_day: i64,
}
impl WrappedSettings {
    /// Visible, and inside the yearly window when one is configured (it may
    /// wrap around New Year, e.g. 1 December → 31 January).
    fn open(&self, local: Local) -> bool {
        if self.visible == 0 {
            return false;
        }
        if self.period_enabled == 0 {
            return true;
        }
        let today = i64::from(local.month) * 100 + i64::from(local.dom);
        let start = self.start_month * 100 + self.start_day;
        let end = self.end_month * 100 + self.end_day;
        if start <= end {
            (start..=end).contains(&today)
        } else {
            today >= start || today <= end
        }
    }
    fn json(&self) -> Value {
        json!({"visible":self.visible!=0,"periodEnabled":self.period_enabled!=0,"startMonth":self.start_month,
            "startDay":self.start_day,"endMonth":self.end_month,"endDay":self.end_day})
    }
}
async fn wrapped_settings(store: &IdentityStore) -> Outcome<WrappedSettings> {
    query_as("SELECT visible,period_enabled,start_month,start_day,end_month,end_day FROM statistics_wrapped_settings WHERE singleton=1")
        .fetch_one(&store.pool)
        .await
        .map_err(db)
}
/// Whether the Wrapped is currently offered to accounts.
pub(super) async fn wrapped_open(store: &IdentityStore, tz: &str, admin: bool) -> Outcome<bool> {
    let zone = Zone::parse(tz).unwrap_or(Zone::Fixed(0));
    Ok(admin || wrapped_settings(store).await?.open(zone.local(now())))
}

pub(super) async fn read_wrapped_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Outcome<Json<Value>> {
    administrator(&auth, &headers, false).await?;
    Ok(Json(wrapped_settings(&auth.store).await?.json()))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WrappedInput {
    visible: bool,
    period_enabled: bool,
    start_month: i64,
    start_day: i64,
    end_month: i64,
    end_day: i64,
}
fn valid_day(month: i64, day: i64) -> bool {
    let last = match month {
        2 => 29,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => return false,
    };
    (1..=last).contains(&day)
}
pub(super) async fn save_wrapped_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<WrappedInput>,
) -> Outcome<StatusCode> {
    administrator(&auth, &headers, true).await?;
    if !valid_day(input.start_month, input.start_day) || !valid_day(input.end_month, input.end_day)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    query("UPDATE statistics_wrapped_settings SET visible=?,period_enabled=?,start_month=?,start_day=?,end_month=?,end_day=? WHERE singleton=1")
        .bind(i64::from(input.visible)).bind(i64::from(input.period_enabled))
        .bind(input.start_month).bind(input.start_day).bind(input.end_month).bind(input.end_day)
        .execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WrappedQuery {
    year: Option<i64>,
    kind: String,
    tz: String,
}
pub(super) async fn personal_wrapped(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(q): Query<WrappedQuery>,
) -> Outcome<Json<Value>> {
    wrapped(auth, headers, None, q).await
}
pub(super) async fn admin_wrapped(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(user): Path<String>,
    Query(q): Query<WrappedQuery>,
) -> Outcome<Json<Value>> {
    if user.is_empty() || user.len() > 160 {
        return Err(StatusCode::BAD_REQUEST);
    }
    wrapped(auth, headers, Some(user), q).await
}

async fn wrapped(
    auth: AuthState,
    headers: HeaderMap,
    target: Option<String>,
    q: WrappedQuery,
) -> Outcome<Json<Value>> {
    let zone = Zone::parse(&q.tz).ok_or(StatusCode::BAD_REQUEST)?;
    let kind = match q.kind.as_str() {
        "" | "all" => "",
        "film" | "episode" | "music" => q.kind.as_str(),
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let admin = target.is_some();
    let epoch = now();
    let local_now = zone.local(epoch);
    let base = Filter {
        from: Some(0),
        to: Some(epoch + 1),
        user_id: target.clone().unwrap_or_default(),
        tz: q.tz.clone(),
        ..Filter::default()
    };
    let (actor, selection) = select(&auth, &headers, &base, admin).await?;
    let config = wrapped_settings(&auth.store).await?;
    if !admin && !config.open(local_now) {
        return Ok(Json(json!({"visible":false,"settings":config.json()})));
    }
    let user_id = target.unwrap_or_else(|| actor.user.id.clone());
    let (name, _) = account(&auth.store, &user_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    let first: Option<i64> = selection
        .statement("SELECT MIN(s.started_at) AS first", "")
        .fetch_one(&auth.store.pool)
        .await
        .map_err(db)?
        .get("first");
    let oldest = first.map_or(local_now.year, |t| zone.local(t).year.min(local_now.year));
    let years: Vec<i64> = (oldest..=local_now.year).rev().collect();
    // In January the recap of the year that just ended is the natural default.
    let default_year = if local_now.month == 1 && oldest < local_now.year {
        local_now.year - 1
    } else {
        local_now.year
    };
    let year = q.year.unwrap_or(default_year);
    if !(1970..=local_now.year).contains(&year) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let year_rows = |year: i64| {
        let from = zone.year_start(year).max(0);
        let to = zone.year_start(year + 1).min(epoch + 1);
        Filter {
            from: Some(from),
            to: Some(to),
            user_id: if admin {
                user_id.clone()
            } else {
                String::new()
            },
            kind: kind.into(),
            tz: q.tz.clone(),
            ..Filter::default()
        }
    };
    let current = year_rows(year);
    let rows = if current.to > current.from {
        let (_, selection) = select(&auth, &headers, &current, admin).await?;
        fetch(&auth, &selection).await?.0
    } else {
        Vec::new()
    };
    let previous_filter = year_rows(year - 1);
    let previous = if year > oldest && previous_filter.to > previous_filter.from {
        let (_, selection) = select(&auth, &headers, &previous_filter, admin).await?;
        let previous_rows = fetch(&auth, &selection).await?.0;
        (!previous_rows.is_empty()).then(|| aggregate::wrapped_totals(&previous_rows))
    } else {
        None
    };
    let qualified: Vec<Play> = rows.iter().filter(|p| p.qualifies()).cloned().collect();
    let mut value = aggregate::wrapped(&rows, &zone, year, previous);
    value["people"] = json!(aggregate::people(
        &credits(&auth, &aggregate::work_weights(&qualified, 40)).await?,
        5
    ));
    value["visible"] = json!(true);
    value["availableYears"] = json!(years);
    value["kind"] = json!(if kind.is_empty() { "all" } else { kind });
    value["user"] = json!({"id":user_id,"name":name});
    value["settings"] = config.json();
    value["generatedAt"] = json!(epoch);
    Ok(Json(value))
}

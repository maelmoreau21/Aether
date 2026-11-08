use super::*;
use axum::{
    extract::Query,
    http::header,
    response::{IntoResponse, Response},
};

#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Filter {
    pub(super) from: Option<i64>,
    pub(super) to: Option<i64>,
    pub(super) user_id: String,
    pub(super) library_id: String,
    pub(super) node_id: String,
    pub(super) kind: String,
    pub(super) device: String,
    pub(super) offset: usize,
    pub(super) limit: Option<usize>,
    pub(super) rank: String,
    /// Time zone for calendar buckets (insights): IANA name or minutes east of UTC.
    pub(super) tz: String,
}
pub(super) struct Selection {
    pub(super) sql: String,
    pub(super) binds: Vec<String>,
}
impl Selection {
    pub(super) fn statement(
        &self,
        select: &str,
        tail: &str,
    ) -> sqlx::query::Query<'_, sqlx::Any, sqlx::any::AnyArguments> {
        let text = crate::database::normalize_sql(&format!("{select} {} {tail}", self.sql));
        let mut q = sqlx::query(sqlx::AssertSqlSafe(text));
        for value in &self.binds {
            q = q.bind(value);
        }
        q
    }
}
pub(super) async fn select(
    auth: &AuthState,
    headers: &HeaderMap,
    f: &Filter,
    admin: bool,
) -> Outcome<(Authenticated, Selection)> {
    let actor = if admin {
        administrator(auth, headers, false).await?
    } else {
        auth.authenticate(headers).await?
    };
    if !admin
        && actor.user.role != Role::Administrator
        && (settings(&auth.store).await?.enabled == 0 || !users_can_view(&auth.store).await?)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    if !admin && !f.user_id.is_empty() && f.user_id != actor.user.id {
        return Err(StatusCode::FORBIDDEN);
    }
    if !crate::product::access_now(&auth.store, &actor.user.id, now()).await? {
        return Err(StatusCode::FORBIDDEN);
    }
    let from = f.from.unwrap_or(now() - 30 * 86400);
    let to = f.to.unwrap_or(now() + 1);
    if from < 0
        || to <= from
        || to - from > 36600 * 86400
        || to > now() + 86400
        || f.offset > 100000
        || f.limit.is_some_and(|v| v == 0 || v > 100)
        || [&f.user_id, &f.library_id, &f.node_id, &f.kind, &f.device]
            .iter()
            .any(|v| v.len() > 160)
        || super::aggregate::Zone::parse(&f.tz).is_none()
        || !matches!(f.kind.as_str(), "" | "film" | "episode" | "music")
        || !matches!(
            f.rank.as_str(),
            "" | "users" | "seconds" | "completion" | "favorites" | "rating"
        )
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut sql="FROM statistics_sessions s JOIN users u ON u.id=s.user_id LEFT JOIN local_credentials c ON c.user_id=u.id LEFT JOIN libraries l ON l.id=s.library_id LEFT JOIN media_nodes n ON n.id=l.node_id LEFT JOIN inventory_works w ON w.id=s.media_id AND s.kind!='music' LEFT JOIN music_tracks t ON t.id=s.media_id AND s.kind='music' WHERE s.started_at>=CAST(? AS BIGINT) AND s.started_at<CAST(? AS BIGINT)".to_owned();
    let mut binds = vec![from.to_string(), to.to_string()];
    for (column, value) in [
        ("s.user_id", &f.user_id),
        ("s.library_id", &f.library_id),
        ("s.node_id", &f.node_id),
        ("s.kind", &f.kind),
        ("s.device", &f.device),
    ] {
        if !value.is_empty() {
            sql.push_str(&format!(" AND {column}=?"));
            binds.push(value.clone());
        }
    }
    if !admin {
        sql.push_str(" AND s.user_id=? AND l.enabled=1 AND n.status!='revoked' AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) AND ((s.kind='music' AND t.id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM parental_policies p WHERE p.user_id=? AND p.allow_unrated=0)) OR (s.kind!='music' AND w.id IS NOT NULL AND ");
        sql.push_str(&crate::product::policy_sql());
        sql.push_str("))");
        binds.extend([
            actor.user.id.clone(),
            actor.user.role.as_str().to_owned(),
            actor.user.id.clone(),
            actor.user.id.clone(),
            actor.user.id.clone(),
        ]);
    }
    Ok((actor, Selection { sql, binds }))
}
fn number(row: &sqlx::any::AnyRow, name: &str) -> f64 {
    row.get::<f64, _>(name)
}
fn count(row: &sqlx::any::AnyRow, name: &str) -> i64 {
    row.get::<i64, _>(name)
}
fn session(row: &sqlx::any::AnyRow, epoch: i64, details: bool) -> Outcome<Value> {
    let mut state: Value = serde_json::from_str(&row.get::<String, _>("state_json"))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(o) = state.as_object_mut() {
        o.remove("previous");
        if !details {
            o.remove("ranges");
        }
    }
    let ended: Option<i64> = row.get("ended_at");
    let updated: i64 = row.get("updated_at");
    let reason: String = row.get("end_reason");
    let stale = ended.is_none() && updated < epoch - 90;
    Ok(
        json!({"id":row.get::<String,_>("id"),"userId":row.get::<String,_>("user_id"),
        "user":row.get::<String,_>("user_name"),"mediaId":row.get::<String,_>("media_id"),"canonicalId":row.get::<String,_>("canonical_id"),
        "title":row.get::<String,_>("title"),"artist":row.get::<String,_>("artist"),"album":row.get::<String,_>("album"),
        "kind":row.get::<String,_>("kind"),"libraryId":row.get::<String,_>("library_id"),"nodeId":row.get::<String,_>("node_id"),
        "device":row.get::<String,_>("device"),"startedAt":row.get::<i64,_>("started_at"),"updatedAt":updated,
        "endedAt":ended.or(if stale{Some(updated)}else{None}),"endReason":if stale{"interrupted"}else{&reason},
        "status":if stale||reason=="interrupted"{"interrupted"}else if ended.is_none(){"active"}else if row.get::<i64,_>("completed")==1{"completed"}else{"partial"},
        "seconds":row.get::<f64,_>("seconds"),"coveredSeconds":row.get::<f64,_>("covered_seconds"),"duration":row.get::<f64,_>("duration"),
        "completed":row.get::<i64,_>("completed")!=0,"metrics":state}),
    )
}
pub(super) const NAME: &str = "COALESCE(u.display_name,u.sso_username,c.login,'Utilisateur')";

pub(super) async fn admin(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(f): Query<Filter>,
) -> Outcome<Json<Value>> {
    report(auth, headers, f, true).await
}
pub(super) async fn personal(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(f): Query<Filter>,
) -> Outcome<Json<Value>> {
    report(auth, headers, f, false).await
}
async fn report(
    auth: AuthState,
    headers: HeaderMap,
    f: Filter,
    admin: bool,
) -> Outcome<Json<Value>> {
    let (actor, selection) = select(&auth, &headers, &f, admin).await?;
    let epoch = now();
    let row=selection.statement(&format!("SELECT COUNT(*) AS plays,COUNT(DISTINCT CASE WHEN s.seconds>0 THEN s.user_id ELSE NULL END) AS users,COUNT(DISTINCT CASE WHEN s.seconds>0 THEN s.canonical_id ELSE NULL END) AS media,COALESCE(SUM(s.seconds),0.0) AS seconds,COALESCE(SUM(s.covered_seconds),0.0) AS coverage,CAST(COALESCE(SUM(s.completed),0) AS BIGINT) AS completed,CAST(COALESCE(SUM(CASE WHEN s.ended_at IS NULL AND s.updated_at>=unixepoch()-90 THEN 1 ELSE 0 END),0) AS BIGINT) AS active,CAST(COALESCE(SUM(CASE WHEN {} THEN 1 ELSE 0 END),0) AS BIGINT) AS qualified", super::aggregate::QUALIFIED_SQL), "").fetch_one(&auth.store.pool).await.map_err(db)?;
    let plays = count(&row, "plays");
    let summary = json!({"plays":plays,"qualifiedPlays":count(&row,"qualified"),"users":count(&row,"users"),"media":count(&row,"media"),"seconds":number(&row,"seconds"),"coveredSeconds":number(&row,"coverage"),"completed":count(&row,"completed"),"active":count(&row,"active")});
    let limit = f.limit.unwrap_or(50);
    let history = selection
        .statement(
            &format!("SELECT s.*,{NAME} AS user_name"),
            &format!(
                "ORDER BY s.started_at DESC,s.id LIMIT {limit} OFFSET {}",
                f.offset
            ),
        )
        .fetch_all(&auth.store.pool)
        .await
        .map_err(db)?;
    let history = history
        .iter()
        .map(|r| session(r, epoch, false))
        .collect::<Outcome<Vec<_>>>()?;
    let feedback_user = if admin {
        String::new()
    } else {
        format!(" AND f.user_id={}", "?")
    };
    let popular_select = format!(
        "SELECT s.canonical_id,s.kind,MAX(s.title) AS title,COUNT(*) AS plays,COUNT(DISTINCT CASE WHEN s.seconds>0 THEN s.user_id ELSE NULL END) AS users,COALESCE(SUM(s.seconds),0.0) AS seconds,CAST(COALESCE(SUM(s.completed),0) AS BIGINT) AS completed,(SELECT COUNT(*) FROM work_feedback f WHERE f.work_id=s.canonical_id AND f.favorite=1{feedback_user}) AS favorites,(SELECT COALESCE(AVG(f.rating),0.0) FROM work_feedback f WHERE f.work_id=s.canonical_id{feedback_user}) AS rating"
    );
    let order = match f.rank.as_str() {
        "seconds" => "seconds DESC",
        "completion" => "SUM(s.completed)*1.0/COUNT(*) DESC",
        "favorites" => "favorites DESC",
        "rating" => "rating DESC",
        _ => "users DESC",
    };
    // SELECT parameters precede WHERE parameters. No client value becomes SQL syntax.
    let mut popular_selection = Selection {
        sql: selection.sql.clone(),
        binds: Vec::new(),
    };
    if !admin {
        popular_selection
            .binds
            .extend([actor.user.id.clone(), actor.user.id.clone()]);
    }
    popular_selection.binds.extend(selection.binds.clone());
    let rows=popular_selection.statement(&popular_select,&format!("GROUP BY s.canonical_id,s.kind ORDER BY {order},seconds DESC,s.canonical_id LIMIT 30")).fetch_all(&auth.store.pool).await.map_err(db)?;
    let popular:Vec<Value>=rows.iter().map(|r|json!({"id":r.get::<String,_>("canonical_id"),"kind":r.get::<String,_>("kind"),"title":r.get::<String,_>("title"),"plays":count(r,"plays"),"users":count(r,"users"),"seconds":number(r,"seconds"),"completed":count(r,"completed"),"favorites":count(r,"favorites"),"rating":number(r,"rating")})).collect();
    let rows=selection.statement(&format!("SELECT s.user_id,MAX({NAME}) AS name,COUNT(*) AS plays,COALESCE(SUM(s.seconds),0.0) AS seconds"),"GROUP BY s.user_id ORDER BY seconds DESC,s.user_id LIMIT 100").fetch_all(&auth.store.pool).await.map_err(db)?;
    let users:Vec<Value>=rows.iter().map(|r|json!({"id":r.get::<String,_>("user_id"),"name":r.get::<String,_>("name"),"plays":count(r,"plays"),"seconds":number(r,"seconds")})).collect();
    let daily_sql = format!(
        "SELECT d.day,COALESCE(SUM(d.seconds),0.0) AS seconds FROM statistics_daily d JOIN statistics_sessions selected ON selected.id=d.session_id WHERE d.session_id IN (SELECT s.id {}) GROUP BY d.day ORDER BY d.day LIMIT 36601",
        selection.sql
    );
    let mut daily_q = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(
        &daily_sql,
    )));
    for b in &selection.binds {
        daily_q = daily_q.bind(b);
    }
    let rows = daily_q.fetch_all(&auth.store.pool).await.map_err(db)?;
    let daily: Vec<Value> = rows
        .iter()
        .map(|r| json!({"day":count(r,"day"),"seconds":number(r,"seconds")}))
        .collect();
    let rows=selection.statement("SELECT (CAST(s.started_at/3600 AS BIGINT)+72)%168 AS hour,COUNT(*) AS plays,COALESCE(SUM(s.seconds),0.0) AS seconds","GROUP BY (CAST(s.started_at/3600 AS BIGINT)+72)%168 ORDER BY hour").fetch_all(&auth.store.pool).await.map_err(db)?;
    let hours:Vec<Value>=rows.iter().map(|r|json!({"hour":count(r,"hour"),"plays":count(r,"plays"),"seconds":number(r,"seconds")})).collect();
    let mut behavior = serde_json::Map::new();
    let fields = [
        "pauses",
        "resumes",
        "seeksForward",
        "seeksBackward",
        "audioChanges",
        "subtitleChanges",
        "speedChanges",
        "introSkips",
        "creditsSkips",
        "otherSkips",
        "bufferingCount",
        "bufferingSeconds",
        "errors",
    ];
    let expressions=fields.iter().map(|field|format!("COALESCE(SUM(CAST(COALESCE(json_extract(s.state_json,'$.{field}'),'0') AS DOUBLE PRECISION)),0.0) AS \"{field}\"")).collect::<Vec<_>>().join(",");
    let row=selection.statement(&format!("SELECT {expressions},COALESCE(AVG(CAST(json_extract(s.state_json,'$.startupMs') AS DOUBLE PRECISION)),0.0) AS startup"),"").fetch_one(&auth.store.pool).await.map_err(db)?;
    for field in fields {
        behavior.insert(field.into(), json!(number(&row, field)));
    }
    behavior.insert("startupMs".into(), json!(number(&row, "startup")));
    let row=selection.statement("SELECT COUNT(*) AS plays,COUNT(DISTINCT CAST(length(s.user_id) AS TEXT) || ':' || s.user_id || s.canonical_id) AS pairs,CAST(COALESCE(SUM(CASE WHEN s.kind='music' AND s.end_reason='next' THEN 1 ELSE 0 END),0) AS DOUBLE PRECISION) AS music_skips,COALESCE(SUM(CAST(COALESCE(json_extract(s.state_json,'$.mediaSeconds'),'0') AS DOUBLE PRECISION)-s.covered_seconds),0.0) AS repeated_seconds", "").fetch_one(&auth.store.pool).await.map_err(db)?;
    behavior.insert(
        "additionalSessions".into(),
        json!(count(&row, "plays") - count(&row, "pairs")),
    );
    behavior.insert("musicSkips".into(), json!(number(&row, "music_skips")));
    behavior.insert(
        "repeatedMediaSeconds".into(),
        json!(number(&row, "repeated_seconds").max(0.0)),
    );
    let mut distributions = serde_json::Map::new();
    for (name, expression) in [
        (
            "initialAudio",
            "COALESCE(json_extract(json_extract(s.state_json,'$.initialAudio'),'$.language'),'und')",
        ),
        (
            "audio",
            "COALESCE(json_extract(json_extract(s.state_json,'$.audio'),'$.language'),'und')",
        ),
        (
            "subtitles",
            "COALESCE(json_extract(json_extract(s.state_json,'$.subtitle'),'$.language'),'off')",
        ),
        (
            "method",
            "COALESCE(json_extract(s.state_json,'$.method'),'unknown')",
        ),
        ("device", "s.device"),
        ("kind", "s.kind"),
        ("node", "s.node_id"),
    ] {
        let rows=selection.statement(&format!("SELECT {expression} AS label,COUNT(*) AS plays,COALESCE(SUM(s.seconds),0.0) AS seconds"),&format!("GROUP BY {expression} ORDER BY plays DESC,label LIMIT 50")).fetch_all(&auth.store.pool).await.map_err(db)?;
        distributions.insert(name.into(),json!(rows.iter().map(|r|json!({"label":r.get::<String,_>("label"),"plays":count(r,"plays"),"seconds":number(r,"seconds")})).collect::<Vec<_>>()));
    }
    for dimension in ["audio", "subtitles", "method"] {
        let sql = format!(
            "SELECT d.label,COUNT(DISTINCT d.session_id) AS plays,COALESCE(SUM(d.seconds),0.0) AS seconds FROM statistics_dimensions d WHERE d.dimension='{dimension}' AND d.session_id IN (SELECT s.id {}) GROUP BY d.label ORDER BY seconds DESC,d.label LIMIT 50",
            selection.sql
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(&sql)));
        for value in &selection.binds {
            q = q.bind(value);
        }
        let rows = q.fetch_all(&auth.store.pool).await.map_err(db)?;
        distributions.insert(format!("{dimension}Time"),json!(rows.iter().map(|r|json!({"label":r.get::<String,_>("label"),"plays":count(r,"plays"),"seconds":number(r,"seconds")})).collect::<Vec<_>>()));
    }
    let mut music = serde_json::Map::new();
    for name in ["artist", "album"] {
        let rows=selection.statement(&format!("SELECT s.{name} AS label,COUNT(*) AS plays,COALESCE(SUM(s.seconds),0.0) AS seconds"),&format!("AND s.kind='music' GROUP BY s.{name} ORDER BY seconds DESC,label LIMIT 30")).fetch_all(&auth.store.pool).await.map_err(db)?;
        music.insert(name.into(),json!(rows.iter().map(|r|json!({"label":r.get::<String,_>("label"),"plays":count(r,"plays"),"seconds":number(r,"seconds")})).collect::<Vec<_>>()));
    }
    let periods=query("SELECT started_at,ended_at FROM statistics_collection_periods ORDER BY started_at DESC LIMIT 1000").fetch_all(&auth.store.pool).await.map_err(db)?;
    let libraries = selection
        .statement(
            "SELECT DISTINCT s.library_id,COALESCE(l.name,s.library_id) AS name",
            "ORDER BY name,s.library_id LIMIT 200",
        )
        .fetch_all(&auth.store.pool)
        .await
        .map_err(db)?;
    let nodes = selection
        .statement("SELECT DISTINCT s.node_id", "ORDER BY s.node_id LIMIT 200")
        .fetch_all(&auth.store.pool)
        .await
        .map_err(db)?;
    let devices = selection
        .statement("SELECT DISTINCT s.device", "ORDER BY s.device LIMIT 200")
        .fetch_all(&auth.store.pool)
        .await
        .map_err(db)?;
    let facets = json!({"libraries":libraries.iter().map(|r|json!({"id":r.get::<String,_>("library_id"),"name":r.get::<String,_>("name")})).collect::<Vec<_>>(),"nodes":nodes.iter().map(|r|r.get::<String,_>("node_id")).collect::<Vec<_>>(),"devices":devices.iter().map(|r|r.get::<String,_>("device")).collect::<Vec<_>>()});
    let s = settings(&auth.store).await?;
    let wrapped = super::insights::wrapped_open(&auth.store, &f.tz, admin).await?;
    Ok(Json(
        json!({"enabled":s.enabled!=0,"usersCanView":users_can_view(&auth.store).await?,"firstEnabledAt":s.first_enabled_at,"eventRetentionDays":s.event_retention_days,"summaryRetentionDays":s.summary_retention_days,"wrappedAvailable":wrapped,
        "periods":periods.iter().map(|r|json!({"startedAt":r.get::<i64,_>("started_at"),"endedAt":r.get::<Option<i64>,_>("ended_at")})).collect::<Vec<_>>(),
        "summary":summary,"sessions":history,"offset":f.offset,"hasMore":f.offset+history.len()<(plays as usize),
        "popular":popular,"users":users,"daily":daily,"hours":hours,"behavior":behavior,"distributions":distributions,"music":music,"facets":facets,"timezone":"UTC"}),
    ))
}

pub(super) async fn admin_detail(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(page): Query<TimelinePage>,
) -> Outcome<Json<Value>> {
    detail(auth, headers, id, true, page).await
}
pub(super) async fn personal_detail(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(page): Query<TimelinePage>,
) -> Outcome<Json<Value>> {
    detail(auth, headers, id, false, page).await
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct TimelinePage {
    after: i64,
}
async fn detail(
    auth: AuthState,
    headers: HeaderMap,
    id: String,
    admin: bool,
    page: TimelinePage,
) -> Outcome<Json<Value>> {
    if page.after < 0 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let f = Filter {
        from: Some(0),
        ..Filter::default()
    };
    let (_, mut selection) = select(&auth, &headers, &f, admin).await?;
    selection.sql.push_str(" AND s.id=?");
    selection.binds.push(id.clone());
    let row = selection
        .statement(&format!("SELECT s.*,{NAME} AS user_name"), "")
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(db)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let session = session(&row, now(), true)?;
    let config = settings(&auth.store).await?;
    let rows=query("SELECT occurred_at,event_json,sequence FROM statistics_events WHERE session_id=? AND sequence>? AND received_at>=? ORDER BY sequence LIMIT 2001")
        .bind(&id).bind(page.after).bind(now()-config.event_retention_days*86400).fetch_all(&auth.store.pool).await.map_err(db)?;
    let timeline = rows
        .iter()
        .take(2000)
        .map(|r| {
            let event: Value = serde_json::from_str(&r.get::<String, _>("event_json"))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(json!({"occurredAt":r.get::<i64,_>("occurred_at"),"event":event}))
        })
        .collect::<Outcome<Vec<_>>>()?;
    Ok(Json(
        json!({"session":session,"events":timeline,"eventsTruncated":rows.len()>2000,"nextSequence":if rows.len()>2000 {Some(rows[1999].get::<i64,_>("sequence"))}else{None},"detailsExpired":page.after==0&&rows.is_empty()&&row.get::<i64,_>("last_sequence")>0}),
    ))
}
pub(super) async fn admin_export(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(f): Query<Filter>,
) -> Outcome<Response> {
    export(auth, headers, f, true).await
}
pub(super) async fn personal_export(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(f): Query<Filter>,
) -> Outcome<Response> {
    export(auth, headers, f, false).await
}
fn csv(s: &str) -> String {
    let safe = if s.trim_start().starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.into()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
async fn export(auth: AuthState, headers: HeaderMap, f: Filter, admin: bool) -> Outcome<Response> {
    let (_, selection) = select(&auth, &headers, &f, admin).await?;
    let rows = selection
        .statement(
            &format!("SELECT s.*,{NAME} AS user_name"),
            &format!(
                "ORDER BY s.started_at DESC,s.id LIMIT 2001 OFFSET {}",
                f.offset
            ),
        )
        .fetch_all(&auth.store.pool)
        .await
        .map_err(db)?;
    let truncated = rows.len() > 2000;
    let mut body="\u{feff}session,user,title,kind,artist,album,library,node,device,started_at,seconds,covered_seconds,duration,completed,status\r\n".to_owned();
    for row in rows.iter().take(2000) {
        let s = session(row, now(), false)?;
        body.push_str(
            &[
                "id",
                "user",
                "title",
                "kind",
                "artist",
                "album",
                "libraryId",
                "nodeId",
                "device",
                "startedAt",
                "seconds",
                "coveredSeconds",
                "duration",
                "completed",
                "status",
            ]
            .iter()
            .map(|key| {
                csv(&s[key]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| s[key].to_string()))
            })
            .collect::<Vec<_>>()
            .join(","),
        );
        body.push_str("\r\n");
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=statistics.csv",
            ),
            (
                header::HeaderName::from_static("x-export-truncated"),
                if truncated { "true" } else { "false" },
            ),
        ],
        body,
    )
        .into_response())
}

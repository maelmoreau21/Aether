use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Policy {
    pub(crate) maximum_age: i64,
    pub(crate) allow_unrated: bool,
    #[serde(default)]
    pub(crate) blocked_tags: Vec<String>,
    #[serde(default)]
    pub(crate) access_schedule: Vec<AccessWindow>,
}

#[derive(serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccessWindow {
    /// Monday=0, Sunday=6. Minutes are UTC, end exclusive; split overnight windows.
    pub(crate) day: i64,
    pub(crate) start_minute: i64,
    pub(crate) end_minute: i64,
}

pub(crate) fn schedule_allows(windows: &[AccessWindow], epoch: i64) -> bool {
    let day = (epoch.div_euclid(86400) + 3).rem_euclid(7);
    let minute = epoch.rem_euclid(86400) / 60;
    windows.is_empty()
        || windows
            .iter()
            .any(|w| w.day == day && w.start_minute <= minute && minute < w.end_minute)
}

pub(crate) async fn access_now(
    store: &IdentityStore,
    user: &str,
    epoch: i64,
) -> Result<bool, Error> {
    let encoded: Option<String> =
        query_scalar("SELECT access_schedule_json FROM parental_policies WHERE user_id=?")
            .bind(user)
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match encoded {
        None => Ok(true),
        Some(encoded) => {
            let windows: Vec<AccessWindow> =
                serde_json::from_str(&encoded).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(schedule_allows(&windows, epoch))
        }
    }
}

pub(crate) async fn parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(user): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let row: Option<(i64, i64, String, String)> =
        query_as("SELECT maximum_age,allow_unrated,blocked_tags_json,access_schedule_json FROM parental_policies WHERE user_id=?")
            .bind(user)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        row.map(|(age, unrated, tags, schedule)| json!({"maximumAge":age,"allowUnrated":unrated!=0,"blockedTags":serde_json::from_str::<Value>(&tags).unwrap_or(Value::Null),"accessSchedule":serde_json::from_str::<Value>(&schedule).unwrap_or(Value::Null)}))
            .unwrap_or(Value::Null),
    ))
}

pub(crate) async fn set_parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(user): Path<String>,
    Json(input): Json<Policy>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    if !(0..=21).contains(&input.maximum_age)
        || input.blocked_tags.len() > 64
        || input
            .blocked_tags
            .iter()
            .any(|v| v.trim().is_empty() || v.len() > 100 || v.chars().any(char::is_control))
        || input.access_schedule.len() > 56
        || input.access_schedule.iter().any(|w| {
            !(0..=6).contains(&w.day)
                || !(0..1440).contains(&w.start_minute)
                || !(1..=1440).contains(&w.end_minute)
                || w.start_minute >= w.end_minute
        })
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let exists: i64 = query_scalar("SELECT COUNT(*) FROM users WHERE id=?")
        .bind(&user)
        .fetch_one(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    let tags: Vec<String> = input
        .blocked_tags
        .iter()
        .map(|v| v.trim().to_lowercase())
        .collect();
    let mut tx = auth
        .store
        .pool
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated,blocked_tags_json,access_schedule_json) VALUES(?,?,?,?,?) ON CONFLICT(user_id) DO UPDATE \
            SET maximum_age=excluded.maximum_age,allow_unrated=excluded.allow_unrated,\
            blocked_tags_json=excluded.blocked_tags_json,\
            access_schedule_json=excluded.access_schedule_json").bind(&user).bind(input.maximum_age).bind(i64::from(input.allow_unrated)).bind(json!(tags).to_string()).bind(json!(input.access_schedule).to_string()).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    query("DELETE FROM parental_blocked_tags WHERE user_id=?")
        .bind(&user)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for tag in tags {
        query("INSERT INTO parental_blocked_tags(user_id,tag_json) VALUES(?,?) ON CONFLICT DO NOTHING").bind(&user).bind(json!(tag).to_string()).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn reset_parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(user): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("DELETE FROM parental_policies WHERE user_id=?")
        .bind(user)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}

/// Normalizes a provider/NFO classification (TMDb, TVDB, IMDb, OMDb, Kodi NFO)
/// into a comparable minimum age: `FR:-16`, `-16 ans`, `FSK 16`, `GB:15`,
/// `US:PG-13`, `Rated R`, `TV-MA`, `Tous publics`, `U`... Unknown labels stay NULL
/// and follow the policy's `allow_unrated` choice.
macro_rules! rating_age_sql {
    ($expr:literal) => {
        concat!(
            "CASE replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(upper(trim(",
            $expr,
            ")),'FR:',''),'FR-',''),'US:',''),'GB:',''),'UK:',''),'DE:',''),'CA:',''),'BE:',''),'CH:',''),'FSK ',''),'FSK',''),'BBFC ',''),'RATED ',''),' ANS',''),'ANS',''),' ','') ",
            "WHEN 'G' THEN 0 WHEN 'TV-G' THEN 0 WHEN 'TV-Y' THEN 0 WHEN 'U' THEN 0 WHEN 'TOUSPUBLICS' THEN 0 WHEN 'TP' THEN 0 WHEN 'ALL' THEN 0 WHEN 'AL' THEN 0 WHEN '0' THEN 0 ",
            "WHEN '6' THEN 6 WHEN 'TV-Y7' THEN 7 WHEN '7' THEN 7 ",
            "WHEN 'PG' THEN 10 WHEN 'TV-PG' THEN 10 WHEN '10' THEN 10 WHEN '-10' THEN 10 ",
            "WHEN '12' THEN 12 WHEN '-12' THEN 12 WHEN '12A' THEN 12 WHEN 'PG-12' THEN 12 ",
            "WHEN 'PG-13' THEN 13 WHEN 'TV-14' THEN 14 WHEN '14' THEN 14 WHEN '15' THEN 15 WHEN 'MA15+' THEN 15 ",
            "WHEN '16' THEN 16 WHEN '-16' THEN 16 WHEN 'R' THEN 17 ",
            "WHEN '18' THEN 18 WHEN '-18' THEN 18 WHEN 'NC-17' THEN 18 WHEN 'TV-MA' THEN 18 WHEN 'R18' THEN 18 WHEN 'X' THEN 18 ELSE NULL END"
        )
    };
}

// The same expression is used before catalogue pagination and on every derived resource.
// Episodes inherit their series' manual age and provider classification, since
// providers and NFO files classify the series rather than each episode.
pub const AGE_SQL: &str = concat!(
    "COALESCE(w.minimum_age,(SELECT parent.minimum_age FROM inventory_episodes ep JOIN inventory_seasons season ON season.id=ep.season_id JOIN inventory_works parent ON parent.id=season.series_id WHERE ep.work_id=w.id),",
    rating_age_sql!(
        "COALESCE(json_extract(w.manual_json,'$.mpaa'),json_extract(w.editorial_json,'$.mpaa'))"
    ),
    ",(SELECT ",
    rating_age_sql!(
        "COALESCE(json_extract(parent.manual_json,'$.mpaa'),json_extract(parent.editorial_json,'$.mpaa'))"
    ),
    " FROM inventory_episodes ep JOIN inventory_seasons season ON season.id=ep.season_id JOIN inventory_works parent ON parent.id=season.series_id WHERE ep.work_id=w.id))"
);

pub fn policy_sql() -> String {
    format!(
        "NOT EXISTS(SELECT 1 FROM parental_policies policy WHERE policy.user_id=? AND (({AGE_SQL})>policy.maximum_age OR (({AGE_SQL}) IS NULL AND policy.allow_unrated=0) OR {BLOCKED_TAGS_SQL}))"
    )
}

pub const BLOCKED_TAGS_SQL: &str = "EXISTS(SELECT 1 FROM parental_blocked_tags bt WHERE bt.user_id=policy.user_id \
        AND (instr(lower(COALESCE(json_extract(w.manual_json,'$.tags'),\
        json_extract(w.editorial_json,'$.tags'),'[]')),bt.tag_json)>0 OR EXISTS(SELECT 1 \
        FROM inventory_episodes te JOIN inventory_seasons ts ON ts.id=te.season_id \
        JOIN inventory_works tw ON tw.id=ts.series_id WHERE te.work_id=w.id \
        AND instr(lower(COALESCE(json_extract(tw.manual_json,'$.tags'),\
        json_extract(tw.editorial_json,'$.tags'),'[]')),bt.tag_json)>0)))";

pub async fn parental_allowed(
    store: &IdentityStore,
    user: &str,
    work: &str,
) -> Result<bool, Error> {
    let sql = format!(
        "SELECT COUNT(*) FROM inventory_works w WHERE w.id=? AND {}",
        policy_sql()
    );
    let sql = crate::database::normalize_sql(&sql);
    let allowed: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(work)
        .bind(user)
        .fetch_one(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(allowed == 1)
}

pub async fn target_allowed(
    store: &IdentityStore,
    user: &str,
    kind: &str,
    target: &str,
) -> Result<bool, Error> {
    let policies: i64 = query_scalar("SELECT COUNT(*) FROM parental_policies WHERE user_id=?")
        .bind(user)
        .fetch_one(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if policies == 0 {
        return Ok(true);
    }
    let work: Option<String> = match kind {
        "work" => Some(target.to_owned()),
        "image" => query_scalar("SELECT work_id FROM inventory_images WHERE id=?")
            .bind(target)
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        _ => query_scalar(
            "SELECT e.work_id \
                FROM inventory_assets a \
                JOIN inventory_sources s ON s.id=a.source_id \
                JOIN inventory_versions v ON v.id=s.version_id \
                JOIN inventory_editions e ON e.id=v.edition_id \
                WHERE a.id=?",
        )
        .bind(target)
        .fetch_optional(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    };
    let Some(work) = work else {
        return Ok(false);
    };
    if kind == "work" {
        if parental_allowed(store, user, &work).await? {
            return Ok(true);
        }
        // If `work` is part of a group, allow opening the unified work card as long as at least
        // one member work in an authorized library is parental-allowed (expand_detail will swap
        // out the restricted base work metadata and exclude restricted editions/sources).
        let role_str: Option<String> = query_scalar("SELECT role FROM users WHERE id=?")
            .bind(user)
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let role = role_str.as_deref().unwrap_or("user");
        let members: Vec<String> = query_scalar(
            "SELECT gm.work_id FROM media_group_members gm \
             JOIN inventory_works w ON w.id = gm.work_id \
             JOIN libraries l ON l.id = w.library_id \
             JOIN media_nodes n ON n.id = l.node_id \
             WHERE (gm.group_id = ? OR gm.group_id = (SELECT m2.group_id FROM media_group_members m2 WHERE m2.work_id = ?)) \
               AND l.enabled = 1 AND n.status != 'revoked' \
               AND ((? = 'administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT \
               JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT \
               JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id \
               AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
               OR EXISTS(SELECT 1 FROM effective_library_permissions lp WHERE lp.library_id=l.id \
               AND lp.user_id = ?))",
        )
        .bind(&work)
        .bind(&work)
        .bind(role)
        .bind(user)
        .fetch_all(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for member in members {
            if parental_allowed(store, user, &member).await? {
                return Ok(true);
            }
        }
        Ok(false)
    } else {
        parental_allowed(store, user, &work).await
    }
}

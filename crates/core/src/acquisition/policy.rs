//! Request rights: who may request, what is approved without an administrator, and quotas.
//!
//! A rule is attached to the server (default), to a group or to one account. For every media
//! class (film, series, music) it can allow or deny requests, approve them automatically and cap
//! them over a sliding window. Resolution order: the account's own rule, then its groups, then
//! the server default, then the built-in behaviour (allowed, approval follows the global
//! `auto_approve` switch, no window quota). Rules grant neither a role nor library access:
//! administrators are always allowed, approved and unlimited, and a request is still checked
//! against the library permissions of its author.
use super::*;
use axum::extract::{Path, State};

pub(super) const CLASSES: [&str; 3] = ["film", "series", "music"];
/// Statuses that still count against the number of requests an account may have open.
pub(super) const OPEN_STATUSES: &str =
    "'pending','approved','submitted','acquiring','imported','partial'";

pub(super) fn class_of(kind: &str) -> &'static str {
    match kind {
        "film" => "film",
        "series" => "series",
        _ => "music",
    }
}
fn service_of(class: &str) -> &'static str {
    match class {
        "film" => "radarr",
        "series" => "sonarr",
        _ => "lidarr",
    }
}

/// `limit` requests per sliding window of `days` days; a `limit` of 0 means unlimited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quota {
    pub limit: i64,
    pub days: i64,
}
/// One media class. `None` inherits from the next level.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ClassRule {
    pub allow: Option<bool>,
    pub auto_approve: Option<bool>,
    pub quota: Option<Quota>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Rule {
    pub film: ClassRule,
    pub series: ClassRule,
    pub music: ClassRule,
}
impl Rule {
    fn class(&self, class: &str) -> &ClassRule {
        match class {
            "film" => &self.film,
            "series" => &self.series,
            _ => &self.music,
        }
    }
    fn is_empty(&self) -> bool {
        *self == Rule::default()
    }
    fn valid(&self) -> bool {
        CLASSES.iter().all(|class| {
            self.class(class)
                .quota
                .is_none_or(|q| (0..=10_000).contains(&q.limit) && (1..=365).contains(&q.days))
        })
    }
}

/// What one account may do for one media class once every level is combined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Grant {
    pub allow: bool,
    pub auto_approve: bool,
    pub quota: Option<Quota>,
}

/// Several groups combine additively: an explicit "yes" wins over an explicit "no".
fn flag(
    own: Option<bool>,
    groups: impl Iterator<Item = Option<bool>>,
    default: Option<bool>,
) -> Option<bool> {
    own.or_else(|| {
        let set: Vec<bool> = groups.flatten().collect();
        (!set.is_empty()).then(|| set.contains(&true))
    })
    .or(default)
}
/// The most generous of two quotas (unlimited beats any limit).
fn generous(a: Quota, b: Quota) -> Quota {
    if a.limit == 0 {
        return a;
    }
    if b.limit == 0 {
        return b;
    }
    if i128::from(a.limit) * i128::from(b.days) >= i128::from(b.limit) * i128::from(a.days) {
        a
    } else {
        b
    }
}
pub(super) fn resolve(
    class: &str,
    own: &Rule,
    groups: &[Rule],
    default: &Rule,
    global_auto: bool,
) -> Grant {
    let pick = |field: fn(&ClassRule) -> Option<bool>| {
        flag(
            field(own.class(class)),
            groups.iter().map(|group| field(group.class(class))),
            field(default.class(class)),
        )
    };
    let quota = own
        .class(class)
        .quota
        .or_else(|| {
            groups
                .iter()
                .filter_map(|group| group.class(class).quota)
                .reduce(generous)
        })
        .or(default.class(class).quota)
        .filter(|quota| quota.limit > 0);
    Grant {
        allow: pick(|rule| rule.allow).unwrap_or(true),
        auto_approve: pick(|rule| rule.auto_approve).unwrap_or(global_auto),
        quota,
    }
}

#[derive(Clone, Debug)]
pub(super) struct Effective {
    pub administrator: bool,
    /// The account behind the user: its owner when the user is a profile.
    pub account: String,
    pub max_pending: i64,
    grants: [Grant; 3],
}
impl Effective {
    pub fn grant(&self, kind: &str) -> Grant {
        self.grants[CLASSES
            .iter()
            .position(|class| *class == class_of(kind))
            .unwrap_or(0)]
    }
}

fn parse(json: &str) -> Result<Rule, Error> {
    // A rule Core cannot read is never guessed: fail closed instead of widening access.
    serde_json::from_str(json)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_policy_state"))
}
/// The account a user belongs to: its owner, or the user itself.
pub(super) async fn account(store: &IdentityStore, user: &str) -> Result<String, Error> {
    let owner: Option<String> =
        query_scalar("SELECT owner_id FROM account_profiles WHERE user_id=?")
            .bind(user)
            .fetch_optional(store.pool())
            .await
            .map_err(db)?;
    Ok(owner.unwrap_or_else(|| user.to_owned()))
}
async fn settings(store: &IdentityStore) -> Result<(bool, i64), Error> {
    let (auto, max): (i64, i64) = query_as(
        "SELECT auto_approve,max_pending_per_user FROM acquisition_settings WHERE singleton=1",
    )
    .fetch_one(store.pool())
    .await
    .map_err(db)?;
    Ok((auto == 1, max))
}
async fn default_rule(store: &IdentityStore) -> Result<Rule, Error> {
    let json: String =
        query_scalar("SELECT policy_json FROM request_default_policy WHERE singleton=1")
            .fetch_one(store.pool())
            .await
            .map_err(db)?;
    parse(&json)
}
pub(super) async fn effective(
    store: &IdentityStore,
    user: &str,
    administrator: bool,
) -> Result<Effective, Error> {
    let account = account(store, user).await?;
    let (auto, max_pending) = settings(store).await?;
    if administrator {
        return Ok(Effective {
            administrator,
            account,
            max_pending,
            grants: [Grant {
                allow: true,
                auto_approve: true,
                quota: None,
            }; 3],
        });
    }
    let own: Option<String> =
        query_scalar("SELECT policy_json FROM request_user_policies WHERE user_id=?")
            .bind(&account)
            .fetch_optional(store.pool())
            .await
            .map_err(db)?;
    let own = own
        .map(|json| parse(&json))
        .transpose()?
        .unwrap_or_default();
    let stored: Vec<String> = query_scalar("SELECT policy_json FROM request_group_policies WHERE group_id IN (SELECT group_id FROM effective_user_group_members WHERE user_id=?) OR group_id IN (SELECT id FROM user_groups WHERE is_default=1)")
        .bind(&account)
        .fetch_all(store.pool())
        .await
        .map_err(db)?;
    let groups = stored
        .iter()
        .map(|json| parse(json))
        .collect::<Result<Vec<_>, _>>()?;
    let default = default_rule(store).await?;
    let grants = CLASSES.map(|class| resolve(class, &own, &groups, &default, auto));
    Ok(Effective {
        administrator,
        account,
        max_pending,
        grants,
    })
}

/// Requests of one account (and its profiles) in a media class during the quota window.
/// Declined, cancelled and failed requests do not consume the quota.
fn usage_sql(class: &str) -> String {
    let kinds = match class {
        "film" => "'film'",
        "series" => "'series'",
        _ => "'artist','album','track'",
    };
    format!(
        "SELECT COUNT(*),MIN(created_at) FROM media_requests WHERE (user_id=? OR user_id IN (SELECT user_id FROM account_profiles WHERE owner_id=?)) AND kind IN ({kinds}) AND created_at>=? AND imported=0 AND status NOT IN ('rejected','cancelled','failed')"
    )
}
async fn usage(
    store: &IdentityStore,
    account: &str,
    class: &str,
    quota: Quota,
) -> Result<(i64, Option<i64>), Error> {
    query_as(sqlx::AssertSqlSafe(usage_sql(class)))
        .bind(account)
        .bind(account)
        .bind(now() - quota.days * 86_400)
        .fetch_one(store.pool())
        .await
        .map_err(db)
}
/// Re-checked inside the write transaction so two requests cannot both take the last slot.
pub(super) async fn enforce_quota(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    effective: &Effective,
    kind: &str,
) -> Result<(), Error> {
    let Some(quota) = effective.grant(kind).quota else {
        return Ok(());
    };
    let (used, _): (i64, Option<i64>) = query_as(sqlx::AssertSqlSafe(usage_sql(class_of(kind))))
        .bind(&effective.account)
        .bind(&effective.account)
        .bind(now() - quota.days * 86_400)
        .fetch_one(&mut **tx)
        .await
        .map_err(db)?;
    if used >= quota.limit {
        return Err(err(
            StatusCode::TOO_MANY_REQUESTS,
            "request_period_quota_exceeded",
        ));
    }
    Ok(())
}
/// The status a new request starts in, and the audit code recording why.
pub(super) fn admission(effective: &Effective, kind: &str) -> (&'static str, Option<&'static str>) {
    if effective.administrator {
        ("approved", None)
    } else if effective.grant(kind).auto_approve {
        ("approved", Some("auto_approved"))
    } else {
        ("pending", None)
    }
}

/// Rights as a person sees them, with what is left of each quota.
async fn describe(
    store: &IdentityStore,
    user: &str,
    effective: &Effective,
) -> Result<Value, Error> {
    let pending: i64 = query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM media_requests WHERE user_id=? AND status IN ({OPEN_STATUSES})"
    )))
    .bind(user)
    .fetch_one(store.pool())
    .await
    .map_err(db)?;
    let mut classes = serde_json::Map::new();
    for class in CLASSES {
        let grant = effective.grant(class);
        let quota = match grant.quota {
            Some(quota) => {
                let (used, oldest) = usage(store, &effective.account, class, quota).await?;
                // The oldest request leaves the window first and frees one slot.
                let resets_at = if used >= quota.limit {
                    oldest.map(|at| at + quota.days * 86_400)
                } else {
                    None
                };
                let remaining = (quota.limit - used).max(0);
                json!({"limit":quota.limit,"days":quota.days,"used":used,"remaining":remaining,"resetsAt":resets_at})
            }
            None => Value::Null,
        };
        classes.insert(
            class.to_owned(),
            json!({"allow":grant.allow,"autoApprove":grant.auto_approve,"quota":quota}),
        );
    }
    Ok(
        json!({"administrator":effective.administrator,"pending":pending,"maxPending":effective.max_pending,"classes":classes}),
    )
}
/// Whether this person may reach at least one enabled destination for a media class.
async fn has_destination(
    auth: &AuthState,
    actor: &Authenticated,
    class: &str,
) -> Result<bool, Error> {
    let libraries: Vec<String> = query_scalar("SELECT library_id FROM acquisition_servers WHERE kind=? AND enabled=1 ORDER BY is_default DESC,name,id LIMIT 100")
        .bind(service_of(class))
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?;
    for library in libraries {
        if authorize(auth, actor, &library).await.is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `GET /api/v1/requests/policy`: what the caller may request and how much is left.
pub(super) async fn mine(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = crate::applications::require(&auth, &headers, "requests", false).await?;
    let administrator = actor.user.role == Role::Administrator;
    let effective = effective(&auth.store, &actor.user.id, administrator).await?;
    let mut value = describe(&auth.store, &actor.user.id, &effective).await?;
    // External results carry no trusted classification: restricted accounts cannot request.
    let restricted = unrestricted(&auth.store, &actor.user.id).await.is_err();
    value["restricted"] = json!(restricted);
    for class in CLASSES {
        let reachable = !restricted && has_destination(&auth, &actor, class).await?;
        value["classes"][class]["destination"] = json!(reachable);
    }
    Ok(Json(value))
}

pub(super) fn routes() -> axum::Router<AuthState> {
    use axum::routing::get;
    axum::Router::new()
        .route("/api/v1/requests/policy", get(mine))
        .route("/api/v1/admin/acquisition/policies", get(overview))
        .route(
            "/api/v1/admin/acquisition/policies/default",
            axum::routing::put(save_default),
        )
        .route(
            "/api/v1/admin/acquisition/policies/groups/{id}",
            axum::routing::put(save_group).delete(delete_group),
        )
        .route(
            "/api/v1/admin/acquisition/policies/users/{id}",
            axum::routing::put(save_user).delete(delete_user),
        )
        .route(
            "/api/v1/admin/acquisition/policies/users/{id}/effective",
            get(user_effective),
        )
}

fn system_role(id: &str) -> Option<&'static str> {
    match id {
        "builtin-role-user" => Some("user"),
        "builtin-role-administrator" => Some("administrator"),
        _ => None,
    }
}
async fn overview(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let (auto, max) = settings(&auth.store).await?;
    let default = default_rule(&auth.store).await?;
    // Administrators bypass every rule, so their built-in group is not offered.
    let groups: Vec<(String, String, i64, Option<String>)> = query_as("SELECT g.id,g.name,g.is_default,p.policy_json FROM user_groups g LEFT JOIN request_group_policies p ON p.group_id=g.id WHERE g.id<>'builtin-role-administrator' ORDER BY g.name,g.id LIMIT 500")
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?;
    let mut listed = vec![];
    for (id, name, is_default, json) in groups {
        let rule = json.map(|json| parse(&json)).transpose()?;
        listed.push(json!({"id":id,"name":name,"isDefault":is_default==1,"systemRole":system_role(&id),"rule":rule}));
    }
    let users: Vec<(String, String)> = query_as("SELECT user_id,policy_json FROM request_user_policies ORDER BY updated_at DESC,user_id LIMIT 200")
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?;
    let labels =
        crate::applications::user_labels(&auth.store, users.iter().map(|(id, _)| id.as_str()))
            .await?;
    let mut overridden = vec![];
    for (id, json) in &users {
        let rule = parse(json)?;
        overridden.push(json!({"id":id,"label":labels.get(id),"rule":rule}));
    }
    Ok(Json(
        json!({"default":default,"autoApprove":auto,"maxPending":max,"groups":listed,"users":overridden}),
    ))
}
fn checked(rule: &Rule) -> Result<(), Error> {
    if rule.valid() {
        Ok(())
    } else {
        Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_policy",
        ))
    }
}
async fn save_default(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(rule): Json<Rule>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    checked(&rule)?;
    query("UPDATE request_default_policy SET policy_json=? WHERE singleton=1")
        .bind(json!(rule).to_string())
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(Json(json!(rule)))
}
async fn save_group(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(rule): Json<Rule>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    checked(&rule)?;
    let known: i64 = query_scalar(
        "SELECT COUNT(*) FROM user_groups WHERE id=? AND id<>'builtin-role-administrator'",
    )
    .bind(&id)
    .fetch_one(auth.store.pool())
    .await
    .map_err(db)?;
    if known != 1 {
        return Err(err(StatusCode::NOT_FOUND, "group_not_found"));
    }
    store_rule(
        &auth.store,
        "request_group_policies",
        "group_id",
        &id,
        &rule,
    )
    .await?;
    Ok(Json(json!(rule)))
}
async fn save_user(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(rule): Json<Rule>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    checked(&rule)?;
    known_account(&auth.store, &id).await?;
    store_rule(&auth.store, "request_user_policies", "user_id", &id, &rule).await?;
    Ok(Json(json!(rule)))
}
/// Rules belong to accounts; a profile always follows its owner.
async fn known_account(store: &IdentityStore, id: &str) -> Result<(), Error> {
    let count: i64 = query_scalar("SELECT COUNT(*) FROM users WHERE id=?")
        .bind(id)
        .fetch_one(store.pool())
        .await
        .map_err(db)?;
    if count != 1 {
        return Err(err(StatusCode::NOT_FOUND, "user_not_found"));
    }
    if account(store, id).await? != id {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "policy_target_is_profile",
        ));
    }
    Ok(())
}
/// An empty rule is the same as no rule: the row is removed.
async fn store_rule(
    store: &IdentityStore,
    table: &str,
    column: &str,
    id: &str,
    rule: &Rule,
) -> Result<(), Error> {
    let (table, column) = match (table, column) {
        ("request_group_policies", "group_id") => ("request_group_policies", "group_id"),
        ("request_user_policies", "user_id") => ("request_user_policies", "user_id"),
        _ => {
            return Err(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_policy_state",
            ));
        }
    };
    if rule.is_empty() {
        query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE {column}=?"
        )))
        .bind(id)
        .execute(store.pool())
        .await
        .map_err(db)?;
        return Ok(());
    }
    query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {table}({column},policy_json,updated_at) VALUES(?,?,?) ON CONFLICT({column}) DO UPDATE SET policy_json=excluded.policy_json,updated_at=excluded.updated_at"
    )))
    .bind(id)
    .bind(json!(rule).to_string())
    .bind(now())
    .execute(store.pool())
    .await
    .map_err(db)?;
    Ok(())
}
async fn delete_group(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("DELETE FROM request_group_policies WHERE group_id=?")
        .bind(id)
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn delete_user(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("DELETE FROM request_user_policies WHERE user_id=?")
        .bind(id)
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
/// What an account ends up with once its groups and the server default are applied.
async fn user_effective(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let role: Option<String> = query_scalar("SELECT role FROM users WHERE id=?")
        .bind(&id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
    let role = role.ok_or_else(|| err(StatusCode::NOT_FOUND, "user_not_found"))?;
    let effective = effective(&auth.store, &id, role == "administrator").await?;
    let own: Option<String> =
        query_scalar("SELECT policy_json FROM request_user_policies WHERE user_id=?")
            .bind(&effective.account)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let own = own.map(|json| parse(&json)).transpose()?;
    let described = describe(&auth.store, &id, &effective).await?;
    Ok(Json(json!({"rule":own,"effective":described})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(film: ClassRule) -> Rule {
        Rule {
            film,
            ..Rule::default()
        }
    }
    fn allow(value: bool) -> ClassRule {
        ClassRule {
            allow: Some(value),
            ..ClassRule::default()
        }
    }
    fn auto(value: bool) -> ClassRule {
        ClassRule {
            auto_approve: Some(value),
            ..ClassRule::default()
        }
    }
    fn quota(limit: i64, days: i64) -> ClassRule {
        ClassRule {
            quota: Some(Quota { limit, days }),
            ..ClassRule::default()
        }
    }

    #[test]
    fn built_in_behaviour_follows_the_global_switch() {
        let none = Rule::default();
        let off = resolve("film", &none, &[], &none, false);
        assert!(off.allow && !off.auto_approve && off.quota.is_none());
        assert!(resolve("series", &none, &[], &none, true).auto_approve);
    }
    #[test]
    fn levels_apply_in_order_account_groups_default() {
        let deny = rule(allow(false));
        let permit = rule(allow(true));
        let none = Rule::default();
        // Default denies, a group re-allows, the account denies again.
        assert!(!resolve("film", &none, &[], &deny, true).allow);
        assert!(resolve("film", &none, std::slice::from_ref(&permit), &deny, true).allow);
        assert!(!resolve("film", &deny, std::slice::from_ref(&permit), &permit, true).allow);
        // Another class is untouched by a film rule.
        assert!(resolve("series", &deny, &[], &deny, true).allow);
    }
    #[test]
    fn groups_combine_additively_and_an_explicit_yes_wins() {
        let none = Rule::default();
        let groups = [rule(auto(false)), rule(auto(true))];
        assert!(resolve("film", &none, &groups, &none, false).auto_approve);
        let groups = [rule(auto(false)), Rule::default()];
        assert!(!resolve("film", &none, &groups, &rule(auto(true)), true).auto_approve);
    }
    #[test]
    fn quota_takes_the_most_generous_group_and_zero_means_unlimited() {
        let none = Rule::default();
        let groups = [rule(quota(2, 7)), rule(quota(10, 7))];
        assert_eq!(
            resolve("film", &none, &groups, &none, false).quota,
            Some(Quota { limit: 10, days: 7 })
        );
        // 3 per day is more generous than 10 per 30 days.
        let groups = [rule(quota(10, 30)), rule(quota(3, 1))];
        assert_eq!(
            resolve("film", &none, &groups, &none, false).quota,
            Some(Quota { limit: 3, days: 1 })
        );
        let groups = [rule(quota(10, 7)), rule(quota(0, 1))];
        assert_eq!(resolve("film", &none, &groups, &none, false).quota, None);
        // The account's own limit replaces the groups', even a stricter one.
        assert_eq!(
            resolve("film", &rule(quota(1, 7)), &groups, &none, false).quota,
            Some(Quota { limit: 1, days: 7 })
        );
        assert_eq!(
            resolve(
                "film",
                &rule(quota(0, 7)),
                &groups,
                &rule(quota(1, 1)),
                false
            )
            .quota,
            None
        );
    }
    #[test]
    fn invalid_quotas_are_rejected_and_unknown_fields_refused() {
        assert!(rule(quota(5, 30)).valid());
        assert!(!rule(quota(-1, 30)).valid());
        assert!(!rule(quota(5, 0)).valid());
        assert!(!rule(quota(5, 366)).valid());
        assert!(!rule(quota(10_001, 7)).valid());
        assert!(
            serde_json::from_value::<Rule>(json!({"film":{"allow":true,"admin":true}})).is_err()
        );
        assert!(serde_json::from_value::<Rule>(json!({"games":{}})).is_err());
        assert!(
            serde_json::from_value::<Rule>(json!({}))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn kinds_map_to_classes() {
        assert_eq!(class_of("film"), "film");
        assert_eq!(class_of("series"), "series");
        for kind in ["artist", "album", "track"] {
            assert_eq!(class_of(kind), "music");
        }
    }
}

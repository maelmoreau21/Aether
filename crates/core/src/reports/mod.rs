//! Resource-scoped reports: structured observations, never automatic repairs.
use crate::{applications::{self,Error,db,err},auth::{AuthState,Authenticated},database::{query,query_as,query_scalar},identity::{IdentityStore,Role}};
use axum::{Json,Router,extract::{DefaultBodyLimit,Path,Query,State},http::{HeaderMap,StatusCode},routing::{get,post}};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use uuid::Uuid;
pub fn now()->i64{media_contracts::protocol::unix_now()}
#[derive(Clone,Default,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Context {
    pub engine:Option<String>,pub error_code:Option<String>,pub audio_track_id:Option<String>,pub subtitle_track_id:Option<String>,
}
#[derive(Clone,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Create {
    pub category:String,pub description:String,pub work_id:Option<String>,pub track_id:Option<String>,pub asset_id:Option<String>,
    pub position_seconds:Option<f64>,pub end_seconds:Option<f64>,#[serde(default)]pub context:Context,
}
#[derive(sqlx::FromRow,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Record {
    pub id:String,pub user_id:Option<String>,pub library_id:Option<String>,pub work_id:Option<String>,pub track_id:Option<String>,pub asset_id:Option<String>,
    pub category:String,pub description:String,pub status:String,pub position_seconds:Option<f64>,pub end_seconds:Option<f64>,
    #[serde(skip_serializing)]pub context_json:String,pub created_at:i64,pub updated_at:i64,pub source_key:Option<String>,pub author_label:Option<String>,
}
impl Record {fn value(&self)->Value {let mut value=serde_json::to_value(self).unwrap_or_else(|_|json!({}));value["context"]=serde_json::from_str(&self.context_json).unwrap_or_else(|_|json!({}));value}}
pub fn router(auth:AuthState)->Router {
    Router::new().route("/api/v1/reports",get(list).post(create)).route("/api/v1/reports/{id}",get(detail))
        .route("/api/v1/reports/{id}/comments",post(comment)).route("/api/v1/reports/{id}/status",post(status))
        .layer(DefaultBodyLimit::max(32768))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(axum::http::header::CACHE_CONTROL,axum::http::HeaderValue::from_static("private, no-store")))
        .with_state(auth)
}
fn valid_id(value:&str)->bool{!value.is_empty()&&value.len()<=256&&!value.chars().any(char::is_control)}
async fn music_allowed(store:&IdentityStore,user:&str)->Result<(),Error>{
    let denied:i64=query_scalar("SELECT COUNT(*) FROM parental_policies WHERE user_id=? AND allow_unrated=0").bind(user).fetch_one(store.pool()).await.map_err(db)?;
    if denied!=0{return Err(err(StatusCode::FORBIDDEN,"parental_restriction"));}Ok(())
}
fn valid(input:&Create)->bool {
    matches!(input.category.as_str(),"video"|"audio"|"subtitles"|"metadata"|"missing"|"other")
    && (3..=8000).contains(&input.description.trim().len())
    && [&input.work_id,&input.track_id,&input.asset_id,&input.context.audio_track_id,&input.context.subtitle_track_id].iter().all(|v|v.as_ref().is_none_or(|s|valid_id(s)))
    && [&input.context.engine,&input.context.error_code].iter().all(|v|v.as_ref().is_none_or(|s|s.len()<=128&&s.bytes().all(|c|c.is_ascii_alphanumeric()||b"_.:+ -".contains(&c))))
    && [input.position_seconds,input.end_seconds].iter().all(|v|v.is_none_or(|n|n.is_finite()&&(0.0..=604800.0).contains(&n)))
    && input.end_seconds.is_none_or(|end|input.position_seconds.is_some_and(|start|end>=start))
}
async fn resource(store:&IdentityStore,actor:&Authenticated,input:&Create)->Result<String,Error>{
    let mut library:Option<String>=None;
    for (kind,id) in [("work",&input.work_id),("track",&input.track_id),("asset",&input.asset_id)] {
        if let Some(id)=id {
            if kind=="track" {music_allowed(store,&actor.user.id).await?;}
            let found=if kind=="track" {query_scalar::<_,String>("SELECT library_id FROM music_tracks WHERE id=?").bind(id).fetch_optional(store.pool()).await.map_err(db)?}
            else {crate::inventory::resource_library(store,kind,id).await.map_err(db)?};
            let found=found.ok_or_else(||err(StatusCode::NOT_FOUND,"unknown_resource"))?;
            if library.as_ref().is_some_and(|previous|previous!=&found){return Err(err(StatusCode::BAD_REQUEST,"resource_mismatch"));}
            applications::authorize_library(store,actor,"reports",&found).await?;
            if kind!="track" && !crate::product::target_allowed(store,&actor.user.id,kind,id).await.map_err(|s|err(s,"parental_policy_unavailable"))? {return Err(err(StatusCode::FORBIDDEN,"parental_restriction"));}
            library=Some(found);
        }
    }
    if input.track_id.is_some() && (input.work_id.is_some()||input.asset_id.is_some()){return Err(err(StatusCode::BAD_REQUEST,"resource_mismatch"));}
    if let (Some(asset),Some(work))=(&input.asset_id,&input.work_id){
        let matches:i64=query_scalar("SELECT COUNT(*) FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id WHERE a.id=? AND e.work_id=?").bind(asset).bind(work).fetch_one(store.pool()).await.map_err(db)?;
        if matches!=1{return Err(err(StatusCode::BAD_REQUEST,"resource_mismatch"));}
    }
    for track in [&input.context.audio_track_id,&input.context.subtitle_track_id].into_iter().flatten(){
        let Some(asset)=&input.asset_id else {return Err(err(StatusCode::BAD_REQUEST,"asset_required"));};
        let count:i64=query_scalar("SELECT COUNT(*) FROM inventory_tracks WHERE id=? AND asset_id=?").bind(track).bind(asset).fetch_one(store.pool()).await.map_err(db)?;
        if count!=1{return Err(err(StatusCode::BAD_REQUEST,"track_mismatch"));}
    }
    library.ok_or_else(||err(StatusCode::BAD_REQUEST,"resource_required"))
}
async fn read(store:&IdentityStore,actor:&Authenticated,id:&str)->Result<Record,Error>{
    let record:Record=query_as("SELECT * FROM media_reports WHERE id=?").bind(id).fetch_optional(store.pool()).await.map_err(db)?.ok_or_else(||err(StatusCode::NOT_FOUND,"unknown_report"))?;
    if actor.user.role!=Role::Administrator && record.user_id.as_deref()!=Some(&actor.user.id){return Err(err(StatusCode::NOT_FOUND,"unknown_report"));}
    if let Some(library)=&record.library_id{
        applications::authorize_library(store,actor,"reports",library).await?;
        if record.track_id.is_some(){music_allowed(store,&actor.user.id).await?;}
        let target=record.work_id.as_ref().map(|id|("work",id)).or_else(||record.asset_id.as_ref().map(|id|("asset",id)));
        if let Some((kind,id))=target && !crate::product::target_allowed(store,&actor.user.id,kind,id).await.map_err(|s|err(s,"parental_policy_unavailable"))? {return Err(err(StatusCode::FORBIDDEN,"parental_restriction"));}
    }
    Ok(record)
}
#[derive(Default,Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {#[serde(default)]offset:u32}
async fn list(State(auth):State<AuthState>,headers:HeaderMap,Query(page):Query<Page>)->Result<Json<Value>,Error>{
    let actor=applications::require(&auth,&headers,"reports",false).await?;
    if page.offset>1_000_000{return Err(err(StatusCode::BAD_REQUEST,"invalid_page"));}
    let rows:Vec<Record>=query_as("SELECT * FROM media_reports WHERE (?='administrator' OR user_id=?) ORDER BY updated_at DESC,id LIMIT 100 OFFSET ?")
        .bind(actor.user.role.as_str()).bind(&actor.user.id).bind(i64::from(page.offset)).fetch_all(auth.store.pool()).await.map_err(db)?;
    let mut items=Vec::new();
    for row in rows {match read(&auth.store,&actor,&row.id).await {Ok(v)=>items.push(v.value()),Err((StatusCode::FORBIDDEN|StatusCode::NOT_FOUND,_))=>{},Err(e)=>return Err(e)}}
    Ok(Json(json!({"reports":items})))
}
async fn create(State(auth):State<AuthState>,headers:HeaderMap,Json(input):Json<Create>)->Result<(StatusCode,Json<Value>),Error>{
    let actor=applications::require(&auth,&headers,"reports",true).await?;
    if !valid(&input){return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_report"));}
    let library=resource(&auth.store,&actor,&input).await?;
    let result=applications::invoke(&auth.store,"reports","reports.validate",serde_json::to_value(&input).map_err(|_|err(StatusCode::BAD_REQUEST,"invalid_report"))?).await?;
    if result["valid"]!=true{return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_report"));}
    let id=Uuid::new_v4().to_string();let at=now();
    let mut conn=auth.store.pool().acquire().await.map_err(db)?;
    let mut tx=crate::database::begin_write(&mut conn).await.map_err(db)?;
    let count:i64=query_scalar("SELECT COUNT(*) FROM media_reports WHERE user_id=? AND status IN ('open','in_progress')").bind(&actor.user.id).fetch_one(&mut *tx).await.map_err(db)?;
    if count>=100{return Err(err(StatusCode::TOO_MANY_REQUESTS,"report_limit_reached"));}
    query("INSERT INTO media_reports(id,user_id,library_id,work_id,track_id,asset_id,category,description,status,position_seconds,end_seconds,context_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,'open',?,?,?,?,?)")
        .bind(&id).bind(&actor.user.id).bind(&library).bind(&input.work_id).bind(&input.track_id).bind(&input.asset_id).bind(&input.category).bind(input.description.trim())
        .bind(input.position_seconds).bind(input.end_seconds).bind(serde_json::to_string(&input.context).map_err(|_|err(StatusCode::BAD_REQUEST,"invalid_report"))?).bind(at).bind(at).execute(&mut *tx).await.map_err(db)?;
    query("INSERT INTO media_report_events(id,report_id,user_id,kind,status,created_at) VALUES(?,?,?,'report.created','open',?)").bind(Uuid::new_v4().to_string()).bind(&id).bind(&actor.user.id).bind(at).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok((StatusCode::CREATED,Json(read(&auth.store,&actor,&id).await?.value())))
}
async fn detail(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>)->Result<Json<Value>,Error>{
    let actor=applications::require(&auth,&headers,"reports",false).await?;
    let mut value=read(&auth.store,&actor,&id).await?.value();
    let comments:Vec<(String,Option<String>,Option<String>,String,i64)>=query_as("SELECT id,user_id,author_label,message,created_at FROM media_report_comments WHERE report_id=? ORDER BY created_at,id LIMIT 500").bind(&id).fetch_all(auth.store.pool()).await.map_err(db)?;
    value["comments"]=json!(comments.into_iter().map(|(id,user,author,message,at)|json!({"id":id,"userId":user,"authorLabel":author,"message":message,"createdAt":at})).collect::<Vec<_>>());
    let history:Vec<(String,String,i64)>=query_as("SELECT kind,status,created_at FROM media_report_events WHERE report_id=? ORDER BY created_at,id LIMIT 500").bind(&id).fetch_all(auth.store.pool()).await.map_err(db)?;
    value["history"]=json!(history.into_iter().map(|(kind,status,at)|json!({"kind":kind,"status":status,"createdAt":at})).collect::<Vec<_>>());
    Ok(Json(value))
}
#[derive(Deserialize)]#[serde(deny_unknown_fields)]struct Comment {message:String}
async fn comment(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>,Json(input):Json<Comment>)->Result<StatusCode,Error>{
    let actor=applications::require(&auth,&headers,"reports",true).await?;let record=read(&auth.store,&actor,&id).await?;
    if !(1..=8000).contains(&input.message.trim().len()){return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_comment"));}
    let mut conn=auth.store.pool().acquire().await.map_err(db)?;let mut tx=crate::database::begin_write(&mut conn).await.map_err(db)?;
    let count:i64=query_scalar("SELECT COUNT(*) FROM media_report_comments WHERE report_id=?").bind(&id).fetch_one(&mut *tx).await.map_err(db)?;
    if count>=500{return Err(err(StatusCode::TOO_MANY_REQUESTS,"comment_limit_reached"));}
    query("INSERT INTO media_report_comments(id,report_id,user_id,message,created_at) VALUES(?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind(&id).bind(&actor.user.id).bind(input.message.trim()).bind(now()).execute(&mut *tx).await.map_err(db)?;
    query("UPDATE media_reports SET updated_at=? WHERE id=?").bind(now()).bind(&id).execute(&mut *tx).await.map_err(db)?;
    query("INSERT INTO media_report_events(id,report_id,user_id,kind,status,created_at) VALUES(?,?,?,'report.comment',?,?)").bind(Uuid::new_v4().to_string()).bind(&id).bind(&actor.user.id).bind(&record.status).bind(now()).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]#[serde(deny_unknown_fields)]struct Status {status:String}
async fn status(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>,Json(input):Json<Status>)->Result<StatusCode,Error>{
    let actor=applications::require(&auth,&headers,"reports",true).await?;let record=read(&auth.store,&actor,&id).await?;
    if !matches!(input.status.as_str(),"open"|"in_progress"|"resolved"|"closed"){return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_status"));}
    if actor.user.role!=Role::Administrator && !(input.status=="open"&&matches!(record.status.as_str(),"closed"|"resolved")){return Err(err(StatusCode::FORBIDDEN,"administrator_required"));}
    let mut conn=auth.store.pool().acquire().await.map_err(db)?;let mut tx=crate::database::begin_write(&mut conn).await.map_err(db)?;
    let changed=query("UPDATE media_reports SET status=?,updated_at=? WHERE id=? AND status=?").bind(&input.status).bind(now()).bind(&id).bind(&record.status).execute(&mut *tx).await.map_err(db)?.rows_affected();
    if changed!=1{return Err(err(StatusCode::CONFLICT,"report_changed"));}
    query("INSERT INTO media_report_events(id,report_id,user_id,kind,status,created_at) VALUES(?,?,?,'report.status',?,?)").bind(Uuid::new_v4().to_string()).bind(&id).bind(&actor.user.id).bind(&input.status).bind(now()).execute(&mut *tx).await.map_err(db)?;tx.commit().await.map_err(db)?;Ok(StatusCode::NO_CONTENT)
}

/// Import historical facts without invoking notification or repair paths.
/// A source report may remain unlinked when its exact media has no unique local match.
pub async fn import_legacy(
    store:&IdentityStore,
    source_id:&str,
    value:&Value,
    user_id:Option<&str>,
    user_mappings:&std::collections::BTreeMap<String,String>,
)->Result<bool,Error> {
    fn source_identifier(value:&Value)->Option<String> {
        value.as_i64().filter(|v|*v>0).map(|v|v.to_string())
    }
    fn author(value:&Value)->String {
        value["displayName"].as_str().or_else(||value["username"].as_str()).unwrap_or("Auteur historique")
            .chars().filter(|c|!c.is_control()).take(128).collect()
    }
    fn date(value:&Value,default:i64)->i64 {
        value.as_str().and_then(|v|time::OffsetDateTime::parse(v,&time::format_description::well_known::Rfc3339).ok())
            .map(|v|v.unix_timestamp()).or_else(||value.as_i64()).filter(|n|*n>=0).unwrap_or(default)
    }
    let source_report=source_identifier(&value["id"]).ok_or_else(||err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_source_report_id"))?;
    let source_key=format!("{source_id}:report:{source_report}");
    let category=match value["issueType"].as_i64(){Some(1)=>"video",Some(2)=>"audio",Some(3)=>"subtitles",Some(4)=>"other",_=>return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_source_report_category"))};
    let source_status=match value["status"].as_i64(){Some(1)=>"open",Some(2)=>"resolved",_=>return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_source_report_status"))};
    let comments=value["comments"].as_array().filter(|v|v.len()<=1000).ok_or_else(||err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_source_comments"))?;
    let created=date(&value["createdAt"],now());
    let updated=date(&value["updatedAt"],created);
    let mut ordered=comments.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|comment|(date(&comment["createdAt"],created),comment["id"].as_i64().unwrap_or(0)));
    let description=ordered.first().and_then(|v|v["message"].as_str()).unwrap_or("Signalement historique sans description");
    if description.len()>65536 {return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"source_description_too_large"));}
    struct ImportedComment {key:String,message:String,user:Option<String>,author:String,created:i64}
    let mut imported_comments=Vec::new();
    for comment in ordered {
        let id=source_identifier(&comment["id"]).ok_or_else(||err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_source_comment_id"))?;
        let message=comment["message"].as_str().filter(|v|!v.is_empty()&&v.len()<=65536).ok_or_else(||err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_source_comment"))?;
        let mapped=source_identifier(&comment["user"]["id"]).and_then(|id|user_mappings.get(&id));
        let user=match mapped {Some(id)=>query_scalar::<_,String>("SELECT id FROM users WHERE id=? AND disabled_at IS NULL").bind(id).fetch_optional(store.pool()).await.map_err(db)?,None=>None};
        imported_comments.push(ImportedComment {key:format!("{source_key}:comment:{id}"),message:message.into(),user,author:author(&comment["user"]),created:date(&comment["createdAt"],created)});
    }
    let external_id=source_identifier(&value["media"]["tmdbId"]);
    let kind=match value["media"]["mediaType"].as_str(){Some("movie")=>Some("film"),Some("tv")=>Some("series"),_=>None};
    let matches:Vec<(String,String)>=match (external_id.as_deref(),kind) {
        (Some(id),Some(kind))=>query_as("SELECT id,library_id FROM inventory_works WHERE kind=? AND CAST(json_extract(external_ids_json,'$.tmdb') AS TEXT)=? ORDER BY id LIMIT 2")
            .bind(kind).bind(id).fetch_all(store.pool()).await.map_err(db)?,_=>vec![],
    };
    let (work,library)=if matches.len()==1{(Some(matches[0].0.clone()),Some(matches[0].1.clone()))}else{(None,None)};
    // Original season/episode scope is historical context, never a claimed asset association.
    let context=json!({"source":{"reportId":source_report,"tmdbId":external_id,"mediaType":kind,
        "season":value["problemSeason"].as_i64(),"episode":value["problemEpisode"].as_i64()}}).to_string();
    let mut conn=store.pool().acquire().await.map_err(db)?;
    let mut tx=crate::database::begin_write_scoped(&mut conn,"media-report-import",&source_key).await.map_err(db)?;
    let existing:Option<(String,i64)>=query_as("SELECT id,updated_at FROM media_reports WHERE source_key=?").bind(&source_key).fetch_optional(&mut *tx).await.map_err(db)?;
    let inserted=existing.is_none();
    let report_id=existing.as_ref().map(|v|v.0.clone()).unwrap_or_else(||Uuid::new_v4().to_string());
    if inserted {
        query("INSERT INTO media_reports(id,user_id,library_id,work_id,category,description,status,context_json,created_at,updated_at,source_key,author_label) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(&report_id).bind(user_id).bind(&library).bind(&work).bind(category).bind(description).bind(source_status).bind(&context).bind(created).bind(updated)
            .bind(&source_key).bind(author(&value["createdBy"])).execute(&mut *tx).await.map_err(db)?;
        query("INSERT INTO media_report_events(id,report_id,user_id,kind,status,created_at) VALUES(?,?,NULL,'report.imported',?,?)")
            .bind(Uuid::new_v4().to_string()).bind(&report_id).bind(source_status).bind(now()).execute(&mut *tx).await.map_err(db)?;
    }else if existing.as_ref().is_some_and(|v|updated>v.1) {
        // A later source delta may update history; newer native edits keep priority.
        query("UPDATE media_reports SET status=?,description=?,context_json=?,updated_at=? WHERE id=? AND updated_at<?")
            .bind(source_status).bind(description).bind(&context).bind(updated).bind(&report_id).bind(updated).execute(&mut *tx).await.map_err(db)?;
    }
    for comment in imported_comments {
        query("INSERT INTO media_report_comments(id,report_id,user_id,author_label,message,created_at,source_key) VALUES(?,?,?,?,?,?,?) ON CONFLICT(source_key) DO NOTHING")
            .bind(Uuid::new_v4().to_string()).bind(&report_id).bind(comment.user).bind(comment.author).bind(comment.message).bind(comment.created).bind(comment.key).execute(&mut *tx).await.map_err(db)?;
    }
    crate::database::commit(tx).await.map_err(db)?;
    Ok(inserted)
}

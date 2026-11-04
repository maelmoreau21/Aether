use super::*;
use axum::{extract::{DefaultBodyLimit,Path,Query,State},routing::{get,post}};

pub fn router(auth:AuthState)->Router {
    Router::new().route("/api/v1/requests",get(list).post(create))
        .route("/api/v1/requests/search",get(search))
        .route("/api/v1/requests/destinations",get(destinations))
        .route("/api/v1/requests/{id}",get(detail))
        .route("/api/v1/requests/{id}/action",post(action))
        .route("/api/v1/admin/acquisition/settings",get(settings).put(save_settings))
        .route("/api/v1/admin/acquisition/servers",get(servers).post(add_server))
        .route("/api/v1/admin/acquisition/servers/{id}",axum::routing::put(update_server).delete(delete_server))
        .route("/api/v1/admin/acquisition/servers/{id}/test",post(test_server))
        .route("/api/v1/admin/acquisition/servers/{id}/options",get(options))
        .layer(DefaultBodyLimit::max(65536))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(axum::http::header::CACHE_CONTROL,axum::http::HeaderValue::from_static("private, no-store")))
        .with_state(auth)
}
#[derive(Deserialize)] #[serde(rename_all="camelCase",deny_unknown_fields)]
struct SettingsInput {auto_approve:bool,max_pending_per_user:i64}
async fn settings(State(auth):State<AuthState>,headers:HeaderMap)->Result<Json<Value>,Error>{
    admin(&auth,&headers,false).await?;
    let (auto,max):(i64,i64)=query_as("SELECT auto_approve,max_pending_per_user FROM acquisition_settings WHERE singleton=1").fetch_one(auth.store.pool()).await.map_err(db)?;
    Ok(Json(json!({"autoApprove":auto==1,"maxPendingPerUser":max})))
}
async fn save_settings(State(auth):State<AuthState>,headers:HeaderMap,Json(input):Json<SettingsInput>)->Result<Json<Value>,Error>{
    admin(&auth,&headers,true).await?;
    if !(1..=1000).contains(&input.max_pending_per_user){return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_request_quota"));}
    query("UPDATE acquisition_settings SET auto_approve=?,max_pending_per_user=? WHERE singleton=1").bind(i64::from(input.auto_approve)).bind(input.max_pending_per_user).execute(auth.store.pool()).await.map_err(db)?;
    Ok(Json(json!({"autoApprove":input.auto_approve,"maxPendingPerUser":input.max_pending_per_user})))
}
async fn servers(State(auth):State<AuthState>,headers:HeaderMap)->Result<Json<Value>,Error>{
    admin(&auth,&headers,false).await?;
    let values:Vec<Server>=query_as(sqlx::AssertSqlSafe(format!("SELECT {SERVER_COLUMNS} FROM acquisition_servers ORDER BY name,id LIMIT 100"))).fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(json!({"servers":values.iter().map(Server::public).collect::<Vec<_>>()})))
}
async fn destinations(State(auth):State<AuthState>,headers:HeaderMap)->Result<Json<Value>,Error>{
    let actor=crate::applications::require(&auth,&headers,"requests",false).await?;
    let values:Vec<Server>=query_as(sqlx::AssertSqlSafe(format!("SELECT {SERVER_COLUMNS} FROM acquisition_servers WHERE enabled=1 ORDER BY name,id LIMIT 100"))).fetch_all(auth.store.pool()).await.map_err(db)?;
    let mut out=vec![];
    for server in values {if authorize(&auth,&actor,&server.library_id).await.is_ok(){out.push(json!({"id":server.id,"name":server.name,"kind":server.kind,"libraryId":server.library_id,"isDefault":server.is_default==1}));}}
    Ok(Json(json!({"servers":out})))
}
#[derive(Deserialize)] #[serde(rename_all="camelCase",deny_unknown_fields)]
struct ServerInput {
    #[serde(default)]id:Option<String>,name:String,kind:String,base_url:String,#[serde(default)]api_key:Option<String>,
    library_id:String,root_folder:String,quality_profile_id:i64,#[serde(default)]metadata_profile_id:Option<i64>,
    #[serde(default)]is_default:bool,#[serde(default="yes")]enabled:bool,
}
fn yes()->bool{true}
async fn add_server(State(auth):State<AuthState>,headers:HeaderMap,Json(input):Json<ServerInput>)->Result<Json<Value>,Error>{
    admin(&auth,&headers,true).await?;save_server(&auth,None,input).await
}
async fn update_server(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>,Json(input):Json<ServerInput>)->Result<Json<Value>,Error>{
    admin(&auth,&headers,true).await?;save_server(&auth,Some(id),input).await
}
async fn save_server(auth:&AuthState,id:Option<String>,input:ServerInput)->Result<Json<Value>,Error>{
    if input.name.trim().is_empty()||input.name.len()>128||!matches!(input.kind.as_str(),"radarr"|"sonarr"|"lidarr")
        ||(input.enabled&&(input.quality_profile_id<=0||input.root_folder.is_empty()||(input.kind=="lidarr"&&input.metadata_profile_id.is_none())))||input.root_folder.len()>2048||input.root_folder.chars().any(char::is_control)
        ||input.metadata_profile_id.is_some_and(|v|v<=0)||input.id.as_ref().is_some_and(|v|id.as_ref()!=Some(v)){
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_acquisition_server"));
    }
    let library:Option<String>=query_scalar("SELECT l.kind FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND n.status!='revoked'").bind(&input.library_id).fetch_optional(auth.store.pool()).await.map_err(db)?;
    let expected=match input.kind.as_str(){"radarr"=>"films","sonarr"=>"series",_=>"music"};
    if library.as_deref()!=Some(expected){return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"server_library_kind_mismatch"));}
    let previous=match id.as_deref(){Some(id)=>Some(server(&auth.store,id).await?),None=>None};
    let key=match input.api_key.filter(|s|!s.is_empty()){
        Some(key)=>key,None=>previous.as_ref().filter(|old|old.kind==input.kind&&old.base_url.trim_end_matches('/')==input.base_url.trim_end_matches('/')).map(|s|s.api_key.clone()).ok_or_else(||err(StatusCode::UNPROCESSABLE_ENTITY,"api_key_required"))?
    };
    let connection=transport::configure(&input.base_url,&key).await?;
    let id=id.unwrap_or_else(||uuid::Uuid::new_v4().to_string());let generation=uuid::Uuid::new_v4().to_string();
    let mut conn=auth.store.pool().acquire().await.map_err(db)?;let mut tx=crate::database::begin_write_serialized(&mut conn,LOCK).await.map_err(db)?;
    let active:i64=query_scalar("SELECT COUNT(*) FROM media_requests WHERE server_id=? AND status NOT IN ('available','failed','rejected','cancelled','suspended')").bind(&id).fetch_one(&mut *tx).await.map_err(db)?;
    if active>0&&previous.as_ref().is_some_and(|old|old.base_url!=connection.base_url||old.kind!=input.kind||old.library_id!=input.library_id||old.root_folder!=input.root_folder){return Err(err(StatusCode::CONFLICT,"server_has_active_requests"));}
    let count:i64=query_scalar("SELECT COUNT(*) FROM acquisition_servers").fetch_one(&mut *tx).await.map_err(db)?;
    if previous.is_none()&&count>=100{return Err(err(StatusCode::CONFLICT,"server_limit_reached"));}
    if input.is_default{query("UPDATE acquisition_servers SET is_default=0 WHERE kind=? AND id!=?").bind(&input.kind).bind(&id).execute(&mut *tx).await.map_err(db)?;}
    query("INSERT INTO acquisition_servers(id,name,kind,base_url,api_key,allowed_ips_json,library_id,root_folder,quality_profile_id,metadata_profile_id,is_default,enabled,generation) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,kind=excluded.kind,base_url=excluded.base_url,api_key=excluded.api_key,allowed_ips_json=excluded.allowed_ips_json,library_id=excluded.library_id,root_folder=excluded.root_folder,quality_profile_id=excluded.quality_profile_id,metadata_profile_id=excluded.metadata_profile_id,is_default=excluded.is_default,enabled=excluded.enabled,generation=excluded.generation")
        .bind(&id).bind(input.name.trim()).bind(input.kind).bind(connection.base_url).bind(key).bind(json!(connection.allowed_ips).to_string()).bind(input.library_id).bind(input.root_folder).bind(input.quality_profile_id).bind(input.metadata_profile_id).bind(i64::from(input.is_default)).bind(i64::from(input.enabled)).bind(generation).execute(&mut *tx).await.map_err(db)?;
    crate::database::commit(tx).await.map_err(db)?;Ok(Json(json!({"server":server(&auth.store,&id).await?.public()})))
}
async fn delete_server(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>)->Result<StatusCode,Error>{
    admin(&auth,&headers,true).await?;
    let count:i64=query_scalar("SELECT COUNT(*) FROM media_requests WHERE server_id=?").bind(&id).fetch_one(auth.store.pool()).await.map_err(db)?;
    if count>0{return Err(err(StatusCode::CONFLICT,"server_has_request_history_disable_instead"));}
    query("DELETE FROM acquisition_servers WHERE id=?").bind(id).execute(auth.store.pool()).await.map_err(db)?;Ok(StatusCode::NO_CONTENT)
}
async fn test_server(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>)->Result<Json<Value>,Error>{
    admin(&auth,&headers,true).await?;let server=server(&auth.store,&id).await?;
    let status=server.call("GET","system/status",&[],None).await?;
    Ok(Json(json!({"ok":true,"version":status["version"],"appName":status["appName"]})))
}
async fn options(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>)->Result<Json<Value>,Error>{
    admin(&auth,&headers,false).await?;let server=server(&auth.store,&id).await?;
    let roots=server.call("GET","rootfolder",&[],None).await?;let profiles=server.call("GET","qualityprofile",&[],None).await?;
    let metadata=if server.kind=="lidarr"{server.call("GET","metadataprofile",&[],None).await?}else{json!([])};
    Ok(Json(json!({"rootFolders":roots,"qualityProfiles":profiles,"metadataProfiles":metadata})))
}
#[derive(Deserialize)] #[serde(rename_all="camelCase")]
struct Search {q:String,kind:String,server_id:Option<String>}
async fn choose(auth:&AuthState,actor:&Authenticated,kind:&str,id:Option<&str>)->Result<Server,Error>{
    let service=match kind{"film"=>"radarr","series"=>"sonarr","artist"|"album"|"track"=>"lidarr",_=>return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_request_kind"))};
    let server=if let Some(id)=id{server(&auth.store,id).await?}else{
        let sql=format!("SELECT {SERVER_COLUMNS} FROM acquisition_servers WHERE kind=? AND enabled=1 ORDER BY is_default DESC,name,id LIMIT 1");
        query_as(sqlx::AssertSqlSafe(sql)).bind(service).fetch_optional(auth.store.pool()).await.map_err(db)?.ok_or_else(||err(StatusCode::CONFLICT,"acquisition_server_not_configured"))?
    };
    if server.enabled!=1||server.kind!=service{return Err(err(StatusCode::CONFLICT,"acquisition_server_incompatible"));}
    authorize(auth,actor,&server.library_id).await?;Ok(server)
}
async fn search(State(auth):State<AuthState>,headers:HeaderMap,Query(input):Query<Search>)->Result<Json<Value>,Error>{
    let actor=crate::applications::require(&auth,&headers,"requests",false).await?;unrestricted(&auth.store,&actor.user.id).await?;
    if input.q.trim().len()<2||input.q.len()>200{return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_search_query"));}
    let server=choose(&auth,&actor,&input.kind,input.server_id.as_deref()).await?;
    let results=remote::search(&server,&input.kind,input.q.trim()).await?;Ok(Json(json!({"results":results})))
}
async fn create(State(auth):State<AuthState>,headers:HeaderMap,Json(mut input):Json<RequestInput>)->Result<Json<Value>,Error>{
    let actor=crate::applications::require(&auth,&headers,"requests",true).await?;
    unrestricted(&auth.store,&actor.user.id).await?;
    if !input.valid()||(input.kind=="artist"&&input.album_ids.is_empty()&&!input.monitor_future){return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_request_selection"));}
    let server=choose(&auth,&actor,&input.kind,input.server_id.as_deref()).await?;
    let validation=crate::applications::invoke(&auth.store,"requests","requests.validate",json!(input)).await?;
    if validation["valid"]!=true{return Err(err(StatusCode::UNPROCESSABLE_ENTITY,validation["error"].as_str().unwrap_or("invalid_request")));}
    let canonical=remote::lookup(&server,&input).await?;
    input.title=remote::title(&canonical,&input.kind).unwrap_or(&input.title).chars().take(512).collect();
    input.server_id=Some(server.id.clone());
    let mut conn=auth.store.pool().acquire().await.map_err(db)?;let mut tx=crate::database::begin_write_serialized(&mut conn,LOCK).await.map_err(db)?;
    let duplicate:Option<String>=query_scalar("SELECT id FROM media_requests WHERE server_id=? AND kind=? AND external_id=? AND input_json=? AND status NOT IN ('rejected','cancelled','failed') LIMIT 1")
        .bind(&server.id).bind(&input.kind).bind(&input.external_id).bind(json!(input).to_string()).fetch_optional(&mut *tx).await.map_err(db)?;
    if duplicate.is_some(){return Err(err(StatusCode::CONFLICT,"request_already_exists"));}
    let(auto,max):(i64,i64)=query_as("SELECT auto_approve,max_pending_per_user FROM acquisition_settings WHERE singleton=1").fetch_one(&mut *tx).await.map_err(db)?;
    let pending:i64=query_scalar("SELECT COUNT(*) FROM media_requests WHERE user_id=? AND status IN ('pending','approved','submitted','acquiring','imported','partial')").bind(&actor.user.id).fetch_one(&mut *tx).await.map_err(db)?;
    if pending>=max{return Err(err(StatusCode::TOO_MANY_REQUESTS,"request_quota_exceeded"));}
    let status=if auto==1||actor.user.role==Role::Administrator{"approved"}else{"pending"};
    let id=uuid::Uuid::new_v4().to_string();let timestamp=now();
    query("INSERT INTO media_requests(id,user_id,server_id,library_id,kind,title,external_id,external_provider,input_json,status,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&actor.user.id).bind(&server.id).bind(&server.library_id).bind(&input.kind).bind(&input.title).bind(&input.external_id).bind(&input.external_provider).bind(json!(input).to_string()).bind(status).bind(timestamp).bind(timestamp).execute(&mut *tx).await.map_err(db)?;
    event(&mut tx,&id,Some(&actor.user.id),status,None).await?;
    if status=="approved"{query("INSERT INTO acquisition_jobs(request_id,available_at) VALUES(?,?)").bind(&id).bind(timestamp).execute(&mut *tx).await.map_err(db)?;}
    crate::database::commit(tx).await.map_err(db)?;Ok(Json(json!(record(&auth.store,&id).await?)))
}
#[derive(Deserialize,Default)]struct Page{before:Option<String>,limit:Option<i64>}
async fn list(State(auth):State<AuthState>,headers:HeaderMap,Query(page):Query<Page>)->Result<Json<Value>,Error>{
    let actor=crate::applications::require(&auth,&headers,"requests",false).await?;
    let rows:Vec<RequestRecord>=query_as(sqlx::AssertSqlSafe(format!("SELECT {REQUEST_COLUMNS} FROM media_requests WHERE (?='administrator' OR user_id=?) AND (? IS NULL OR id<?) ORDER BY id DESC LIMIT ?")))
        .bind(actor.user.role.as_str()).bind(&actor.user.id).bind(&page.before).bind(&page.before).bind(page.limit.unwrap_or(100).clamp(1,200)).fetch_all(auth.store.pool()).await.map_err(db)?;
    let cursor=rows.last().map(|r|r.id.clone());let mut visible=vec![];
    for row in rows{if authorize(&auth,&actor,&row.library_id).await.is_ok(){visible.push(row);}}
    Ok(Json(json!({"requests":visible,"nextCursor":cursor})))
}
async fn own(auth:&AuthState,actor:&Authenticated,id:&str)->Result<RequestRecord,Error>{
    let row=record(&auth.store,id).await?;if actor.user.role!=Role::Administrator&&row.user_id.as_deref()!=Some(actor.user.id.as_str()){return Err(err(StatusCode::NOT_FOUND,"request_not_found"));}
    authorize(auth,actor,&row.library_id).await?;Ok(row)
}
async fn detail(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>)->Result<Json<Value>,Error>{
    let actor=crate::applications::require(&auth,&headers,"requests",false).await?;let row=own(&auth,&actor,&id).await?;
    let events:Vec<(String,Option<String>,String,Option<String>,i64)>=query_as("SELECT id,actor_id,status,code,created_at FROM acquisition_events WHERE request_id=? ORDER BY created_at,id LIMIT 500").bind(&id).fetch_all(auth.store.pool()).await.map_err(db)?;
    let input:Value=serde_json::from_str(&row.input_json).map_err(|_|err(StatusCode::INTERNAL_SERVER_ERROR,"invalid_request_state"))?;
    let mut value=json!(row);value["selection"]=input;value["events"]=json!(events.iter().map(|(id,actor,status,code,created)|json!({"id":id,"actorId":actor,"status":status,"code":code,"createdAt":created})).collect::<Vec<_>>());Ok(Json(value))
}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]struct Action{action:String,server_id:Option<String>}
async fn action(State(auth):State<AuthState>,headers:HeaderMap,Path(id):Path<String>,Json(input):Json<Action>)->Result<Json<Value>,Error>{
    let actor=crate::applications::require(&auth,&headers,"requests",true).await?;let row=own(&auth,&actor,&id).await?;
    let administrator=actor.user.role==Role::Administrator;
    let status=match input.action.as_str(){"cancel" if matches!(row.status.as_str(),"pending"|"approved")=>"cancelled","approve" if administrator&&row.status=="pending"=>"approved","reject" if administrator&&row.status=="pending"=>"rejected","retry" if administrator&&matches!(row.status.as_str(),"failed"|"suspended")=>"approved",_=>return Err(err(StatusCode::CONFLICT,"request_action_not_allowed"))};
    let mut selection:RequestInput=serde_json::from_str(&row.input_json).map_err(|_|err(StatusCode::INTERNAL_SERVER_ERROR,"invalid_request_state"))?;
    if let Some(server_id)=input.server_id.as_deref(){
        if row.remote_id.is_some(){return Err(err(StatusCode::CONFLICT,"request_already_dispatched"));}
        let destination=choose(&auth,&actor,&selection.kind,Some(server_id)).await?;selection.server_id=Some(destination.id);
    }
    let server=choose(&auth,&actor,&selection.kind,selection.server_id.as_deref()).await?;
    if status=="approved"{let owner=row.user_id.as_deref().ok_or_else(||err(StatusCode::CONFLICT,"historical_author_mapping_required"))?;unrestricted(&auth.store,owner).await?;}
    let mut conn=auth.store.pool().acquire().await.map_err(db)?;let mut tx=crate::database::begin_write_serialized(&mut conn,LOCK).await.map_err(db)?;
    let changed=query("UPDATE media_requests SET status=?,error_code=NULL,server_id=?,library_id=?,input_json=?,updated_at=? WHERE id=? AND status=?").bind(status).bind(&server.id).bind(&server.library_id).bind(json!(selection).to_string()).bind(now()).bind(&id).bind(&row.status).execute(&mut *tx).await.map_err(db)?.rows_affected();
    if changed!=1{return Err(err(StatusCode::CONFLICT,"request_changed"));}
    event(&mut tx,&id,Some(&actor.user.id),status,None).await?;
    if status=="approved"{query("INSERT INTO acquisition_jobs(request_id,available_at) VALUES(?,?) ON CONFLICT(request_id) DO UPDATE SET available_at=excluded.available_at,lease_token=NULL,lease_until=0,attempts=0,phase='initial'").bind(&id).bind(now()).execute(&mut *tx).await.map_err(db)?;}else{query("DELETE FROM acquisition_jobs WHERE request_id=?").bind(&id).execute(&mut *tx).await.map_err(db)?;}
    crate::database::commit(tx).await.map_err(db)?;Ok(Json(json!(record(&auth.store,&id).await?)))
}

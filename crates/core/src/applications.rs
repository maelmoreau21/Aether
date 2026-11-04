//! Native surfaces backed by explicitly enabled, signed application extensions.
//! Guests run in the disposable worker without network, secrets or storage access.
use crate::{auth::{AuthState, Authenticated}, database::{query_as, query_scalar}, identity::IdentityStore};
use axum::{Json, Router, extract::State, http::{HeaderMap, StatusCode}, routing::get};
use media_extension_sdk::{ExtensionRequest, ExtensionResponse, Package};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub type Error = (StatusCode, Json<Value>);
pub fn err(status: StatusCode, code: &str) -> Error { (status, Json(json!({"error":code}))) }
pub fn db(_: sqlx::Error) -> Error { err(StatusCode::INTERNAL_SERVER_ERROR,"database_error") }
fn extension_id(cap: &str) -> Result<&'static str, Error> {
    match cap { "requests" => Ok("media-requests"), "reports" => Ok("media-reports"), _ => Err(err(StatusCode::BAD_REQUEST,"unsupported_capability")) }
}
async fn package(store: &IdentityStore, cap: &str) -> Result<(Package,String), Error> {
    let id=extension_id(cap)?;
    let row:Option<(String,String)>=query_as("SELECT e.package_json,e.generation FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.id=? AND e.enabled=1 AND p.active=1")
        .bind(id).fetch_optional(store.pool()).await.map_err(db)?;
    let (raw,generation)=row.ok_or_else(||err(StatusCode::SERVICE_UNAVAILABLE,"extension_disabled"))?;
    let package:Package=serde_json::from_str(&raw).map_err(|_|err(StatusCode::INTERNAL_SERVER_ERROR,"invalid_package"))?;
    if package.manifest.id!=id || !package.manifest.permissions.iter().any(|p|p==cap) { return Err(err(StatusCode::FORBIDDEN,"extension_permission_denied")); }
    Ok((package,generation))
}
pub async fn enabled(store: &IdentityStore, cap:&str)->Result<bool,Error> {
    match package(store,cap).await { Ok(_)=>Ok(true), Err((StatusCode::SERVICE_UNAVAILABLE|StatusCode::FORBIDDEN,_))=>Ok(false), Err(e)=>Err(e) }
}
pub async fn require(auth:&AuthState,headers:&HeaderMap,cap:&str,mutation:bool)->Result<Authenticated,Error> {
    let actor=auth.authenticate(headers).await.map_err(|s|err(s,"authentication_required"))?;
    if mutation { auth.require_csrf(headers,&actor).map_err(|s|err(s,"csrf_required"))?; }
    package(&auth.store,cap).await?;
    Ok(actor)
}
pub async fn authorize_library(store:&IdentityStore,actor:&Authenticated,cap:&str,library:&str)->Result<(),Error> {
    library_allowed(store,&actor.user.id,actor.user.role.as_str(),cap,library).await
}
pub async fn authorize_background(store:&IdentityStore,cap:&str,library:&str,user_id:&str)->Result<(),Error>{
    let role:Option<String>=query_scalar("SELECT role FROM users WHERE id=? AND disabled_at IS NULL").bind(user_id).fetch_optional(store.pool()).await.map_err(db)?;
    let role=role.ok_or_else(||err(StatusCode::FORBIDDEN,"account_inactive"))?;
    if !crate::product::access_now(store,user_id,media_contracts::protocol::unix_now()).await.map_err(|s|err(s,"parental_policy_unavailable"))? {return Err(err(StatusCode::FORBIDDEN,"parental_restriction"));}
    library_allowed(store,user_id,&role,cap,library).await
}
async fn library_allowed(store:&IdentityStore,user_id:&str,role:&str,cap:&str,library:&str)->Result<(),Error>{
    let id=extension_id(cap)?;
    package(store,cap).await?;
    let count:i64=query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id JOIN extensions e ON e.id=? WHERE l.id=? AND l.enabled=1 AND n.status!='revoked' AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id) OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=l.id AND f.active=1)) AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0))")
        .bind(id).bind(library).bind(role).bind(user_id).fetch_one(store.pool()).await.map_err(db)?;
    if count!=1 {return Err(err(StatusCode::FORBIDDEN,"library_permission_denied"));} Ok(())
}
pub fn router(auth:AuthState)->Router {
    Router::new().route("/api/v1/extensions/features",get(features))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(axum::http::header::CACHE_CONTROL,axum::http::HeaderValue::from_static("private, no-store")))
        .with_state(auth)
}
async fn features(State(auth):State<AuthState>,headers:HeaderMap)->Result<Json<Value>,Error> {
    auth.authenticate(&headers).await.map_err(|s|err(s,"authentication_required"))?;
    Ok(Json(json!({"requests":enabled(&auth.store,"requests").await?,"reports":enabled(&auth.store,"reports").await?})))
}
pub async fn invoke(store:&IdentityStore,cap:&str,operation:&str,input:Value)->Result<Value,Error> {
    if !matches!((cap,operation),("requests","requests.validate"|"requests.plan")|("reports","reports.validate"|"reports.classify")) {return Err(err(StatusCode::BAD_REQUEST,"unsupported_operation"));}
    let (package,generation)=package(store,cap).await?;
    let _permit=crate::extensions::runtime::try_acquire_execution(&package.manifest.id,crate::extensions::runtime::ExecutionClass::Interactive)
        .ok_or_else(||err(StatusCode::CONFLICT,"extension_busy"))?;
    let invocation=crate::extensions::runtime::Invocation::new(crate::extensions::storage::wasm(store,&package).await.map_err(|_|err(StatusCode::SERVICE_UNAVAILABLE,"extension_module_missing"))?,ExtensionRequest{api_version:1,operation:operation.into(),input});
    let payload=serde_json::to_vec(&invocation).map_err(|_|err(StatusCode::BAD_REQUEST,"invalid_input"))?;
    if payload.len()>400_000 {return Err(err(StatusCode::PAYLOAD_TOO_LARGE,"invalid_input"));}
    let exe=std::env::current_exe().map_err(|_|err(StatusCode::SERVICE_UNAVAILABLE,"worker_unavailable"))?;
    let mut directory=exe.parent().ok_or_else(||err(StatusCode::SERVICE_UNAVAILABLE,"worker_unavailable"))?;
    if directory.file_name().is_some_and(|v|v=="deps") {directory=directory.parent().ok_or_else(||err(StatusCode::SERVICE_UNAVAILABLE,"worker_unavailable"))?;}
    let mut command=tokio::process::Command::new(directory.join(if cfg!(windows){"media-extension-worker.exe"}else{"media-extension-worker"}));
    command.env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child=command.spawn().map_err(|_|err(StatusCode::SERVICE_UNAVAILABLE,"worker_unavailable"))?;
    let result=tokio::time::timeout(Duration::from_secs(8),async {
        let mut stdin=child.stdin.take().ok_or_else(||err(StatusCode::INTERNAL_SERVER_ERROR,"worker_io"))?;
        stdin.write_all(&(payload.len() as u32).to_be_bytes()).await.map_err(|_|err(StatusCode::SERVICE_UNAVAILABLE,"worker_io"))?;
        stdin.write_all(&payload).await.map_err(|_|err(StatusCode::SERVICE_UNAVAILABLE,"worker_io"))?;
        drop(stdin);
        let mut stdout=child.stdout.take().ok_or_else(||err(StatusCode::SERVICE_UNAVAILABLE,"worker_io"))?;
        let size=stdout.read_u32().await.map_err(|_|err(StatusCode::UNPROCESSABLE_ENTITY,"extension_failed"))? as usize;
        if size>media_extension_sdk::MAX_JSON+4096 {return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_output"));}
        let mut frame=vec![0;size];
        stdout.read_exact(&mut frame).await.map_err(|_|err(StatusCode::UNPROCESSABLE_ENTITY,"extension_failed"))?;
        let frame:Value=serde_json::from_slice(&frame).map_err(|_|err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_output"))?;
        if frame["kind"]!="finished" {return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"extension_failed"));}
        let output:ExtensionResponse=serde_json::from_value(frame["response"].clone()).map_err(|_|err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_output"))?;
        if output.api_version!=1 {return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_output"));}
        if !child.wait().await.map_err(|_|err(StatusCode::UNPROCESSABLE_ENTITY,"extension_failed"))?.success(){return Err(err(StatusCode::UNPROCESSABLE_ENTITY,"extension_failed"));}
        Ok(output.result)
    }).await.map_err(|_|err(StatusCode::GATEWAY_TIMEOUT,"extension_timeout"))??;
    let (_,current)=self::package(store,cap).await?;
    if current!=generation {return Err(err(StatusCode::CONFLICT,"extension_revoked"));}
    Ok(result)
}

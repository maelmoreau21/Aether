# Extraits de revue statique — 3 novembre 2026

SHA et limites : voir la matrice. Aucun extrait ne prouve un résultat de test.

## A01 — .github/workflows/ci.yml:417

Motif recherché : `bake-action`. Extrait statique ; ne constitue pas une exécution.

```text
415:       - uses: actions/checkout@v4
416:       - uses: docker/setup-buildx-action@v3
417:       - uses: docker/bake-action@v6
418:         with:
419:           files: docker-bake.ci.hcl
420:           load: true
421:           push: false
422:         env:
423:           GIT_BRANCH: ${{ github.ref_name }}
```

## A02 — .github/workflows/publish.yml:125

Motif recherché : `containerimage.digest`. Extrait statique ; ne constitue pas une exécution.

```text
123:             image_repo="ghcr.io/${OWNER,,}/$image"
124:             if [ "$PUSH_IMAGES" = "true" ]; then
125:               digest=$(jq -er --arg target "$target" '.[$target]["containerimage.digest"] | select(startswith("sha256:"))' <<< "$BUILD_METADATA")
126:               target_ref="${image_repo}@${digest}"
127:               docker pull "$target_ref"
128:               printf '%s\n' "$digest" > "digests/$image.digest"
129:             else
130:               target_ref="${image_repo}:sha-${EXPECTED_REVISION}"
131:             fi
```

## A03 — crates/core/src/inventory.rs:4122

Motif recherché : `prepared_http_start`. Extrait statique ; ne constitue pas une exécution.

```text
4120:     eprintln!(
4121:         "{}",
4122:         json!({"event":"prepared_http_start","asset":asset_id,"node":remote_id,"component":component,"method":request.method().as_str(),"range":request.headers().contains_key("range")})
4123:     );
4124:     let probe = match decode(row.get("probe_json")) {
4125:         Ok(probe) => probe,
4126:         Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "probe_invalid"),
4127:     };
4128:     if let Some(index) = component
```

## A04 — crates/node/src/video_stream.rs:234

Motif recherché : `async fn`. Extrait statique ; ne constitue pas une exécution.

```text
232:         Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/representative")
233:     }
234:     async fn opened(extension: &str) -> OpenedFile {
235:         OpenedFile::open_in_root(&root(), Path::new(&format!("representative.{extension}")))
236:             .await
237:             .unwrap()
238:     }
239:     async fn packets(file: &OpenedFile) -> Vec<String> {
240:         let mut input = file.try_clone_std().unwrap();
```

## A05 — apps/web/src/lib/player/native.ts:138

Motif recherché : `replacement audio`. Extrait statique ; ne constitue pas une exécution.

```text
136:       ? new SegmentedAudio(this.video, (source.audioOffsetSeconds ?? 0) + this.audioOffset, this.audioError, {mode: source.audioSegmentMode, trackId: source.audioTrackId})
137:       : new ExternalAudio(this.video, (source.audioOffsetSeconds ?? 0) + this.audioOffset, this.audioError);
138:     // Keep the current presentation running while the replacement audio is
139:     // prepared. A cold Node preparation must not freeze an otherwise healthy
140:     // video; the commit below still aligns both clocks before resuming.
141:     if (!independentSwitch) { this.pause(); pausedForReplacement = true; }
142:     try {
143:       if (source.prepare) {
144:         if (!source.videoStreamUrl && source.prepareVideo !== false && previous?.url !== source.url) await this.prepareUrl(source.url);
```

## A06 — crates/node/src/packaging.rs:211

Motif recherché : `async fn`. Extrait statique ; ne constitue pas une exécution.

```text
209:     }
210: 
211:     pub async fn prepare(
212:         &self,
213:         root: &Path,
214:         relative: &Path,
215:         expected_size: u64,
216:         expected_mtime: &str,
217:         component: Component,
```

## A07 — apps/web/src/lib/player/external-audio.ts:4

Motif recherché : `seek`. Extrait statique ; ne constitue pas une exécution.

```text
2: 
3: // Preserve encoded audio and channels; never route production audio through a stereo WebAudio graph.
4: // Independent clocks remain client-dependent; seeks and buffering are coordinated explicitly.
5: export class ExternalAudio {
6:   // One maximum-duration Opus packet (120 ms) plus container rounding.
7:   private static readonly END_TOLERANCE = .15;
8:   protected audio = new Audio();
9:   private timer?: ReturnType<typeof setInterval>;
10:   private stallTimer?: ReturnType<typeof setTimeout>;
```

## A08 — crates/node/src/mp4_audio.rs:1

Motif recherché : `Opus`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Read-only, bounded ISO BMFF Opus packet extraction. No decoder, subprocess,
2: //! temporary media, or whole-window payload buffer is involved.
3: //!
4: //! This intentionally accepts a narrow non-fragmented MP4 subset. Unsupported
5: //! layouts fail explicitly, so callers can retain the existing packet-copy path.
6: use crate::{
7:     files::OpenedFile,
```

## A09 — apps/web/src/lib/player/ass-renderer.ts:3

Motif recherché : `JASSUB`. Extrait statique ; ne constitue pas une exécution.

```text
1: // Loaded only when an original ASS/SSA track is selected. All renderer assets
2: // and the fallback font are bundled locally; no remote font discovery.
3: import JASSUB from 'jassub';
4: import fontUrl from 'jassub/dist/default.woff2?url';
5: 
6: export async function createAssRenderer(video: HTMLVideoElement, url: string, offset: number) {
7:   const renderer = new JASSUB({
8:     video, subUrl: url, timeOffset: -offset, queryFonts: false, fonts: [fontUrl],
9:     maxRenderHeight: 2160, libassMemoryLimit: 64 * 1024 * 1024,
```

## A10 — apps/web/src/lib/player/native.test.ts:14

Motif recherché : `seek`. Extrait statique ; ne constitue pas une exécution.

```text
12: class Video extends EventTarget {
13:   currentTime = 0; duration = 12; paused = true; volume = 0.4; playbackRate = 1.5;
14:   src = ''; loads = 0; auto = true; readyState = 4; seeking = false;
15:   buffered = {length:1,start:()=>0,end:()=>12};
16:   textTracks = [];
17:   pause() { this.paused = true; }
18:   play() { this.paused = false; return Promise.resolve(); }
19:   querySelectorAll() { return []; }
20:   removeAttribute() { this.src = ''; }
```

## A11 — crates/node/src/stream.rs:1

Motif recherché : `RANGE`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Bounded 64 KiB HTTP streaming (`GET`, `HEAD`, RFC 9110 single `Range`) directly
2: //! from an already-confined `OpenedFile` handle, eliminating path-reopen TOCTOU races.
3: 
4: use crate::files::OpenedFile;
5: use bytes::Bytes;
6: use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
7: use http_body_util::{BodyExt, Empty, combinators::BoxBody};
```

## A12 — scripts/browser-streaming-performance.cjs:30

Motif recherché : `MEDIA_PERFORMANCE_SESSIONS`. Extrait statique ; ne constitue pas une exécution.

```text
28:     : fs.readFileSync(path.join(root, 'apps/web/src/lib/player', `${name}.ts`), 'utf8'),
29: ]));
30: const concurrentSessions = Number(process.env.MEDIA_PERFORMANCE_SESSIONS || (process.env.MEDIA_PERFORMANCE_CONCURRENT === '1' ? 4 : 0));
31: if (![0, 1, 4, 8, 16].includes(concurrentSessions)) throw new Error('concurrent sessions must be 0, 1, 4, 8 or 16');
32: const seekTargets = process.env.MEDIA_PERFORMANCE_SEEK_TARGETS?.split(',').map(Number);
33: if (seekTargets && (!seekTargets.length || seekTargets.some(value => !Number.isFinite(value) || value < 0))) throw new Error('seek targets must be finite nonnegative seconds');
34: const audioDelayMs = Number(process.env.MEDIA_PERFORMANCE_AUDIO_DELAY_MS || 0);
35: const englishHz = Number(process.env.MEDIA_PERFORMANCE_ENGLISH_HZ || 1050);
36: if (!Number.isFinite(englishHz) || englishHz < 100 || englishHz > 10000) throw new Error('English audio frequency must be 100..10000 Hz');
```

## A13 — docs/DEPLOYMENT_SYNOLOGY.md:21

Motif recherché : `Synology`. Extrait statique ; ne constitue pas une exécution.

```text
19: > Les anciennes affirmations de sécurité distribuée ne prouvent pas HTTPS entre machines.
20: 
21: # Déploiement d'Aether sur Synology DS1522+ (Standalone & Distribué P2)
22: 
23: Ce guide fournit les instructions exactes pour installer et exploiter **Aether** sur un NAS **Synology DS1522+** (processeur AMD Ryzen Embedded R1600 `x86_64` sans iGPU) sous DSM 7.2+ avec **Container Manager** (ou `docker compose` en SSH), en coexistence totale et sans risque avec Jellyfin.
24: 
25: ---
26: 
27: ## 1. Particularités matérielles et garanties de sécurité sur Synology DS1522+
```

## A14 — crates/node/src/packaging.rs:250

Motif recherché : `lease`. Extrait statique ; ne constitue pas une exécution.

```text
248:                 serde_json::json!({"event":"preparation_cache_hit","attempt":log.id,"component":format!("{component:?}"),"elapsed_ms":log.started.elapsed().as_millis()})
249:             );
250:             return Ok(opened.with_cache_lease(directory));
251:         }
252:         let busy = || {
253:             io::Error::new(
254:                 io::ErrorKind::WouldBlock,
255:                 "preparation busy; retry explicitly",
256:             )
```

## A15 — crates/core/tests/extensions.rs:588

Motif recherché : `signed_package_real_worker`. Extrait statique ; ne constitue pas une exécution.

```text
586: 
587: #[tokio::test(flavor = "multi_thread")]
588: async fn signed_package_real_worker_live_permissions_persistence_and_lifecycle() {
589:     let temp = tempfile::tempdir().unwrap();
590:     let store = IdentityStore::open(&temp.path().join("db.sqlite"))
591:         .await
592:         .unwrap();
593:     store
594:         .bootstrap(
```

## A16 — ../Aether-Plugins/Aether-Plugins-TMDb/src/lib.rs:20

Motif recherché : `DEFAULT_BASE_URL`. Extrait statique ; ne constitue pas une exécution.

```text
18: use std::time::Duration;
19: 
20: const DEFAULT_BASE_URL: &str = "https://api.themoviedb.org/3";
21: const IMAGE_ORIGINAL_URL: &str = "https://image.tmdb.org/t/p/original";
22: const IMAGE_W500_URL: &str = "https://image.tmdb.org/t/p/w500";
23: const IMAGE_PROFILE_URL: &str = "https://image.tmdb.org/t/p/w185";
24: 
25: #[derive(Clone, Default)]
26: pub struct TmdbPlugin;
```

## A17 — apps/web/src/lib/admin/AdvancedLibraryOptions.svelte:26

Motif recherché : `Moteur à venir`. Extrait statique ; ne constitue pas une exécution.

```text
24:   <ProviderPriority id="metadata-providers" label="Métadonnées" bind:value={options.metadataProviders} suggestions={providers} />
25:   <ProviderPriority id="image-providers" label="Images" bind:value={options.imageProviders} suggestions={providers} />
26:   <ProviderPriority id="similar-providers" label="Articles similaires (moteur à venir)" bind:value={options.similarItemsProviders} suggestions={providers} />
27:   <label for="online-refresh">Actualiser les métadonnées depuis Internet</label>
28:   <select id="online-refresh" bind:value={options.onlineRefreshIntervalDays}><option value={0}>Jamais automatiquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
29:   <p class="hint">Les fournisseurs installés et autorisés actualisent les champs automatiques à cette cadence. Les corrections manuelles restent prioritaires. Cette cadence est distincte de l’actualisation des NFO locaux.</p>
30: </details>
31: 
32: {#if kind !== 'music'}
```

## B01 — crates/core/src/scans.rs:127

Motif recherché : `async fn`. Extrait statique ; ne constitue pas une exécution.

```text
125:     }
126: 
127:     pub async fn shutdown(&self) {
128:         self.cancel_all();
129:         self.slots.close();
130:         let tasks = self
131:             .tasks
132:             .lock()
133:             .map(|mut tasks| std::mem::take(&mut *tasks))
```

## B02 — crates/core/src/scheduler.rs:206

Motif recherché : `run_library_automation`. Extrait statique ; ne constitue pas une exécution.

```text
204:         return Ok(());
205:     }
206:     run_library_automation(auth, timestamp).await?;
207:     let due:Option<(String,i64,i64)>=crate::database::query_as("SELECT s.library_id,s.interval_minutes,s.next_run_at FROM scheduled_scans s JOIN libraries l ON l.id=s.library_id JOIN media_nodes n ON n.id=l.node_id WHERE s.enabled=1 AND s.next_run_at<=? AND l.enabled=1 AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM scan_jobs j WHERE j.library_id=l.id AND j.status IN ('queued','running')) ORDER BY s.next_run_at,s.library_id LIMIT 1")
208:         .bind(timestamp).fetch_optional(&auth.store.pool).await?;
209:     let Some((id, minutes, previous)) = due else {
210:         let due: Option<(String,String,i64)> = crate::database::query_as("SELECT l.id,l.options_json,l.next_metadata_refresh_at FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id) AND l.enabled=1 AND l.id NOT IN ('fixture-films','fixture-series') AND l.kind!='music' AND n.status!='revoked' AND l.next_metadata_refresh_at<=? AND CAST(COALESCE(json_extract(l.options_json,'$.automaticRefreshIntervalDays'),'0') AS BIGINT)>0 ORDER BY l.next_metadata_refresh_at,l.id LIMIT 1")
211:             .bind(timestamp).fetch_optional(&auth.store.pool).await?;
212:         if let Some((id, encoded, previous)) = due {
```

## B03 — crates/core/src/libraries.rs:33

Motif recherché : `folder`. Extrait statique ; ne constitue pas une exécution.

```text
31:     pub root: String,
32:     #[sqlx(skip)]
33:     pub folders: Vec<LibraryFolder>,
34:     #[sqlx(try_from = "crate::database::DbBool")]
35:     pub enabled: bool,
36:     #[sqlx(default)]
37:     pub excluded_patterns: String,
38:     #[sqlx(skip)]
39:     pub user_ids: Vec<String>,
```

## B04 — crates/node/src/scanner.rs:143

Motif recherché : `is_excluded`. Extrait statique ; ne constitue pas une exécution.

```text
141: }
142: 
143: pub(crate) fn is_excluded(relative_path: &str, name: &str, excluded_patterns: &[String]) -> bool {
144:     if ignored(name) {
145:         return true;
146:     }
147:     let lower_name = name.to_ascii_lowercase();
148:     let lower_rel = relative_path.to_ascii_lowercase();
149:     for pat in excluded_patterns {
```

## B05 — crates/node/src/scanner.rs:503

Motif recherché : `episode`. Extrait statique ; ne constitue pas une exécution.

```text
501: 
502: #[derive(Debug, Clone, PartialEq, Eq)]
503: pub struct ParsedSeriesEpisode {
504:     pub series_dir: PathBuf,
505:     pub series_raw_title: String,
506:     pub series_year: Option<u16>,
507:     pub series_external_ids: BTreeMap<String, String>,
508:     pub series_identity_key: String,
509:     pub season_number: u32,
```

## B06 — crates/core/src/automation.rs:101

Motif recherché : `provider`. Extrait statique ; ne constitue pas une exécution.

```text
99:     Ok(id)
100: }
101: /// Cache provider responses for external APIs with TTL.
102: pub async fn get_cached_provider_response(
103:     tx: &mut Transaction<'_, Any>,
104:     provider: &str,
105:     operation: &str,
106:     query_val: &Value,
107:     now: i64,
```

## B07 — crates/core/src/product.rs:355

Motif recherché : `struct Editorial`. Extrait statique ; ne constitue pas une exécution.

```text
353: #[derive(Deserialize)]
354: #[serde(rename_all = "camelCase")]
355: struct Editorial {
356:     title: String,
357:     year: Option<i64>,
358:     #[serde(default)]
359:     original_title: Option<String>,
360:     #[serde(default)]
361:     tagline: Option<String>,
```

## B08 — crates/core/src/automation.rs:870

Motif recherché : `automatic_metadata_provenance`. Extrait statique ; ne constitue pas une exécution.

```text
868:                 v.is_null() || v.as_str() == Some("") || v.as_array().is_some_and(Vec::is_empty)
869:             });
870:             let previous:Option<(String,i64,String)>=query_as("SELECT extension_id,priority,value_json FROM automatic_metadata_provenance WHERE work_id=? AND field_name=?")
871:                 .bind(&work).bind(destination).fetch_optional(&mut **tx).await?;
872:             let replace = empty
873:                 || previous
874:                     .as_ref()
875:                     .is_some_and(|(owner, old_priority, old_value)| {
876:                         serde_json::from_str::<Value>(old_value)
```

## B09 — crates/core/src/artwork.rs:34

Motif recherché : `async fn`. Extrait statique ; ne constitue pas une exécution.

```text
32:     pub width: Option<u32>,
33: }
34: pub async fn legacy(
35:     store: &IdentityStore,
36:     work: &str,
37:     width: u32,
38:     headers: &HeaderMap,
39: ) -> Result<Response, StatusCode> {
40:     if ![0, 320, 640, 1280].contains(&width) {
```

## B10 — crates/core/src/product.rs:99

Motif recherché : `automatic_collections`. Extrait statique ; ne constitue pas une exécution.

```text
97: }
98: /// Derived collections share the existing list UI but never grant access or mutate personal lists.
99: async fn automatic_collections(
100:     auth: &AuthState,
101:     actor: &Authenticated,
102: ) -> Result<std::collections::BTreeMap<String, (String, Vec<Value>)>, Error> {
103:     let rows: Vec<(String, String, String, Option<String>)> = query_as("SELECT w.id,w.title,w.editorial_json,w.manual_json FROM inventory_works w JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id WHERE w.kind='film' AND w.has_available_asset=1 AND l.enabled=1 AND n.status!='revoked' AND CAST(COALESCE(json_extract(l.options_json,'$.automaticCollections'),'0') AS TEXT) IN ('1','true') AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) AND (json_extract(w.editorial_json,'$.collection') IS NOT NULL OR json_extract(COALESCE(w.manual_json,'{}'),'$.collection') IS NOT NULL) ORDER BY w.effective_year,w.title_sort,w.id LIMIT 10000")
104:         .bind(actor.user.role.as_str()).bind(&actor.user.id)
105:         .fetch_all(auth.store.pool()).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
```

## B11 — apps/web/src/lib/player/version-choice.ts:3

Motif recherché : `function`. Extrait statique ; ne constitue pas une exécution.

```text
1: import type { Media } from './types';
2: // Browser declarations are a selection hint; actual decoding errors stay explicit in the player.
3: export function bestVersion(media: Media[], canPlay: (mime:string)=>string): Media | undefined {
4:   const candidates=media.filter(item=>item.available!==false&&item.nodeStatus!=='disconnected'&&item.nodeStatus!=='revoked');
5:   function score(item:Media){
6:     let best=-1;
7:     for(const source of item.sources){
8:       const video=source.tracks?.find(track=>track.codec_type==='video');
9:       const codec=video?.codec_name;
```

## B12 — crates/core/src/catalog.rs:167

Motif recherché : `pub async fn list`. Extrait statique ; ne constitue pas une exécution.

```text
165: }
166: 
167: pub async fn list(
168:     State(inventory): State<Arc<Inventory>>,
169:     query: Result<Query<CatalogQuery>, QueryRejection>,
170:     mut visible: Option<Extension<crate::permissions::VisibleLibraries>>,
171:     store: Option<Extension<crate::identity::IdentityStore>>,
172:     actor: Option<Extension<crate::auth::Authenticated>>,
173: ) -> Response {
```

## B13 — crates/core/src/catalog.rs:80

Motif recherché : `genre`. Extrait statique ; ne constitue pas une exécution.

```text
78: pub struct CatalogQuery {
79:     library: Option<String>,
80:     genre: Option<String>,
81:     year: Option<i64>,
82:     quality: Option<i64>,
83:     codec: Option<String>,
84:     language: Option<String>,
85:     kind: Option<String>,
86:     q: Option<String>,
```

## B14 — crates/core/src/progress.rs:240

Motif recherché : `async fn write`. Extrait statique ; ne constitue pas une exécution.

```text
238: }
239: 
240: async fn write(
241:     State(auth): State<AuthState>,
242:     Path(work): Path<String>,
243:     headers: HeaderMap,
244:     Json(mut input): Json<Input>,
245: ) -> Result<Json<Value>, StatusCode> {
246:     let user = authorized(&auth, &headers, &work, true).await?;
```

## B15 — crates/core/src/product.rs:373

Motif recherché : `actors`. Extrait statique ; ne constitue pas une exécution.

```text
371:     writers: Vec<String>,
372:     #[serde(default)]
373:     actors: Vec<Value>,
374:     #[serde(default)]
375:     ratings: Vec<Value>,
376:     #[serde(default)]
377:     external_ids: std::collections::BTreeMap<String, String>,
378:     #[serde(default)]
379:     poster_url: Option<String>,
```

## B16 — apps/web/src/lib/catalog/Catalog.svelte:401

Motif recherché : `continue`. Extrait statique ; ne constitue pas une exécution.

```text
399:     if (!id) for (const {kind, key} of requests) {
400:       const cached = cachedCatalog(key);
401:       if (!cached) continue;
402:       cachedKinds++;
403:       if (kind === 'film') { films = cached.items; totalFilms = cached.total; moreFilms = cached.hasMore ?? (cached.total ?? 0) > start + 24; }
404:       else { series = cached.items; totalSeries = cached.total; moreSeries = cached.hasMore ?? (cached.total ?? 0) > start + 24; }
405:     }
406:     if (cachedKinds === 0) {
407:       films = [];
```

## B17 — crates/node/src/trickplay.rs:198

Motif recherché : `async fn`. Extrait statique ; ne constitue pas une exécution.

```text
196: 
197:     /// Check for an existing, completed native Aether trickplay cache.
198:     pub async fn check_native_cache(&self, request: &TrickplayRequest) -> Option<CacheManifest> {
199:         let key = self.fingerprint(request);
200:         let target_dir = self.cache_root.join(&key);
201:         let manifest_bytes = tokio::fs::read(target_dir.join("manifest.json"))
202:             .await
203:             .ok()?;
204:         let meta: CacheManifest = serde_json::from_slice(&manifest_bytes).ok()?;
```

## B18 — crates/core/src/extensions.rs:1993

Motif recherché : `fingerprintsCompact`. Extrait statique ; ne constitue pas une exécution.

```text
1991:                 "episodeNumber": number.parse::<u32>().ok(),
1992:                 "duration": duration,
1993:                 "fingerprintsCompact": STANDARD.encode(bytes),
1994:                 "fingerprintRate": result["fingerprintRate"]
1995:             });
1996:             if let Some(compact) = tail_compact {
1997:                 ep_val["tailFingerprintsCompact"] = json!(compact);
1998:             }
1999:             if let Some(ts) = result["tailStart"].as_f64() {
```

## B19 — ../Aether-Plugins/Aether-Plugins-TMDb/src/lib.rs:351

Motif recherché : `original_title`. Extrait statique ; ne constitue pas une exécution.

```text
349:                 title: meta.title.unwrap_or_else(|| query.title.clone()),
350:                 artist: None,
351:                 original_title: meta.original_title,
352:                 year: meta.year,
353:                 media_type: query.media_type,
354:                 overview: meta.synopsis,
355:                 poster_url: meta.posters.first().map(|image| image.url.clone()),
356:                 score: meta.ratings.first().map(|rating| rating.value),
357:             }]);
```

## B20 — crates/core/src/lib.rs:2

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Local Core: persistent authentication and isolated synthetic media inventory.
2: pub mod account;
3: pub mod admin_backups;
4: pub mod admin_users;
5: pub mod api_keys;
6: pub mod auth;
7: pub mod automation;
8: pub mod automation_execution;
```

## C01 — crates/core/src/server_settings.rs:96

Motif recherché : `struct SettingsInput`. Extrait statique ; ne constitue pas une exécution.

```text
94: #[derive(Deserialize)]
95: #[serde(rename_all = "camelCase", deny_unknown_fields)]
96: struct SettingsInput {
97:     server_name: String,
98:     welcome_message: String,
99:     sso_button_label: Option<String>,
100:     block_sso_local_accounts: Option<bool>,
101:     trickplay_enabled: Option<bool>,
102:     trickplay_quality: Option<u8>,
```

## C02 — crates/core/src/system_memory.rs:13

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
11: #[derive(Debug, Clone, Serialize, Deserialize)]
12: #[serde(rename_all = "camelCase")]
13: pub struct ProcessMemoryBreakdown {
14:     pub pid: u32,
15:     pub name: String,
16:     pub rss_bytes: u64,
17:     pub anon_bytes: u64,
18:     pub file_bytes: u64,
19:     pub peak_rss_bytes: u64,
```

## C03 — crates/core/src/server_settings.rs:347

Motif recherché : `async fn network`. Extrait statique ; ne constitue pas une exécution.

```text
345:     Ok(Json(load(&auth.store).await?))
346: }
347: async fn network(
348:     State(auth): State<AuthState>,
349:     headers: HeaderMap,
350: ) -> Result<Json<serde_json::Value>, StatusCode> {
351:     administrator(&auth, &headers, false).await?;
352:     Ok(Json(
353:         serde_json::json!({"publicUrl":auth.origin(),"secureCookies":auth.secure,"webPath":"/app/","apiPath":"/api/v1/","transportManagedByDeployment":true}),
```

## C04 — crates/core/src/account.rs:120

Motif recherché : `async fn sessions`. Extrait statique ; ne constitue pas une exécution.

```text
118:     hash(&format!("device:{token_hash}"))
119: }
120: async fn sessions(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
121:     let actor = actor(&auth, &headers, false).await?;
122:     let rows: Vec<(String,String,i64,i64,i64)> = query_as("SELECT token_hash,device_label,created_at,last_seen_at,expires_at FROM sessions WHERE user_id=? AND expires_at>unixepoch() AND last_seen_at>unixepoch()-604800 ORDER BY last_seen_at DESC LIMIT 200")
123:         .bind(&actor.user.id).fetch_all(&auth.store.pool).await.map_err(db)?;
124:     Ok(Json(
125:         json!({"sessions":rows.into_iter().map(|(key,device,created,last,expires)|json!({"id":public_id(&key),"device":device,"createdAt":created,"lastSeenAt":last,"expiresAt":expires,"current":key==actor.token_hash})).collect::<Vec<_>>()}),
126:     ))
```

## C05 — crates/core/src/permissions.rs:12

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
10: 
11: #[derive(Clone)]
12: pub struct MediaAccess {
13:     pub auth: AuthState,
14:     pub root: PathBuf,
15: }
16: #[derive(Clone)]
17: pub struct VisibleLibraries(pub Vec<String>);
18: // Retained for callers describing the role default; actual rights also require an active library.
```

## C06 — crates/core/src/oidc.rs:170

Motif recherché : `nonce`. Extrait statique ; ne constitue pas une exécution.

```text
168: struct Claims {
169:     sub: String,
170:     nonce: String,
171:     iat: u64,
172:     exp: u64,
173:     aud: serde_json::Value,
174:     #[serde(default)]
175:     azp: Option<String>,
176:     #[serde(flatten)]
```

## C07 — crates/core/src/identity.rs:1

Motif recherché : `session`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Local identity persistence. No HTTP routes, sessions or OIDC implementation.
2: use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
3: 
4: pub fn argon2_memory_cost_kib() -> u32 {
5:     std::env::var("MEDIA_ARGON2_MEMORY_KIB")
6:         .ok()
7:         .and_then(|v| v.parse::<u32>().ok())
```

## C08 — crates/core/src/library_options.rs:183

Motif recherché : `pub fn validate`. Extrait statique ; ne constitue pas une exécution.

```text
181:             .retain(|role, _| self.image_enabled(role.strip_prefix("series/").unwrap_or(role)));
182:     }
183:     pub fn validate(&self) -> bool {
184:         valid_language(&self.preferred_metadata_language)
185:             && self.metadata_country_code.len() == 2
186:             && self
187:                 .metadata_country_code
188:                 .bytes()
189:                 .all(|c| c.is_ascii_uppercase())
```

## C09 — crates/core/src/scheduler.rs:194

Motif recherché : `pub async fn run_due`. Extrait statique ; ne constitue pas une exécution.

```text
192: 
193: /// One bounded pass, exported for deterministic integration tests and operational checks.
194: pub async fn run_due(
195:     auth: &AuthState,
196:     root: &std::path::Path,
197:     jobs: &Arc<ScanJobs>,
198:     timestamp: i64,
199: ) -> Result<(), sqlx::Error> {
200:     let _tick = jobs.scheduler_tick.lock().await;
```

## C10 — crates/core/src/progress.rs:105

Motif recherché : `async fn activity`. Extrait statique ; ne constitue pas une exécution.

```text
103:     Ok(StatusCode::NO_CONTENT)
104: }
105: async fn activity(
106:     State(auth): State<AuthState>,
107:     headers: HeaderMap,
108: ) -> Result<Json<Value>, StatusCode> {
109:     let actor = auth.authenticate(&headers).await?;
110:     if actor.user.role != Role::Administrator {
111:         return Err(StatusCode::FORBIDDEN);
```

## C11 — crates/core/src/backup_bundle.rs:11

Motif recherché : `MDBK0002`. Extrait statique ; ne constitue pas une exécution.

```text
9: };
10: use zeroize::Zeroizing;
11: const MAGIC: &[u8; 8] = b"MDBK0002";
12: const CHUNK: usize = 65536;
13: #[derive(Debug, thiserror::Error)]
14: pub enum BundleError {
15:     #[error("backup I/O failed")]
16:     Io(#[from] std::io::Error),
17:     #[error("backup checksum or format validation failed")]
```

## C12 — crates/core/src/system_tasks.rs:89

Motif recherché : `cache`. Extrait statique ; ne constitue pas une exécution.

```text
87:     /// `.database-*.tmp`, `.backup-*.tmp`, `.restore-*.tmp`, `.media-backup-*.tmp`.
88:     BackupStaging,
89:     /// `<32 hex digits>.tmp` written by the image preview cache.
90:     ImageCache,
91: }
92: 
93: impl TempPattern {
94:     fn matches(self, name: &str) -> bool {
95:         match self {
```

## C13 — crates/core/src/nodes.rs:2

Motif recherché : `relay`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Distributed Media Node management, mutual HMAC authentication, remote scan ingestion,
2: //! and bounded Core relay streaming for Milestone P2.
3: 
4: use crate::{
5:     auth::AuthState,
6:     error,
7:     identity::{IdentityStore, Role},
8:     inventory,
```

## C14 — crates/core/src/database.rs:173

Motif recherché : `schema`. Extrait statique ; ne constitue pas une exécution.

```text
171: }
172: 
173: /// Install the one current schema on an empty database. Existing databases are
174: /// never rewritten here; incompatible older schemas fail explicitly.
175: pub async fn initialize_fresh_schema(conn: &mut AnyConnection) -> Result<(), sqlx::Error> {
176:     const SOURCE: &str = include_str!("../schema.sql");
177:     const SQLITE_START: &str = "-- AETHER_SCHEMA_SQLITE_BEGIN";
178:     const SQLITE_END: &str = "-- AETHER_SCHEMA_SQLITE_END";
179:     const POSTGRES_START: &str = "-- AETHER_SCHEMA_POSTGRES_BEGIN";
```

## D01 — crates/extension-sdk/src/lib.rs:9

Motif recherché : `API_VERSION`. Extrait statique ; ne constitue pas une exécution.

```text
7: pub mod storage;
8: 
9: pub const API_VERSION: u32 = 1;
10: pub const MAX_MODULE: usize = 262_144;
11: pub const MAX_JSON: usize = 65_536;
12: 
13: /// Generic ABI envelope. Host adapters authorize operations before invocation.
14: #[derive(Clone, Debug, Default, Serialize, Deserialize)]
15: #[serde(rename_all = "camelCase", deny_unknown_fields)]
```

## D02 — crates/core/src/extensions.rs:18

Motif recherché : `signature`. Extrait statique ; ne constitue pas une exécution.

```text
16:     Package,
17: };
18: use ring::signature::{ED25519, UnparsedPublicKey};
19: use serde::{Deserialize, Serialize};
20: use serde_json::{Value, json};
21: use sha2::{Digest, Sha256};
22: use std::{
23:     process::Stdio,
24:     time::{Duration, Instant, SystemTime, UNIX_EPOCH},
```

## D03 — crates/core/src/extensions.rs:648

Motif recherché : `upgrade_with_key`. Extrait statique ; ne constitue pas une exécution.

```text
646:     if input.upgrade {
647:         let ext_id = input.package.manifest.id.clone();
648:         upgrade_with_key(auth, headers, ext_id, input.package, key).await
649:     } else {
650:         install_with_key(auth, headers, input.package, key).await
651:     }
652: }
653: 
654: async fn upgrade(
```

## D04 — crates/core/src/extension_updates.rs:66

Motif recherché : `migration`. Extrait statique ; ne constitue pas une exécution.

```text
64:     conn: &mut AnyConnection,
65:     id: &str,
66:     migration: Option<&media_extension_sdk::StorageMigration>,
67: ) -> Result<(), Error> {
68:     if let Some(migration) = migration {
69:         for rename in &migration.key_renames {
70:             let conflicts: i64 = query_scalar("SELECT COUNT(*) FROM extension_data a JOIN extension_data b ON a.extension_id=b.extension_id AND a.scope=b.scope AND a.scope_id=b.scope_id WHERE a.extension_id=? AND a.key=? AND b.key=?")
71:                 .bind(id).bind(&rename.from).bind(&rename.to).fetch_one(&mut *conn).await.map_err(db)?;
72:             if conflicts != 0 {
```

## D05 — crates/core/src/extensions.rs:123

Motif recherché : `generation`. Extrait statique ; ne constitue pas une exécution.

```text
121:     payload: &Value,
122:     config: &Value,
123:     generation: &str,
124:     package_sha: &str,
125:     music_search: bool,
126: ) -> Result<Value, serde_json::Error> {
127:     static KEY: std::sync::OnceLock<ring::hmac::Key> = std::sync::OnceLock::new();
128:     let key = KEY.get_or_init(|| {
129:         ring::hmac::Key::new(ring::hmac::HMAC_SHA256, uuid::Uuid::new_v4().as_bytes())
```

## D06 — crates/core/tests/extensions.rs:141

Motif recherché : `sandbox_denies`. Extrait statique ; ne constitue pas une exécution.

```text
139: 
140: #[test]
141: fn sandbox_denies_imports_cpu_memory_invalid_pointers_and_output() {
142:     use media_poc::extension_runtime::{Invocation, execute};
143:     let run = |wasm: Vec<u8>| execute(Invocation::new(STANDARD.encode(wasm), input()));
144:     assert_eq!(
145:         run(good_module()).unwrap().result["title"],
146:         "Synthetic title"
147:     );
```

## D07 — crates/core/tests/extensions.rs:1132

Motif recherché : `http_broker_rejects_ssrf`. Extrait statique ; ne constitue pas une exécution.

```text
1130: 
1131: #[test]
1132: fn http_broker_rejects_ssrf_and_enforces_domain_allowlist() {
1133:     use media_extension_sdk::HostHttpRequest;
1134:     use media_poc::extension_runtime::{HostState, execute_host_http};
1135:     use wasmi::StoreLimitsBuilder;
1136: 
1137:     let limits = StoreLimitsBuilder::new().build();
1138:     let mut state = HostState {
```

## D08 — ../Aether-Plugins/Aether-Plugins-TMDb/src/lib.rs:36

Motif recherché : `external_ids`. Extrait statique ; ne constitue pas une exécution.

```text
34:         &self,
35:         id: &str,
36:         external_ids: &BTreeMap<String, String>,
37:         media_type: MediaType,
38:         config: &ProviderConfig,
39:     ) -> Result<String, PluginError> {
40:         let numeric = |s: &str| {
41:             !s.is_empty()
42:                 && s.bytes().all(|b| b.is_ascii_digit())
```

## D09 — ../Aether-Plugins/Aether-Plugins-OMDb/src/lib.rs:426

Motif recherché : `handle_extension`. Extrait statique ; ne constitue pas une exécution.

```text
424:     req: aether_plugin_api::ExtensionRequest,
425: ) -> aether_plugin_api::ExtensionResponse {
426:     aether_plugin_api::http::handle_extension(&OmdbPlugin::new(), req)
427: }
```

## D10 — ../Aether-Plugins/Aether-Plugins-LrcLib/src/lib.rs:367

Motif recherché : `handle_extension`. Extrait statique ; ne constitue pas une exécution.

```text
365:         };
366:     }
367:     aether_plugin_api::http::handle_extension(&LrcLibPlugin::new(), req)
368: }
```

## D11 — ../Aether-Plugins/Aether-Plugins-IntroSkipper/src/lib.rs:4

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
2: //! Optimized Rust implementation inspired by https://github.com/intro-skipper/intro-skipper
3: 
4: pub mod chapters;
5: pub mod consensus;
6: pub mod credits;
7: pub mod matcher;
8: pub mod types;
9: 
10: use aether_plugin_api::{
```

## D12 — crates/core/src/extension_artwork_rotation.rs:6

Motif recherché : `rotate`. Extrait statique ; ne constitue pas une exécution.

```text
4: #[derive(Deserialize)]
5: #[serde(rename_all = "camelCase", deny_unknown_fields)]
6: pub(super) struct RotateRequest {
7:     library_id: String,
8:     #[serde(default)]
9:     after: Option<String>,
10: }
11: 
12: /// One work per call, allowing a client to stop between works. No media files
```

## D13 — crates/core/src/extension_repositories.rs:120

Motif recherché : `async fn`. Extrait statique ; ne constitue pas une exécution.

```text
118:     }
119: }
120: pub(crate) async fn fetch(raw: &str, maximum: usize) -> Result<Vec<u8>, Error> {
121:     let url = safe_url(raw)?;
122:     #[cfg(test)]
123:     if let Some(bytes) = test_documents().lock().unwrap().get(raw).cloned() {
124:         return if bytes.len() <= maximum {
125:             Ok(bytes)
126:         } else {
```

## D14 — apps/web/src/lib/admin/AdminExtensions.svelte:42

Motif recherché : `rollback`. Extrait statique ; ne constitue pas une exécution.

```text
40:   type SettingValue = string | number | boolean;
41:   type ExtensionSettings = Record<string, SettingValue>;
42:   type Installed = { manifest: Manifest; publisherId: string; sha256: string; enabled: boolean; libraryIds: string[]; allLibraries?: boolean; settings?: ExtensionSettings; rollbackVersion?: string | null };
43:   type Repository = { id: string; name: string; url: string; refreshedAt: number };
44:   type CatalogueEntry = { repositoryId: string; repositoryName: string; compatible: boolean; manifest: Manifest; publisherId: string; publicKey: string };
45:   let tab = $state<'installed' | 'catalogue' | 'repositories'>('installed');
46:   let category = $state('all');
47:   let catalogueId = $state<string | null>(null);
48:   let addingRepository = $state(false);
```

## D15 — ../Aether-Plugins/Cargo.toml:3

Motif recherché : `members`. Extrait statique ; ne constitue pas une exécution.

```text
1: [workspace]
2: resolver = "2"
3: members = [
4:     "crates/aether-plugin-api",
5:     "Aether-Plugins-TMDb",
6:     "Aether-Plugins-IMDb",
7:     "Aether-Plugins-fanart.tv",
8:     "Aether-Plugins-OMDb",
9:     "Aether-Plugins-MusicBrainz",
```

## E01 — apps/web/src/lib/theme/index.ts:4

Motif recherché : `theme`. Extrait statique ; ne constitue pas une exécution.

```text
2: import { savePreferences } from '../preferences';
3: 
4: export type ThemeMode = 'light' | 'dark' | 'system';
5: 
6: const STORAGE_KEY = 'media_theme';
7: 
8: function getSystemTheme(): 'light' | 'dark' {
9:   if (typeof window === 'undefined') return 'dark';
10:   return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
```

## E02 — apps/web/src/lib/preferences.ts:14

Motif recherché : `homeSections`. Extrait statique ; ne constitue pas une exécution.

```text
12:   rememberAudioSelections?: boolean; rememberSubtitleSelections?: boolean; playDefaultAudioTrack?: boolean;
13:   hidePlayedInLatest?: boolean;
14:   homeSections?: HomeSection[]; cardSize?: 'small' | 'medium' | 'large';
15:   cardDensity?: 'comfortable' | 'compact'; accentColor?: 'orange' | 'blue' | 'violet' | 'green';
16:   offsets?: { audio: number; subtitle: number }; subtitlePreference?: SubtitlePreference;
17:   subtitleStyle?: SubtitleStyle;
18:   skipIntroEnabled?: boolean; skipCreditsEnabled?: boolean;
19:   autoSkipIntro?: boolean; autoSkipCredits?: boolean;
20:   autoplayNextEpisode?: boolean; skipBackSeconds?: number; skipForwardSeconds?: number;
```

## E03 — apps/web/src/lib/catalog/Poster.svelte:91

Motif recherché : `badge`. Extrait statique ; ne constitue pas une exécution.

```text
89: 
90:     {#if isWatched}
91:       <span class="watched-badge" title="Déjà visionné">
92:         <Icon name="check" size={14} />
93:         <span class="sr-only">Déjà visionné</span>
94:       </span>
95:     {/if}
96:     {#if work.available === false}
97:       <span class="offline-badge">Indisponible</span>
```

## E04 — apps/web/src/lib/i18n/index.ts:4

Motif recherché : `locale`. Extrait statique ; ne constitue pas une exécution.

```text
2: import { savePreferences } from '../preferences';
3: 
4: export type SupportedLocale = 'fr' | 'en';
5: 
6: const translations = {
7:   fr: {
8:     // Navigation & Common
9:     home: 'Accueil',
10:     films: 'Films',
```

## E05 — crates/core/src/preferences.rs:246

Motif recherché : `async fn write`. Extrait statique ; ne constitue pas une exécution.

```text
244:     }))
245: }
246: async fn write(
247:     State(auth): State<AuthState>,
248:     headers: HeaderMap,
249:     Json(input): Json<Preferences>,
250: ) -> Result<StatusCode, StatusCode> {
251:     let identity = auth.authenticate(&headers).await?;
252:     auth.require_csrf(&headers, &identity)?;
```

## E06 — apps/web/src/lib/player/playback-preferences.ts:3

Motif recherché : `audio`. Extrait statique ; ne constitue pas une exécution.

```text
1: import type { Preferences } from '../preferences';
2: import type { SubtitlePreference } from './subtitle-preference';
3: import { audioLanguage } from './source-selection';
4: 
5: export const SKIP_SECONDS = [5, 10, 15, 30, 60] as const;
6: 
7: /** Old accounts keep manual episode changes and ten-second control buttons. */
8: export function playbackPreferences(value: {
9:   autoplayNextEpisode?: unknown; skipBackSeconds?: unknown; skipForwardSeconds?: unknown;
```

## E07 — apps/web/src/lib/player/PlayerControls.svelte:91

Motif recherché : `seek`. Extrait statique ; ne constitue pas une exécution.

```text
89: 
90: 
91:     onSeek,
92: 
93: 
94: 
95:     onAudio,
96: 
97: 
```

## E08 — apps/web/src/lib/player/compatibility.ts:6

Motif recherché : `supported`. Extrait statique ; ne constitue pas une exécution.

```text
4: // proves decoding. Unknown codecs and ambiguous mixed-codec originals still
5: // require an actual load; do not manufacture incomplete AV1/HEVC codec strings.
6: export function unsupportedAudio(source:Source, canPlay:(mime:string)=>string):string|undefined {
7:   const tracks=source.tracks?.filter(track=>track.codec_type==='audio') ?? [];
8:   const selected=/^internal-(\d+)$/.exec(source.audioTrackId);
9:   const audios=selected ? tracks.filter(track=>track.index===Number(selected[1])) : tracks;
10:   if (!audios.length) return;
11:   const rejected=audios.filter(track=>{
12:     if (track.codec_name!=='dts') return false;
```

## E09 — crates/core/src/progress.rs:224

Motif recherché : `resume`. Extrait statique ; ne constitue pas une exécution.

```text
222: }
223: 
224: /// Apply the server's resume policy at the persistence boundary for every client.
225: /// Position zero remains an explicit restart / mark-unwatched action.
226: fn apply_resume_policy(input: &mut Input, settings: &crate::server_settings::Settings) {
227:     if input.position == 0.0 || input.watched {
228:         return;
229:     }
230:     let percent = input.position / input.duration * 100.0;
```

## E10 — crates/core/src/product.rs:87

Motif recherché : `personal_lists`. Extrait statique ; ne constitue pas une exécution.

```text
85:         auth.require_csrf(headers, &actor)?;
86:     }
87:     let count: i64 = query_scalar("SELECT COUNT(*) FROM personal_lists WHERE id=? AND user_id=?")
88:         .bind(id)
89:         .bind(&actor.user.id)
90:         .fetch_one(&auth.store.pool)
91:         .await
92:         .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
93:     if count != 1 {
```

## E11 — apps/web/src/lib/catalog/Catalog.svelte:31

Motif recherché : `search`. Extrait statique ; ne constitue pas une exécution.

```text
29:   import { session, getSessionGeneration } from '../auth/session';
30: 
31:   let { section = 'home' }: { section?: 'home' | 'films' | 'series' | 'search' | 'settings' } = $props();
32:   let name = $state(DEFAULT_NAME);
33:   let media = $state<Media[]>([]);
34:   let mediaError = $state('');
35:   let busy = $state(true);
36:   let pendingCatalogKinds = $state(0);
37:   let partialCatalogError = $state('');
```

## E12 — apps/web/src/lib/ui/Navbar.svelte:2

Motif recherché : `nav`. Extrait statique ; ne constitue pas une exécution.

```text
1: <script module lang="ts">
2:   export const navLinks = [
3:     { key: 'home', path: '/', label: 'Accueil', symbol: '⌂' },
4:     { key: 'lists', path: 'lists/', label: 'Playlists', symbol: '☷' },
5:     { key: 'downloads', path: 'downloads/', label: 'Téléchargements', symbol: '↓' }
6:   ] as const;
7: </script>
8: 
```

## E13 — apps/web/src/lib/player/PlayerControls.svelte:978

Motif recherché : `aria`. Extrait statique ; ne constitue pas une exécution.

```text
976: 
977: 
978:     const target = panel?.querySelector<HTMLButtonElement>('[role="menuitemradio"][aria-checked="true"]:not(:disabled)') ?? items[0];
979: 
980: 
981: 
982:     if (target) {
983: 
984: 
```

## E14 — crates/core/src/server_settings.rs:19

Motif recherché : `welcome_message`. Extrait statique ; ne constitue pas une exécution.

```text
17: pub struct Settings {
18:     pub server_name: String,
19:     pub welcome_message: String,
20:     #[serde(default = "default_sso_button_label")]
21:     pub sso_button_label: String,
22:     #[serde(default = "default_block_sso_local")]
23:     pub block_sso_local_accounts: bool,
24:     #[serde(default = "default_true")]
25:     pub trickplay_enabled: bool,
```

## E15 — crates/core/src/system_tasks.rs:6

Motif recherché : `status`. Extrait statique ; ne constitue pas une exécution.

```text
4:     Json, Router,
5:     extract::{DefaultBodyLimit, Path, State},
6:     http::{HeaderMap, StatusCode},
7:     routing::{get, post, put},
8: };
9: use serde::{Deserialize, Serialize};
10: use std::{
11:     path::PathBuf,
12:     sync::Arc,
```

## E16 — apps/web/src/lib/auth/session.ts:6

Motif recherché : `generation`. Extrait statique ; ne constitue pas une exécution.

```text
4: export interface Session { user: AuthUser; csrfToken: string }
5: 
6: type SessionGenerationListener = (generation: number, current: Session | null) => void;
7: 
8: let currentSession: Session | null = null;
9: let sessionGeneration = 0;
10: let authActionSeq = 0;
11: let manualOverrideSeq = 0;
12: let authMutationQueue: Promise<void> = Promise.resolve();
```

## F01 — apps/web/package.json:2

Motif recherché : `name`. Extrait statique ; ne constitue pas une exécution.

```text
1: {
2:   "name": "media-web",
3:   "private": true,
4:   "type": "module",
5:   "scripts": {
6:     "dev": "vite --host 127.0.0.1",
7:     "check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json",
8:     "build": "vite build && node scripts/externalize-bootstrap.mjs",
```

## F02 — apps/web/package.json:2

Motif recherché : `name`. Extrait statique ; ne constitue pas une exécution.

```text
1: {
2:   "name": "media-web",
3:   "private": true,
4:   "type": "module",
5:   "scripts": {
6:     "dev": "vite --host 127.0.0.1",
7:     "check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json",
8:     "build": "vite build && node scripts/externalize-bootstrap.mjs",
```

## F03 — apps/web/package.json:2

Motif recherché : `name`. Extrait statique ; ne constitue pas une exécution.

```text
1: {
2:   "name": "media-web",
3:   "private": true,
4:   "type": "module",
5:   "scripts": {
6:     "dev": "vite --host 127.0.0.1",
7:     "check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json",
8:     "build": "vite build && node scripts/externalize-bootstrap.mjs",
```

## F04 — apps/web/src/lib/offline-vault.ts:4

Motif recherché : `export`. Extrait statique ; ne constitue pas une exécution.

```text
2: // legacy passphrase downloads remain readable without rewriting their data.
3: import { downloadUrl } from './download-url';
4: export const CHUNK_SIZE=1024*1024;
5: export interface OfflineItem { id:string; salt:Uint8Array; size:number; chunks:number; complete:boolean; metadata?:{iv:Uint8Array;data:ArrayBuffer}; createdAt:number; deviceKey?:boolean }
6: export interface OfflineDetails { title:string; mime:string; size:number }
7: export interface EncryptedChunk { iv:Uint8Array; data:ArrayBuffer }
8: let database:Promise<IDBDatabase>|undefined;
9: let deviceDatabase:Promise<IDBDatabase>|undefined;
10: function open(name:string,upgrade:(database:IDBDatabase)=>void):Promise<IDBDatabase>{return new Promise((resolve,reject)=>{const request=indexedDB.open(name,1);request.onupgradeneeded=()=>upgrade(request.result);request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);});}
```

## F05 — crates/core/src/lib.rs:2

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Local Core: persistent authentication and isolated synthetic media inventory.
2: pub mod account;
3: pub mod admin_backups;
4: pub mod admin_users;
5: pub mod api_keys;
6: pub mod auth;
7: pub mod automation;
8: pub mod automation_execution;
```

## F06 — crates/core/src/account.rs:11

Motif recherché : `router`. Extrait statique ; ne constitue pas une exécution.

```text
9: };
10: use axum::{
11:     Json, Router,
12:     extract::{DefaultBodyLimit, Path, State},
13:     http::{HeaderMap, StatusCode},
14:     routing::{get, put},
15: };
16: use serde::Deserialize;
17: use serde_json::{Value, json};
```

## F07 — crates/node/src/probe.rs:103

Motif recherché : `codec`. Extrait statique ; ne constitue pas une exécution.

```text
101:     }
102:     // Validate the bounded native extractor during discovery, never infer its
103:     // support from codec names or ffprobe stream ordinals alone.
104:     if let Ok(Ok(ids)) =
105:         tokio::task::spawn_blocking(move || crate::mp4_audio::supported_tracks(native_file)).await
106:     {
107:         result
108:             .format
109:             .insert("native_opus_tracks_v1".into(), serde_json::json!(ids));
```

## F08 — crates/node/src/video_stream.rs:1

Motif recherché : `copy`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Confined, cancellable AV1 packet-copy streaming with bounded backpressure.
2: //! A single fragmented MP4 timeline avoids independent GOP overlap at joins.
3: use crate::{BoxStreamBody, OpenedFile};
4: use bytes::Bytes;
5: use http::{Method, Request, Response, StatusCode, header};
6: use http_body_util::BodyExt;
7: use std::{io, process::Stdio, sync::Arc, time::Duration};
```

## F09 — crates/core/src/libraries.rs:29

Motif recherché : `kind`. Extrait statique ; ne constitue pas une exécution.

```text
27:     pub id: String,
28:     pub name: String,
29:     pub kind: String,
30:     pub node_id: String,
31:     pub root: String,
32:     #[sqlx(skip)]
33:     pub folders: Vec<LibraryFolder>,
34:     #[sqlx(try_from = "crate::database::DbBool")]
35:     pub enabled: bool,
```

## F10 — crates/core/src/libraries.rs:29

Motif recherché : `kind`. Extrait statique ; ne constitue pas une exécution.

```text
27:     pub id: String,
28:     pub name: String,
29:     pub kind: String,
30:     pub node_id: String,
31:     pub root: String,
32:     #[sqlx(skip)]
33:     pub folders: Vec<LibraryFolder>,
34:     #[sqlx(try_from = "crate::database::DbBool")]
35:     pub enabled: bool,
```

## F11 — crates/core/src/lib.rs:2

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Local Core: persistent authentication and isolated synthetic media inventory.
2: pub mod account;
3: pub mod admin_backups;
4: pub mod admin_users;
5: pub mod api_keys;
6: pub mod auth;
7: pub mod automation;
8: pub mod automation_execution;
```

## F12 — crates/core/src/lib.rs:2

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Local Core: persistent authentication and isolated synthetic media inventory.
2: pub mod account;
3: pub mod admin_backups;
4: pub mod admin_users;
5: pub mod api_keys;
6: pub mod auth;
7: pub mod automation;
8: pub mod automation_execution;
```

## F13 — docker-bake.ci.hcl:17

Motif recherché : `platforms`. Extrait statique ; ne constitue pas une exécution.

```text
15:   context = "."
16:   dockerfile = "Dockerfile"
17:   platforms = ["linux/amd64"]
18:   args = {
19:     GIT_BRANCH = GIT_BRANCH
20:     MEDIA_VERSION = MEDIA_VERSION
21:     CARGO_BUILD_JOBS = "2"
22:   }
23:   cache-from = ["type=gha,scope=publish-images", "type=gha,scope=ci-images", "type=gha,scope=runtime", "type=gha,scope=runtime-core", "type=gha,scope=runtime-node"]
```

## F14 — docs/API.md:1

Motif recherché : `api/v1`. Extrait statique ; ne constitue pas une exécution.

```text
1: > Moteur de tâches (25 octobre 2026) : API administrateur persistante `GET/POST /api/v1/admin/tasks/jobs`, détail `GET /api/v1/admin/tasks/jobs/{id}` et contrôles `POST /api/v1/admin/tasks/jobs/{id}/{pause|resume|cancel}`. Session administrateur et CSRF pour les mutations ; catégories scan/analyze/prepare/image et cibles inventoriées. Contrat, limites et validation : [tâches distribuées](../../DISTRIBUTED_TASKS.md).
2: 
3: > Bibliothèques avec plusieurs dossiers/nœuds : [contrat du 22 octobre](../../LIBRARY_FOLDERS.md).
4: > Navigation par bibliothèque et images multiples : [contrat du 21 octobre](../../RESPONSIVENESS_AND_ARTWORK.md).
5: 
6: > Administration du 19 octobre 2026 : comptes [ADMIN_USERS.md](../../ADMIN_USERS.md),
7: > groupes et clés API [ADMIN_ACCESS.md](../../ADMIN_ACCESS.md), planifications
```

## F15 — crates/extension-sdk/src/lib.rs:7

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
5: use serde::{Deserialize, Serialize};
6: use std::collections::BTreeMap;
7: pub mod storage;
8: 
9: pub const API_VERSION: u32 = 1;
10: pub const MAX_MODULE: usize = 262_144;
11: pub const MAX_JSON: usize = 65_536;
12: 
13: /// Generic ABI envelope. Host adapters authorize operations before invocation.
```

## G01 — crates/core/src/task_engine.rs:21

Motif recherché : `lease`. Extrait statique ; ne constitue pas une exécution.

```text
19: // PostgreSQL admission must serialize across Core processes. The local mutex only
20: // protects one process; this transaction lock covers node, library and provider
21: // capacity checks together until their leases are committed.
22: pub(crate) const TASK_ADMISSION_LOCK: i64 = 0x4d45444941544153;
23: 
24: #[derive(Clone, Debug, Serialize, sqlx::FromRow)]
25: #[serde(rename_all = "camelCase")]
26: pub struct Job {
27:     pub id: String,
```

## G02 — crates/core/src/nodes.rs:2

Motif recherché : `relay`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Distributed Media Node management, mutual HMAC authentication, remote scan ingestion,
2: //! and bounded Core relay streaming for Milestone P2.
3: 
4: use crate::{
5:     auth::AuthState,
6:     error,
7:     identity::{IdentityStore, Role},
8:     inventory,
```

## G03 — crates/core/tests/multi_node_scale_and_failover.rs:146

Motif recherché : `async fn`. Extrait statique ; ne constitue pas une exécution.

```text
144: }
145: 
146: async fn send_oneshot(
147:     router: &Router,
148:     method: Method,
149:     uri: &str,
150:     cookie: &str,
151:     csrf: Option<&str>,
152:     body: Option<Value>,
```

## G04 — crates/node/src/inventory.rs:8

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
6:     probe,
7: };
8: pub use media_contracts::inventory::{Descriptor, LanguageRemux, RepresentativeCase};
9: use std::{
10:     io,
11:     path::{Path, PathBuf},
12: };
13: 
14: // Explicit synthetic inventory for the fixture bench. These are stable fixture IDs,
```

## G05 — crates/node/src/sidecar_manifest.rs:60

Motif recherché : `manifest.version`. Extrait statique ; ne constitue pas une exécution.

```text
58:     let mut manifest: Manifest =
59:         serde_json::from_slice(bytes).map_err(|_| "invalid media association manifest")?;
60:     if manifest.version != 1 || manifest.audio.len() > 8 || manifest.subtitles.len() > 64 {
61:         return Err("unsupported manifest version or track limit".into());
62:     }
63:     let mut files = HashSet::new();
64:     for track in &mut manifest.audio {
65:         if !filename(&track.file)
66:             || !files.insert(track.file.to_ascii_lowercase())
```

## G06 — crates/node/src/scanner.rs:66

Motif recherché : `obu`. Extrait statique ; ne constitue pas une exécution.

```text
64:             if p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
65:                 [
66:                     "mkv", "mp4", "webm", "m4v", "mov", "obu", "opus", "ogg", "srt", "vtt", "ass",
67:                     "ssa",
68:                 ]
69:                 .iter()
70:                 .any(|e| s.eq_ignore_ascii_case(e))
71:             }) {
72:                 if sidecar_paths.len() >= 10000 {
```

## G07 — crates/core/src/automation.rs:39

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
37: );
38: 
39: pub struct EventInput<'a> {
40:     pub key: &'a str,
41:     pub kind: &'a str,
42:     pub library_id: &'a str,
43:     pub asset_id: &'a str,
44:     pub fingerprint: &'a str,
45:     pub payload: &'a Value,
```

## G08 — crates/core/src/progress.rs:19

Motif recherché : `activity`. Extrait statique ; ne constitue pas une exécution.

```text
17: pub fn router(auth: AuthState) -> Router {
18:     Router::new()
19:         .route("/api/v1/admin/activity", get(activity))
20:         .route(
21:             "/api/v1/account/playback/{id}",
22:             axum::routing::put(heartbeat).delete(stop),
23:         )
24:         .route("/api/v1/progress", get(list))
25:         .route("/api/v1/progress/{work}", get(read).put(write))
```

## G09 — crates/core/src/postgres_backup.rs:7

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
5: 
6: #[derive(Debug, thiserror::Error)]
7: pub enum PostgresBackupError {
8:     #[error("invalid PostgreSQL connection configuration")]
9:     Configuration,
10:     #[error("backup path rejected")]
11:     Path,
12:     #[error("backup target already exists")]
13:     TargetExists,
```

## G10 — crates/core/src/lib.rs:2

Motif recherché : `pub`. Extrait statique ; ne constitue pas une exécution.

```text
1: //! Local Core: persistent authentication and isolated synthetic media inventory.
2: pub mod account;
3: pub mod admin_backups;
4: pub mod admin_users;
5: pub mod api_keys;
6: pub mod auth;
7: pub mod automation;
8: pub mod automation_execution;
```

## G11 — apps/web/src/lib/admin/AdvancedLibraryOptions.svelte:26

Motif recherché : `similar-providers`. Extrait statique ; ne constitue pas une exécution.

```text
24:   <ProviderPriority id="metadata-providers" label="Métadonnées" bind:value={options.metadataProviders} suggestions={providers} />
25:   <ProviderPriority id="image-providers" label="Images" bind:value={options.imageProviders} suggestions={providers} />
26:   <ProviderPriority id="similar-providers" label="Articles similaires (moteur à venir)" bind:value={options.similarItemsProviders} suggestions={providers} />
27:   <label for="online-refresh">Actualiser les métadonnées depuis Internet</label>
28:   <select id="online-refresh" bind:value={options.onlineRefreshIntervalDays}><option value={0}>Jamais automatiquement</option><option value={30}>Tous les 30 jours</option><option value={60}>Tous les 60 jours</option><option value={90}>Tous les 90 jours</option></select>
29:   <p class="hint">Les fournisseurs installés et autorisés actualisent les champs automatiques à cette cadence. Les corrections manuelles restent prioritaires. Cette cadence est distincte de l’actualisation des NFO locaux.</p>
30: </details>
31: 
32: {#if kind !== 'music'}
```

## G12 — crates/core/tests/automation_flow.rs:2019

Motif recherché : `extreme_scale`. Extrait statique ; ne constitue pas une exécution.

```text
2017: 
2018: #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
2019: async fn extreme_scale_100k_and_naive_vs_optimized_comparison() {
2020:     let temp = tempfile::tempdir().unwrap();
2021:     let path = temp.path().join("scale-benchmark.sqlite");
2022:     let store = IdentityStore::open(&path).await.unwrap();
2023:     setup(&store).await;
2024: 
2025:     // Simulate 5,000 works representing a multi-series / multi-album library (e.g. 50 distinct TV series or albums, 100 episodes/tracks each)
```

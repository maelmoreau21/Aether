<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { session, getSessionGeneration, onSessionGenerationChange } from '../auth/session';
  import { statisticsRequest, duration, eventLabels, type Statistics, type Detail, type Distribution } from './api';
  let { administrator = false, active = true }: { administrator?: boolean; active?: boolean } = $props();
  const endpoint = $derived(administrator ? '/api/v1/admin/statistics' : '/api/v1/account/statistics');
  let mounted = $state(false), loading = $state(false), saving = $state(false), error = $state(''), message = $state('');
  let data = $state<Statistics | null>(null), detail = $state<Detail | null>(null), detailLoading = $state(false);
  let dialog: HTMLDialogElement;
  const today = new Date().toISOString().slice(0, 10);
  let from = $state(new Date(Date.now() - 29 * 86400000).toISOString().slice(0, 10)), to = $state(today);
  let userId = $state(''), libraryId = $state(''), nodeId = $state(''), kind = $state(''), device = $state(''), rank = $state('users'), offset = $state(0);
  let enabled = $state(false), eventDays = $state(365), summaryDays = $state(0), dirty = $state(false), eraseConfirmed = $state(false);
  let facets = $state<Statistics['facets']>({ libraries: [], nodes: [], devices: [] });
  let users = $state<Statistics['users']>([]);
  let pending: AbortController | undefined, detailPending: AbortController | undefined;
  let requestNumber = 0;
  let nextExportOffset = $state<number | null>(null);
  const statuses: Record<string, string> = { active: 'En cours', completed: 'Terminée', partial: 'Partielle', interrupted: 'Interrompue' };
  const metricLabels: Record<string, string> = { pauses: 'Pauses', resumes: 'Reprises après pause', additionalSessions: 'Sessions supplémentaires par média et personne', musicSkips: 'Titres passés manuellement', seeksForward: 'Déplacements avant', seeksBackward: 'Déplacements arrière', audioChanges: 'Changements audio', subtitleChanges: 'Changements de sous-titres', speedChanges: 'Changements de vitesse', introSkips: 'Introductions passées', creditsSkips: 'Génériques passés', otherSkips: 'Résumés / aperçus passés', bufferingCount: 'Chargements', errors: 'Erreurs' };
  function date(value: number) { return new Date(value * 1000).toLocaleString('fr-FR'); }
  function params() {
    const values = new URLSearchParams({ from: String(Date.parse(`${from}T00:00:00Z`) / 1000), to: String(Date.parse(`${to}T00:00:00Z`) / 1000 + 86400), rank, offset: String(offset) });
    for (const [key, value] of Object.entries({ userId, libraryId, nodeId, kind, device })) if (value) values.set(key, value);
    return values.toString();
  }
  async function load() {
    const generation = getSessionGeneration(), current = ++requestNumber;
    pending?.abort(); pending = new AbortController(); loading = true; error = '';
    try {
      const result: Statistics = await (await statisticsRequest(`${endpoint}?${params()}`, { signal: pending.signal })).json();
      if (current !== requestNumber || generation !== getSessionGeneration()) return;
      data = result;
      if (!dirty) { enabled = result.enabled; eventDays = result.eventRetentionDays; summaryDays = result.summaryRetentionDays; }
      if (!userId) users = result.users;
      if (!libraryId && !nodeId && !device) facets = result.facets;
    } catch (cause) {
      if (current === requestNumber && generation === getSessionGeneration() && !(cause instanceof DOMException && cause.name === 'AbortError')) { data = null; error = cause instanceof Error ? cause.message : 'Statistiques indisponibles.'; }
    } finally { if (current === requestNumber) loading = false; }
  }
  async function save() {
    saving = true; message = ''; error = '';
    try {
      await statisticsRequest(`${endpoint}/settings`, { method: 'PUT', body: JSON.stringify({ enabled, eventRetentionDays: eventDays, summaryRetentionDays: summaryDays }) });
      dirty = false; message = 'Paramètres enregistrés.'; await load();
      window.dispatchEvent(new Event('statistics-settings-changed'));
    } catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { saving = false; }
  }
  async function cleanup() {
    saving = true; message = ''; error = '';
    try { const result = await (await statisticsRequest(`${endpoint}/purge`, { method: 'POST' })).json(); message = `${result.eventsRemoved} événement(s) expiré(s) supprimé(s). Relancez si le lot atteint ${result.batchLimit}.`; await load(); }
    catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { saving = false; }
  }
  async function erase() {
    if (!eraseConfirmed) return;
    saving = true; error = '';
    try { await statisticsRequest(endpoint, { method: 'DELETE' }); eraseConfirmed = false; message = 'Votre historique statistique a été supprimé. Les futures lectures seront collectées si le module reste actif.'; await load(); }
    catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { saving = false; }
  }
  async function openDetail(id: string) {
    const generation = getSessionGeneration();
    detailPending?.abort(); const controller = new AbortController(); detailPending = controller;
    detail = null; detailLoading = true; dialog.showModal();
    try { const result = await (await statisticsRequest(`${endpoint}/sessions/${encodeURIComponent(id)}`, { signal: controller.signal })).json(); if (generation === getSessionGeneration() && detailPending === controller) detail = result; }
    catch (cause) { if (!controller.signal.aborted && generation === getSessionGeneration()) { error = String(cause instanceof Error ? cause.message : cause); dialog.close(); } }
    finally { if (detailPending === controller) detailLoading = false; }
  }
  async function moreEvents() {
    if (!detail?.nextSequence) return;
    const previous = detail, generation = getSessionGeneration(); detailLoading = true;
    try {
      const next: Detail = await (await statisticsRequest(`${endpoint}/sessions/${encodeURIComponent(previous.session.id)}?after=${previous.nextSequence}`)).json();
      if (generation === getSessionGeneration() && detail?.session.id === previous.session.id) detail = { ...next, events: [...previous.events, ...next.events] };
    } catch (cause) { error = String(cause instanceof Error ? cause.message : cause); } finally { detailLoading = false; }
  }
  async function download(exportOffset = 0) {
    error = ''; message = '';
    const generation = getSessionGeneration();
    try {
      const query = new URLSearchParams(params()); query.set('offset', String(exportOffset));
      const response = await statisticsRequest(`${endpoint}/export?${query}`);
      const blob = await response.blob(); if (generation !== getSessionGeneration()) return;
      const url = URL.createObjectURL(blob), anchor = document.createElement('a');
      anchor.href = url; anchor.download = `statistiques-${Math.floor(exportOffset / 2000) + 1}.csv`; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
      nextExportOffset = response.headers.get('x-export-truncated') === 'true' ? exportOffset + 2000 : null;
      if (nextExportOffset !== null) message = 'Un lot de 2 000 sessions a été exporté. Téléchargez le lot suivant pour continuer, ou réduisez la période.';
    } catch (cause) { error = String(cause instanceof Error ? cause.message : cause); }
  }
  onMount(() => {
    mounted = true;
    const timer = setInterval(() => { if (active && !loading && !dirty && document.visibilityState === 'visible') void load(); }, 15000);
    const off = onSessionGenerationChange(() => { requestNumber++; pending?.abort(); detailPending?.abort(); data = null; detail = null; users = []; facets = { libraries: [], nodes: [], devices: [] }; dialog?.close(); });
    return () => { clearInterval(timer); off(); pending?.abort(); detailPending?.abort(); };
  });
  $effect(() => { const account = $session?.user.id; if (mounted && active && account) untrack(() => { void load(); }); });
</script>

<section class="statistics" aria-label={administrator ? 'Statistiques du serveur' : 'Mes statistiques'}>
  <header class="heading"><div><p class="eyebrow">{administrator ? 'Activité du serveur' : 'Votre activité'}</p>{#if administrator}<h2>Analyse des lectures</h2>{:else}<h1>Mes statistiques</h1>{/if}<p>Films, séries et musique · données conservées sur ce serveur.</p></div><button onclick={() => void load()} disabled={loading}>{loading ? 'Chargement…' : 'Actualiser'}</button></header>
  {#if error}<p class="notice error" role="alert">{error}</p>{/if}
  {#if message}<p class="notice" role="status">{message}</p>{/if}
  {#if administrator && data}
    <details class="panel settings"><summary>Collecte et conservation</summary><form onsubmit={event => { event.preventDefault(); void save(); }}>
      <label class="check"><input type="checkbox" bind:checked={enabled} onchange={() => dirty = true} /> Activer les statistiques détaillées</label>
      <p>Les utilisateurs sont informés. La désactivation arrête la collecte et conserve les bilans existants.</p>
      <div class="filters"><label>Détails, en jours<input type="number" min="1" max="3650" bind:value={eventDays} oninput={() => dirty = true} required /></label><label>Bilans, en jours · 0 = illimité<input type="number" min="0" max="36500" bind:value={summaryDays} oninput={() => dirty = true} required /></label></div>
      <div class="actions"><button class="primary" disabled={saving}>Enregistrer</button><button type="button" onclick={() => void cleanup()} disabled={saving}>Purger les données expirées</button></div>
    </form></details>
  {/if}
  {#if data}
    <p class="collection-state">Collecte {data.enabled ? 'active' : 'désactivée'}{#if data.firstEnabledAt} depuis le {date(data.firstEnabledAt)}{:else} · aucun historique collecté{/if}. Détails : {data.eventRetentionDays} jours ; bilans : {data.summaryRetentionDays ? `${data.summaryRetentionDays} jours` : 'sans expiration'}.</p>
  {/if}
  <form class="panel filter-form" onsubmit={event => { event.preventDefault(); offset = 0; nextExportOffset = null; void load(); }}>
    <div class="filters"><label>Du · UTC<input type="date" bind:value={from} max={to} required /></label><label>Au · UTC<input type="date" bind:value={to} min={from} max={today} required /></label>
      {#if administrator}<label>Utilisateur<select bind:value={userId}><option value="">Tous</option>{#each users as user (user.id)}<option value={user.id}>{user.name}</option>{/each}</select></label>{/if}
      <label>Type<select bind:value={kind}><option value="">Tous</option><option value="film">Films</option><option value="episode">Épisodes</option><option value="music">Musique</option></select></label>
      <label>Bibliothèque<select bind:value={libraryId}><option value="">Toutes</option>{#each facets.libraries as library (library.id)}<option value={library.id}>{library.name}</option>{/each}</select></label>
      <label>Media Node<select bind:value={nodeId}><option value="">Tous</option>{#each facets.nodes as node (node)}<option>{node}</option>{/each}</select></label>
      <label>Appareil<select bind:value={device}><option value="">Tous</option>{#each facets.devices as item (item)}<option>{item}</option>{/each}</select></label>
      <label>Classement<select bind:value={rank}><option value="users">Spectateurs uniques</option><option value="seconds">Temps consommé</option><option value="completion">Taux de complétion</option><option value="favorites">Favoris</option><option value="rating">Notes</option></select></label>
    </div><div class="actions"><button class="primary" disabled={loading}>Appliquer les filtres</button><button type="button" onclick={() => void download()} disabled={!data}>Exporter en CSV</button>{#if nextExportOffset !== null}<button type="button" onclick={() => void download(nextExportOffset ?? 0)}>Exporter le lot suivant</button>{/if}</div>
  </form>
  <p class="muted">Les dates filtrent le début des sessions. Les durées et classements portent sur ces sessions ; les horaires et journées sont exprimés en UTC.</p>
  {#if data}
    <div class="kpis">{#each [[duration(data.summary.seconds), 'Temps consommé'], [data.summary.plays, 'Lectures ouvertes'], [data.summary.users, 'Utilisateurs'], [data.summary.media, 'Médias distincts'], [data.summary.completed, 'Lectures terminées'], [data.summary.active, 'En cours']] as [value, label] (label)}<article class="panel"><strong>{value}</strong><span>{label}</span></article>{/each}</div>
    {#if !data.summary.plays}<p class="panel empty">Aucune lecture dans cette période. L’activité antérieure à l’activation des statistiques ne peut pas être reconstruite.</p>{/if}
    <div class="columns">
      <section class="panel"><h2>Temps consommé par jour</h2><p class="muted">Temps de lecture effectif, hors pauses et chargements. Journées UTC.</p><div class="chart" role="img" aria-label="Temps consommé quotidien">
        {#each data.daily.slice(-90) as day (day.day)}<div class="chart-column" title={`${new Date(day.day * 1000).toLocaleDateString('fr-FR')}: ${duration(day.seconds)}`}><svg viewBox="0 0 12 100" preserveAspectRatio="none" aria-hidden="true"><rect x="1" width="10" y={100 - day.seconds / Math.max(1, ...data.daily.map(d => d.seconds)) * 100} height={day.seconds / Math.max(1, ...data.daily.map(d => d.seconds)) * 100} rx="2" /></svg></div>{/each}
      </div><details><summary>Valeurs quotidiennes{data.daily.length > 90 ? ' · graphique : 90 derniers jours renseignés' : ''}</summary><ul>{#each data.daily as day (day.day)}<li>{new Date(day.day * 1000).toLocaleDateString('fr-FR')} : {duration(day.seconds)}</li>{/each}</ul></details></section>
      <section class="panel"><h2>Qualité de lecture</h2><dl><dt>Démarrage moyen observé</dt><dd>{Math.round(data.behavior.startupMs)} ms</dd><dt>Chargements</dt><dd>{data.behavior.bufferingCount}</dd><dt>Temps de chargement</dt><dd>{duration(data.behavior.bufferingSeconds)}</dd><dt>Erreurs</dt><dd>{data.behavior.errors}</dd></dl><p class="muted">Une interruption ne signifie pas que le média déplaît. Une fermeture sans dernier signal laisse une fin inconnue.</p></section>
    </div>
    <section class="panel"><h2>Médias les plus regardés et écoutés</h2><p class="muted">Popularité et appréciation sont séparées. Notes et favoris correspondent à leur état actuel.</p><div class="table-wrap"><table><thead><tr><th>Média</th><th>Lectures</th><th>Personnes</th><th>Temps consommé</th><th>Terminées</th><th>Favoris</th><th>Note / 5</th></tr></thead><tbody>{#each data.popular as media (`${media.kind}:${media.id}`)}<tr><td>{media.title}<small>{media.kind === 'music' ? 'Musique' : media.kind === 'episode' ? 'Épisode' : 'Film'}</small></td><td>{media.plays}</td><td>{media.users}</td><td>{duration(media.seconds)}</td><td>{media.completed} · {Math.round(media.completed / media.plays * 100)} %</td><td>{media.favorites}</td><td>{media.rating ? media.rating.toFixed(1) : '—'}</td></tr>{/each}</tbody></table></div></section>
    <div class="columns"><section class="panel"><h2>Comportement</h2><dl>{#each Object.entries(metricLabels) as [key, label] (key)}<dt>{label}</dt><dd>{data.behavior[key]}</dd>{/each}</dl></section><section class="panel"><h2>Horaires de démarrage · UTC</h2><p class="muted">Lundi à dimanche, heures de 0 à 23.</p><div class="heatmap" role="img" aria-label="Lectures par jour de la semaine et heure UTC">{#each Array.from({ length: 168 }, (_, i) => i) as hour (hour)}{@const cell = data.hours.find(h => h.hour === hour)}<span title={`${['Lundi', 'Mardi', 'Mercredi', 'Jeudi', 'Vendredi', 'Samedi', 'Dimanche'][Math.floor(hour / 24)]} ${hour % 24} h : ${cell?.plays ?? 0} lecture(s)`} class:filled={!!cell}><svg viewBox="0 0 10 10" aria-hidden="true"><rect width="10" height="10" opacity={cell ? .25 + .75 * cell.plays / Math.max(1, ...data.hours.map(h => h.plays)) : .04} /></svg></span>{/each}</div></section></div>
    <div class="columns">{@render distribution('Audio initial', data.distributions.initialAudio ?? [])}{@render distribution('Temps écouté par langue audio', data.distributions.audioTime ?? [])}{@render distribution('Temps avec sous-titres', data.distributions.subtitlesTime ?? [])}{@render distribution('Temps par méthode de lecture', data.distributions.methodTime ?? [])}{@render distribution('Artistes écoutés', data.music.artist ?? [])}{@render distribution('Albums écoutés', data.music.album ?? [])}</div>
    <section class="panel"><h2>Historique et lectures en cours</h2><div class="table-wrap"><table><thead><tr><th>Date</th>{#if administrator}<th>Utilisateur</th>{/if}<th>Média</th><th>Appareil</th><th>Temps consommé</th><th>Couverture</th><th>État</th><th>Détail</th></tr></thead><tbody>{#each data.sessions as item (item.id)}<tr><td>{date(item.startedAt)}</td>{#if administrator}<td>{item.user}</td>{/if}<td>{item.title}{#if item.artist}<small>{item.artist} · {item.album}</small>{/if}</td><td>{item.device}</td><td>{duration(item.seconds)}</td><td>{item.duration ? `${Math.min(100, Math.round(item.coveredSeconds / item.duration * 100))} %` : 'Inconnue'}</td><td>{statuses[item.status] ?? item.status}</td><td><button onclick={() => void openDetail(item.id)} aria-label={`Détail de ${item.title}`}>Voir</button></td></tr>{/each}</tbody></table></div><div class="actions"><button disabled={offset === 0 || loading} onclick={() => { offset = Math.max(0, offset - 50); void load(); }}>Précédent</button><span>Page {Math.floor(offset / 50) + 1}</span><button disabled={!data.hasMore || loading} onclick={() => { offset += 50; void load(); }}>Suivant</button></div></section>
    {#if administrator}<section class="panel"><h2>Activité des utilisateurs</h2><div class="table-wrap"><table><thead><tr><th>Utilisateur</th><th>Lectures</th><th>Temps consommé</th></tr></thead><tbody>{#each data.users as user (user.id)}<tr><td>{user.name}</td><td>{user.plays}</td><td>{duration(user.seconds)}</td></tr>{/each}</tbody></table></div></section>{/if}
    <details class="panel"><summary>Périodes de collecte et limites</summary><p>Les statistiques commencent à l’activation. Les périodes sans collecte et les clients non instrumentés ne sont pas estimés. Les lectures terminées couvrent au moins 90 % des portions du média ; avancer jusqu’à la fin ne suffit pas.</p><ul>{#each data.periods as period, index (`${period.startedAt}-${index}`)}<li>{date(period.startedAt)} → {period.endedAt ? date(period.endedAt) : 'en cours'}</li>{/each}</ul><p>Aucune adresse IP, géolocalisation ou empreinte d’appareil ajoutée. Les données restent dans la base de ce serveur.</p></details>
    {#if !administrator}<details class="panel"><summary>Supprimer mon historique statistique</summary><p>Cette suppression inclut les sessions, leurs événements et leurs contributions aux bilans. Vos progressions, notes et favoris sont conservés.</p><label class="check"><input type="checkbox" bind:checked={eraseConfirmed} /> Je souhaite supprimer mon historique statistique.</label><button disabled={!eraseConfirmed || saving} onclick={() => void erase()}>Supprimer mon historique</button></details>{/if}
  {/if}
</section>
{#snippet distribution(title: string, rows: Distribution[])}<section class="panel"><h2>{title}</h2>{#if rows.length}<dl>{#each rows.slice(0, 12) as row (row.label)}<dt>{row.label || 'Non renseigné'}</dt><dd>{row.plays} · {duration(row.seconds)}</dd>{/each}</dl>{:else}<p class="muted">Aucune donnée.</p>{/if}</section>{/snippet}
<dialog bind:this={dialog} class="session-dialog" onclose={() => { detailPending?.abort(); detail = null; }}><header class="heading"><h2>{detail?.session.title ?? 'Détail de la session'}</h2><button onclick={() => dialog.close()}>Fermer</button></header>{#if detailLoading}<p role="status">Chargement…</p>{:else if detail}<p>{date(detail.session.startedAt)} · {detail.session.device} · {statuses[detail.session.status]}</p><dl><dt>Temps consommé</dt><dd>{duration(detail.session.seconds)}</dd><dt>Portion parcourue</dt><dd>{duration(detail.session.coveredSeconds)} / {duration(detail.session.duration)}</dd><dt>Fin</dt><dd>{detail.session.endReason || 'Lecture en cours'}</dd></dl>{#if detail.detailsExpired}<p>Les événements détaillés ont expiré ou ont été purgés. Le bilan est conservé.</p>{/if}<ol class="timeline">{#each detail.events as item (item.event.sequence)}<li><time>{date(item.occurredAt)}</time><strong>{eventLabels[item.event.kind] ?? item.event.kind}</strong><span>{duration(item.event.position)} · {item.event.origin === 'manual' ? 'Manuel' : item.event.origin === 'sync' ? 'Watch Party' : 'Automatique'}{#if item.event.previousTrack} · {item.event.previousTrack.language} →{/if}{#if item.event.track} {item.event.track.language} ({item.event.track.id}){/if}{#if item.event.fromPosition !== undefined} · depuis {duration(item.event.fromPosition)}{/if}{#if item.event.errorCode} · {item.event.errorCode}{/if}</span></li>{/each}</ol>{#if detail.eventsTruncated}<button onclick={() => void moreEvents()} disabled={detailLoading}>Charger les événements suivants</button>{/if}{/if}</dialog>
<style>
  .statistics{display:grid;gap:22px;color:var(--color-text);width:100%;max-width:var(--maxw);margin:0 auto}.heading{display:flex;justify-content:space-between;align-items:center;gap:20px}.heading h1{font-size:clamp(1.6rem,3vw,2.4rem);margin:4px 0}.heading p,.muted,.collection-state{color:var(--color-muted);line-height:1.6}.eyebrow{font-size:.75rem;text-transform:uppercase;letter-spacing:.12em}.panel{background:var(--color-surface);border:1px solid var(--color-line);border-radius:var(--radius-lg,16px);padding:22px;min-width:0}.panel h2{font-size:1.05rem;margin:0 0 16px}.panel p{line-height:1.6}.filters{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:16px}label{display:grid;gap:8px;font-size:.85rem}.check{display:flex;align-items:center;gap:10px;margin:16px 0}.actions{display:flex;flex-wrap:wrap;align-items:center;gap:12px;margin-top:20px}.primary{background:var(--color-accent);color:var(--color-canvas)}button{padding:10px 15px;border:1px solid var(--color-line);border-radius:var(--radius-md,10px);background:var(--color-surface);color:var(--color-text);cursor:pointer}button:disabled{opacity:.5;cursor:default}input,select{width:100%;box-sizing:border-box;min-height:42px;padding:9px 12px;background:var(--color-canvas);border:1px solid var(--color-line);color:var(--color-text);border-radius:8px}.check input{width:18px;min-height:18px}summary{cursor:pointer;font-weight:600}.kpis{display:grid;grid-template-columns:repeat(auto-fit,minmax(145px,1fr));gap:14px}.kpis article{display:grid;gap:10px}.kpis strong{font-size:1.55rem}.kpis span{font-size:.8rem;color:var(--color-muted)}.columns{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:20px}dl{display:grid;grid-template-columns:1fr auto;gap:12px;margin:0}dt{color:var(--color-muted);overflow-wrap:anywhere}dd{margin:0;text-align:right;font-variant-numeric:tabular-nums}.table-wrap{overflow:auto}table{width:100%;border-collapse:collapse;font-size:.85rem}th{text-align:left;color:var(--color-muted);font-weight:500}td,th{padding:13px 12px;border-bottom:1px solid var(--color-line);vertical-align:middle}td:first-child,th:first-child{padding-left:0}td small{display:block;color:var(--color-muted);margin-top:4px}.notice{border:1px solid var(--color-line);background:var(--color-surface);padding:16px;border-radius:10px}.error{color:var(--color-error,#f07878)}.empty{text-align:center;color:var(--color-muted);padding:40px}.chart{display:flex;align-items:flex-end;gap:3px;height:145px;margin:24px 0}.chart-column{height:100%;min-width:2px;flex:1}.chart svg{height:100%;width:100%;fill:var(--color-accent)}.heatmap{display:grid;grid-template-columns:repeat(24,1fr);gap:3px}.heatmap span{aspect-ratio:1}.heatmap svg{display:block;width:100%;fill:var(--color-accent)}.session-dialog{width:min(850px,calc(100% - 36px));max-height:85dvh;box-sizing:border-box;background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius:18px;padding:26px}.session-dialog::backdrop{background:#000b;backdrop-filter:blur(8px)}.timeline{list-style:none;padding:0}.timeline li{display:grid;grid-template-columns:145px 150px 1fr;gap:12px;padding:13px 0;border-bottom:1px solid var(--color-line);font-size:.8rem}.timeline time,.timeline span{color:var(--color-muted)}@media(max-width:760px){.columns{grid-template-columns:1fr}.panel{padding:16px}.heading{align-items:flex-start}.timeline li{grid-template-columns:1fr;gap:6px}.kpis{grid-template-columns:repeat(2,minmax(0,1fr))}}
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { applicationRequest, features, loadFeatures, type RemoteServer } from './api';
  import LegacyImport from './LegacyImport.svelte';
  import './applications.css';
  interface Profile { id: number; name: string }
  interface Options { rootFolders: { path: string }[]; qualityProfiles: Profile[]; metadataProfiles: Profile[] }
  interface Location { id: string; name: string; kind: 'films' | 'series' | 'music'; nodeId: string; nodeName?: string; root: string }
  const blank = (): Omit<RemoteServer, 'id'> => ({ name: '', kind: 'radarr', baseUrl: '', libraryId: '', rootFolder: '', qualityProfileId: 0, metadataProfileId: undefined, isDefault: false, enabled: false });
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  let servers = $state<RemoteServer[]>([]);
  let locations = $state<Location[]>([]);
  let editing = $state<string | null>(null);
  let form = $state(blank());
  let key = $state('');
  let autoApprove = $state(false);
  let maxPending = $state(20);
  let options = $state<Options | null>(null);
  let confirmingRemoval = $state<string | null>(null);
  const administrator = $derived($session?.user.role === 'administrator');
  const matchingLocations = $derived(locations.filter(location => location.kind === (form.kind === 'radarr' ? 'films' : form.kind === 'sonarr' ? 'series' : 'music')));
  async function refresh() {
    const [list, settings, destinations] = await Promise.all([
      applicationRequest<{ servers: RemoteServer[] }>('/api/v1/admin/acquisition/servers'),
      applicationRequest<{ autoApprove: boolean; maxPendingPerUser: number }>('/api/v1/admin/acquisition/settings'),
      applicationRequest<{ locations: Location[] }>('/api/v1/admin/acquisition/locations')
    ]);
    servers = list.servers; locations = destinations.locations; autoApprove = settings.autoApprove; maxPending = settings.maxPendingPerUser;
  }
  onMount(() => { void (async () => { try { if (!administrator) return; const available = await loadFeatures(true); if (available.requests) await refresh(); } catch (cause) { error = cause instanceof Error ? cause.message : 'Administration indisponible.'; } finally { loading = false; } })(); });
  function edit(server: RemoteServer) { editing = server.id; form = { name: server.name, kind: server.kind, baseUrl: server.baseUrl, libraryId: server.libraryId, rootFolder: server.rootFolder, qualityProfileId: server.qualityProfileId, metadataProfileId: server.metadataProfileId, isDefault: server.isDefault, enabled: server.enabled }; key = ''; options = null; message = ''; }
  function clear() { editing = null; form = blank(); key = ''; options = null; }
  async function save(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; message = '';
    try {
      const saved = await applicationRequest<{ server: RemoteServer }>(editing ? `/api/v1/admin/acquisition/servers/${encodeURIComponent(editing)}` : '/api/v1/admin/acquisition/servers', { method: editing ? 'PUT' : 'POST', body: JSON.stringify({ ...form, ...(key ? { apiKey: key } : {}) }) });
      key = ''; await refresh(); if (saved.server?.id) edit(saved.server); else clear(); message = 'Connexion enregistrée. Chargez ses dossiers et profils, puis activez-la.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Connexion non enregistrée.'; }
    finally { busy = false; }
  }
  async function test(id: string) {
    busy = true; error = ''; message = '';
    try { await applicationRequest(`/api/v1/admin/acquisition/servers/${encodeURIComponent(id)}/test`, { method: 'POST' }); message = 'Connexion réussie.'; }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Connexion impossible.'; }
    finally { busy = false; }
  }
  async function loadOptions() {
    if (!editing) return; busy = true; error = '';
    try { options = await applicationRequest<Options>(`/api/v1/admin/acquisition/servers/${encodeURIComponent(editing)}/options`); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Profils indisponibles.'; }
    finally { busy = false; }
  }
  async function remove(id: string) {
    busy = true; error = ''; message = '';
    try { await applicationRequest(`/api/v1/admin/acquisition/servers/${encodeURIComponent(id)}`, { method: 'DELETE' }); confirmingRemoval = null; if (editing === id) clear(); await refresh(); message = 'Connexion retirée.'; }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Suppression impossible.'; }
    finally { busy = false; }
  }
  async function saveSettings(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; message = '';
    try { await applicationRequest('/api/v1/admin/acquisition/settings', { method: 'PUT', body: JSON.stringify({ autoApprove, maxPendingPerUser: maxPending }) }); message = 'Règles de validation enregistrées.'; }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Enregistrement impossible.'; }
    finally { busy = false; }
  }
</script>
<div class="application-page">
  <header class="heading"><div><p class="muted">Administration</p><h1>Demandes et acquisitions</h1></div><a class="button-link" href={resolve('requests/')}>Voir les demandes</a></header>
  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}{#if message}<p role="status" class="feedback success">{message}</p>{/if}
  {#if !administrator}<p class="panel">Cette page est réservée aux administrateurs.</p>{:else if loading}<p role="status">Chargement…</p>{:else if !$features.requests && !$features.reports}<p class="panel">Installez et activez Demandes ou Signalements dans l’administration des extensions.</p>{:else}
    <div class="stack">
      {#if $features.requests}
      <section class="panel"><h2>Validation</h2><form class="stack" onsubmit={saveSettings}><label>Traitement des nouvelles demandes<select bind:value={autoApprove}><option value={false}>Validation par un administrateur</option><option value={true}>Validation automatique</option></select></label><label>Maximum de demandes en attente par utilisateur<input type="number" min="1" max="1000" required bind:value={maxPending} /></label><div><button class="primary" disabled={busy}>Enregistrer les règles</button></div></form></section>
      <section><div class="heading"><h2>Serveurs d’acquisition</h2><button disabled={busy} onclick={clear}>Nouvelle connexion</button></div><p class="muted">Chaque demande utilise une destination choisie. L’équilibrage automatique entre serveurs sera ajouté ultérieurement.</p><ul class="records">{#each servers as server (server.id)}<li class="record"><div class="record-header"><div><h3>{server.name}</h3><small>{server.kind} · {server.enabled ? 'Activé' : 'Désactivé'}{server.isDefault ? ' · Par défaut' : ''}</small><p class="muted">{server.baseUrl}</p></div><div class="row"><button disabled={busy} onclick={() => edit(server)}>Modifier</button><button disabled={busy} onclick={() => test(server.id)}>Tester</button><button class="danger" disabled={busy} onclick={() => confirmingRemoval = server.id}>Retirer</button></div></div>{#if confirmingRemoval === server.id}<div class="detail"><p>Retirer cette connexion ? Les demandes existantes ne seront pas transférées vers un autre serveur.</p><div class="row"><button class="danger" disabled={busy} onclick={() => remove(server.id)}>Confirmer le retrait</button><button onclick={() => confirmingRemoval = null}>Conserver</button></div></div>{/if}</li>{:else}<li class="empty">Aucun serveur configuré.</li>{/each}</ul></section>
      <section class="panel stack"><h2>{editing ? 'Modifier la connexion' : 'Ajouter une connexion'}</h2><form class="stack" onsubmit={save}>
        <div class="fields"><label>Nom<input required maxlength="100" bind:value={form.name} /></label><label>Service<select bind:value={form.kind} onchange={() => { form.libraryId = ''; options = null; }}><option value="radarr">Radarr · Films</option><option value="sonarr">Sonarr · Séries</option><option value="lidarr">Lidarr · Musique</option></select></label><label>Adresse du serveur<input required type="url" maxlength="2048" bind:value={form.baseUrl} placeholder="http://serveur:7878" /></label><label>Clé API{editing ? ' (vide pour conserver)' : ''}<input type="password" autocomplete="new-password" required={!editing} maxlength="512" bind:value={key} /></label><label>Emplacement de destination<select required bind:value={form.libraryId}><option value="">Choisir un dossier de bibliothèque</option>{#each matchingLocations as location (location.id)}<option value={location.id}>{location.name} · {location.nodeName ?? location.nodeId} · {location.root}</option>{/each}</select></label></div>
        <p class="muted">Le choix d’une adresse HTTP autorise explicitement cette connexion locale sans chiffrement. L’adresse doit être joignable depuis le Core. Le dossier distant sera associé à la bibliothèque existante ; aucun montage n’est créé automatiquement.</p>
        {#if editing}<div><button type="button" disabled={busy} onclick={loadOptions}>Charger les dossiers et profils</button></div>{/if}
        <div class="fields"><label>Dossier distant{#if options}<select required bind:value={form.rootFolder}><option value="">Choisir un dossier</option>{#each options.rootFolders as folder (folder.path)}<option value={folder.path}>{folder.path}</option>{/each}</select>{:else}<input bind:value={form.rootFolder} maxlength="1024" placeholder="Choisir après le premier enregistrement" />{/if}</label><label>Profil de qualité{#if options}<select bind:value={form.qualityProfileId}><option value={0}>Choisir un profil</option>{#each options.qualityProfiles as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select>{:else}<input type="number" min="0" bind:value={form.qualityProfileId} />{/if}</label>{#if form.kind === 'lidarr'}<label>Profil de métadonnées{#if options}<select bind:value={form.metadataProfileId}><option value={undefined}>Choisir un profil</option>{#each options.metadataProfiles as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select>{:else}<input type="number" min="1" bind:value={form.metadataProfileId} />{/if}</label>{/if}</div>
        <label class="check"><input type="checkbox" bind:checked={form.isDefault} />Destination par défaut pour ce type de média</label><label class="check"><input type="checkbox" bind:checked={form.enabled} />Connexion activée</label><div class="row"><button class="primary" disabled={busy}>Enregistrer</button>{#if editing}<button type="button" disabled={busy} onclick={clear}>Terminer la modification</button>{/if}</div>
      </form></section>
      {/if}
      <LegacyImport {servers} />
    </div>
  {/if}
</div>

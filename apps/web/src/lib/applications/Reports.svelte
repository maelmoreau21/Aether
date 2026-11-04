<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { applicationRequest, categoryLabels, dateLabel, features, loadFeatures, statusLabels, type MediaReport } from './api';
  import './applications.css';
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let reports = $state<MediaReport[]>([]);
  let selected = $state<MediaReport | null>(null);
  let message = $state('');
  let filter = $state('all');
  const isAdmin = $derived($session?.user.role === 'administrator');
  const visible = $derived(reports.filter(item => filter === 'all' || item.status === filter));
  async function refresh() { reports = (await applicationRequest<{ reports: MediaReport[] }>('/api/v1/reports')).reports; }
  onMount(() => { void (async () => { try { const available = await loadFeatures(true); if (available.reports) await refresh(); } catch (cause) { error = cause instanceof Error ? cause.message : 'Signalements indisponibles.'; } finally { loading = false; } })(); });
  async function show(id: string) {
    busy = true; error = ''; message = '';
    try { selected = selected?.id === id ? null : await applicationRequest<MediaReport>(`/api/v1/reports/${encodeURIComponent(id)}`); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Détail indisponible.'; }
    finally { busy = false; }
  }
  async function update(status: string) {
    if (!selected) return; busy = true; error = '';
    try { await applicationRequest(`/api/v1/reports/${encodeURIComponent(selected.id)}/status`, { method: 'POST', body: JSON.stringify({ status }) }); selected = await applicationRequest<MediaReport>(`/api/v1/reports/${encodeURIComponent(selected.id)}`); await refresh(); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Modification impossible.'; }
    finally { busy = false; }
  }
  async function comment(event: SubmitEvent) {
    event.preventDefault(); if (!selected) return; busy = true; error = '';
    try { await applicationRequest(`/api/v1/reports/${encodeURIComponent(selected.id)}/comments`, { method: 'POST', body: JSON.stringify({ message: message.trim() }) }); message = ''; selected = await applicationRequest<MediaReport>(`/api/v1/reports/${encodeURIComponent(selected.id)}`); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Commentaire impossible.'; }
    finally { busy = false; }
  }
</script>
<div class="application-page">
  <header class="heading"><div><p class="muted">Suivi des problèmes de votre médiathèque</p><h1>Signalements</h1><p class="muted">Pour ouvrir un signalement précis, utilisez « Signaler » sur une fiche ou dans le lecteur.</p></div><button disabled={busy || loading || !$features.reports} onclick={() => { busy = true; void refresh().catch(cause => error = String(cause instanceof Error ? cause.message : cause)).finally(() => busy = false); }}>Actualiser</button></header>
  {#if error}<p role="alert" class="feedback error">{error}</p>{/if}
  {#if loading}<p class="empty" role="status">Chargement…</p>{:else if !$features.reports}<p class="panel">L’extension officielle Signalements doit être installée et activée par un administrateur.</p>{:else}
    <div class="heading"><h2>{isAdmin ? 'Tous les signalements' : 'Mes signalements'}</h2><label>État<select bind:value={filter}><option value="all">Tous les états</option>{#each ['open','in_progress','resolved','closed'] as status (status)}<option value={status}>{statusLabels[status]}</option>{/each}</select></label></div>
    <ul class="records">{#each visible as report (report.id)}<li class="record"><div class="record-header"><div><h3>{report.title ?? categoryLabels[report.category] ?? 'Signalement'}</h3><small>{categoryLabels[report.category] ?? report.category} · {dateLabel(report.createdAt)}</small></div><span class="badge">{statusLabels[report.status] ?? report.status}</span></div><p class="text">{report.description}</p><button disabled={busy} onclick={() => show(report.id)}>{selected?.id === report.id ? 'Masquer le suivi' : 'Ouvrir le suivi'}</button>
      {#if selected?.id === report.id}<div class="detail stack">
        {#if selected.positionSeconds !== undefined}<p>Position : {selected.positionSeconds} s{selected.endSeconds !== undefined ? ` → ${selected.endSeconds} s` : ''}</p>{/if}
        {#if selected.assetId}<p class="muted">Copie concernée : {selected.assetId}</p>{/if}
        {#if selected.context}<details><summary>Contexte technique</summary>{#each Object.entries(selected.context).filter(([, value]) => value) as [key, value] (key)}<p class="text">{key} : {value}</p>{/each}</details>{/if}
        <div class="row">{#if isAdmin}{#each ['open','in_progress','resolved','closed'].filter(status => status !== selected?.status) as status (status)}<button disabled={busy} onclick={() => update(status)}>{status === 'open' ? 'Rouvrir' : status === 'in_progress' ? 'Prendre en charge' : status === 'resolved' ? 'Marquer résolu' : 'Fermer sans correction'}</button>{/each}{:else if ['resolved','closed'].includes(selected.status)}<button disabled={busy} onclick={() => update('open')}>Rouvrir</button>{/if}</div>
        <section aria-label="Commentaires"><h3>Échanges</h3>{#each selected.comments ?? [] as entry, itemIndex (itemIndex)}<article><small>{entry.author ?? 'Utilisateur'} · {dateLabel(entry.createdAt)}</small><p class="text">{entry.message}</p></article>{:else}<p class="muted">Aucun commentaire.</p>{/each}<form class="stack" onsubmit={comment}><label>Ajouter un commentaire<textarea maxlength="8000" required bind:value={message}></textarea></label><div><button disabled={busy || !message.trim()}>Envoyer</button></div></form></section>
        {#if selected.history?.length}<details><summary>Historique</summary>{#each selected.history as entry, itemIndex (itemIndex)}<p>{dateLabel(entry.createdAt)} · {statusLabels[entry.status ?? ''] ?? entry.action ?? entry.status}</p>{/each}</details>{/if}
      </div>{/if}
    </li>{:else}<li class="empty">Aucun signalement dans cette sélection.</li>{/each}</ul>
  {/if}
</div>

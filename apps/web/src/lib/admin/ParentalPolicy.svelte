<script lang="ts">
  import { session } from '../auth/session';
  type Window = { day: number; startMinute: number; endMinute: number };
  let { userId }: { userId: string } = $props();
  let enabled = $state(false), maximumAge = $state(12), allowUnrated = $state(false);
  let tags = $state(''), windows = $state<Window[]>([]);
  let error = $state(''), notice = $state(''), busy = $state(false), loaded = $state(false);
  const days = ['Lundi','Mardi','Mercredi','Jeudi','Vendredi','Samedi','Dimanche'];
  $effect(() => {
    const id = userId, controller = new AbortController(); loaded = false; error = ''; notice = '';
    fetch(`/api/v1/admin/users/${encodeURIComponent(id)}/parental`, { signal: controller.signal, cache: 'no-store' })
      .then(async response => { if (!response.ok) throw new Error('Restrictions indisponibles.'); return response.json(); })
      .then(value => { if (!controller.signal.aborted) { enabled = value !== null; maximumAge = value?.maximumAge ?? 12; allowUnrated = value?.allowUnrated ?? false; tags = (value?.blockedTags ?? []).join(', '); windows = value?.accessSchedule ?? []; loaded = true; } })
      .catch(cause => { if (!controller.signal.aborted) error = cause.message; });
    return () => controller.abort();
  });
  function time(minutes: number) { return `${String(Math.floor(minutes / 60)).padStart(2,'0')}:${String(minutes % 60).padStart(2,'0')}`; }
  function minutes(value: string) { const [h,m] = value.split(':').map(Number); return h * 60 + m; }
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (busy || !loaded) return;
    busy = true; error = ''; notice = '';
    const id = userId;
    try {
      const response = await fetch(`/api/v1/admin/users/${encodeURIComponent(id)}/parental`, { method: enabled ? 'PUT' : 'DELETE', headers: { 'content-type':'application/json', 'x-csrf-token': $session?.csrfToken ?? '' }, body: enabled ? JSON.stringify({ maximumAge, allowUnrated, blockedTags: tags.split(',').map(v => v.trim()).filter(Boolean), accessSchedule: windows }) : undefined });
      if (!response.ok) throw new Error('Restrictions non enregistrées. Vérifiez les horaires et votre session.');
      if (id === userId) notice = 'Restrictions enregistrées.';
    } catch (cause) { if (id === userId) error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
  }
</script>
<form onsubmit={save}><fieldset disabled={busy || !loaded}><legend>Restrictions parentales</legend>
  <label><input type="checkbox" bind:checked={enabled} />Limiter ce compte</label>
  {#if enabled}
    <label>Âge maximal autorisé<input type="number" min="0" max="21" required bind:value={maximumAge} /></label>
    <label><input type="checkbox" bind:checked={allowUnrated} />Autoriser les médias sans classification connue</label>
    <label>Tags bloqués, séparés par des virgules<input bind:value={tags} maxlength="6400" placeholder="horreur, adulte" /></label>
    <p>Correspondance exacte des tags NFO ou manuels, sans distinction de casse. Les tags d’une série s’appliquent à ses épisodes.</p>
    <h3>Horaires d’accès (UTC)</h3><p>Sans plage : accès toute la semaine. Sinon, seules ces plages sont autorisées. Les bornes de fin sont exclues ; séparez les plages qui traversent minuit. Les administrateurs gardent leur accès de récupération.</p>
    {#each windows as window, index (window)}
      <div class="window"><label>Jour<select bind:value={window.day}>{#each days as day, number (day)}<option value={number}>{day}</option>{/each}</select></label>
      <label>Début<input type="time" required value={time(window.startMinute)} onchange={event => window.startMinute = minutes(event.currentTarget.value)} /></label>
      <label>Fin<select value={window.endMinute === 1440 ? 'midnight' : 'time'} onchange={event => window.endMinute = event.currentTarget.value === 'midnight' ? 1440 : 1200}><option value="time">Heure</option><option value="midnight">24:00</option></select>{#if window.endMinute !== 1440}<input aria-label="Heure de fin" type="time" required value={time(window.endMinute)} onchange={event => window.endMinute = minutes(event.currentTarget.value)} />{/if}</label>
      <button type="button" onclick={() => windows = windows.filter((_, i) => i !== index)}>Retirer</button></div>
    {/each}
    <button type="button" disabled={windows.length >= 56} onclick={() => windows = [...windows, {day:0,startMinute:480,endMinute:1200}]}>Ajouter une plage</button>
    <p>Les âges minimum peuvent être corrigés dans les fiches des médias. Les médias non classifiés sont bloqués par défaut.</p>
  {/if}
  <button disabled={busy}>Enregistrer les restrictions</button>
</fieldset>{#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}</form>
<style>
  fieldset { display:grid;gap:12px;border:1px solid var(--color-line);border-radius:var(--radius-md);margin:20px 0;padding:16px;min-width:0; }
  label { display:flex;gap:8px;align-items:center;flex-wrap:wrap; }
  button,input,select { background:var(--color-surface);color:var(--color-text);padding:10px;border:1px solid var(--color-line);border-radius: var(--radius-md);max-width:100%; }
  .window { display:flex;gap:10px;flex-wrap:wrap;padding:10px;border:1px solid var(--color-line); } p { font-size:13px;line-height:1.5; }
</style>

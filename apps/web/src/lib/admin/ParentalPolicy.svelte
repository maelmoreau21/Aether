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
form { display: grid; gap: 16px; font-family: var(--font-sans); color: var(--color-text); }
  fieldset { display: grid; gap: 16px; min-width: 0; margin: 0; padding: 0; border: 0; }
  fieldset:disabled { opacity: 0.7; }
  legend { margin: 0 0 16px; padding: 0; font-size: 17px; font-weight: 600; }
  h3 { margin: 8px 0 0; padding-top: 20px; border-top: 1px solid var(--color-line-subtle); font-size: 17px; font-weight: 700; }
  p { max-width: 720px; margin: 0; color: var(--color-muted); font-size: 13px; line-height: 1.5; }
  label { display: grid; gap: 8px; min-width: 0; font-size: 14px; font-weight: 700; }
  label:has(> input[type='checkbox']) { display: flex; align-items: center; gap: 12px; min-height: 44px; font-weight: 600; cursor: pointer; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); }
  input:not([type='checkbox']), select {
    box-sizing: border-box;
    width: 100%;
    max-width: 360px;
    height: 46px;
    padding: 0 14px;
    border: 1px solid var(--color-line-subtle);
    border-radius: var(--radius-sm);
    background-color: var(--color-canvas);
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }
  input:not([type='checkbox']):hover, select:hover { border-color: var(--color-line); }
  input:not([type='checkbox']):focus, select:focus { outline: none; border-color: var(--color-accent); background-color: var(--color-canvas); }
  input:not([type='number']):not([type='checkbox']):not([type='time']) { max-width: 520px; }
  .window {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr)) auto;
    align-items: start;
    gap: 12px;
    padding: 0 0 16px;
    border-bottom: 1px solid var(--color-line-subtle);
  }
  .window input, .window select { max-width: none; }
  .window > button { align-self: start; margin-top: 30px; }
  button {
    justify-self: start;
    height: 44px;
    padding: 0 18px;
    border-radius: var(--radius-md);
    background: transparent;
    border: 1px solid var(--color-line);
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color var(--transition-fast), opacity var(--transition-fast), transform var(--transition-fast);
  }
  button:hover:not(:disabled) { background: var(--color-raised); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .window > button { color: var(--color-danger); }
  .window > button:hover:not(:disabled) { background: var(--color-danger-soft); }
  button:not([type]) { height: 44px; padding: 0 22px; border-color: transparent; background: var(--color-accent); color: var(--color-on-accent); }
  button:not([type]):hover:not(:disabled) { background: var(--color-accent-hover); }
  p[role='alert'], p[role='status'] { max-width: none; padding: 12px 16px; border-radius: var(--radius-md); color: var(--color-text); font-size: 14px; }
  p[role='alert'] { background: var(--color-danger-soft); }
  p[role='status'] { background: var(--color-success-soft); }
  @media (max-width: 820px) {
    .window { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .window > button { margin-top: 0; }
  }
  @media (max-width: 640px) {
    .window { grid-template-columns: minmax(0, 1fr); }
    input:not([type='checkbox']), select { max-width: none; }
    button { width: 100%; justify-self: stretch; }
  }
  @media (prefers-reduced-motion: reduce) {
    button, input, select { transition: none; }
  }
</style>

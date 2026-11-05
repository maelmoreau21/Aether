<script lang="ts">
  import { session } from '../auth/session';
  let { userId }: { userId: string } = $props();
  const kindOptions = [{ id: 'films', label: 'Films' }, { id: 'series', label: 'Séries' }, { id: 'music', label: 'Musique' }] as const;
  let kinds = $state<string[]>(['films', 'series', 'music']), effective = $state<string[]>([]);
  let allow = $state(false), loaded = $state(false), busy = $state(false), error = $state(''), notice = $state('');
  $effect(() => {
    const id = userId, controller = new AbortController(); loaded = false; error = ''; notice = '';
    fetch(`/api/v1/admin/users/${encodeURIComponent(id)}/media-policy`, { signal: controller.signal, cache: 'no-store' })
      .then(async response => { if (!response.ok) throw new Error('Permissions indisponibles.'); return response.json(); })
      .then(value => { if (!controller.signal.aborted) { allow = value.allowMediaDownload; kinds = Array.isArray(value.allowedKinds) ? value.allowedKinds : ['films', 'series', 'music']; effective = Array.isArray(value.effectiveKinds) ? value.effectiveKinds : kinds; loaded = true; } })
      .catch(cause => { if (!controller.signal.aborted) error = cause.message; });
    return () => controller.abort();
  });
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (busy || !loaded) return;
    busy = true; error = ''; notice = ''; const id = userId;
    try {
      const response = await fetch(`/api/v1/admin/users/${encodeURIComponent(id)}/media-policy`, { method:'PUT', headers:{'content-type':'application/json','x-csrf-token':$session?.csrfToken ?? ''}, body:JSON.stringify({allowMediaDownload:allow, allowedKinds:kindOptions.map(option => option.id).filter(kind => kinds.includes(kind))}) });
      if (!response.ok) throw new Error('Permissions non enregistrées.');
      if (id === userId) { notice = 'Permissions enregistrées.'; effective = effective.filter(kind => kinds.includes(kind)); }
    } catch (cause) { if (id === userId) error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
  }
</script>
<form onsubmit={save}><fieldset disabled={busy || !loaded}><legend>Types de médias autorisés</legend>
  {#each kindOptions as option (option.id)}
    <label><input type="checkbox" value={option.id} bind:group={kinds} />{option.label}</label>
  {/each}
  <p>Un type décoché disparaît du menu, des étagères, de la recherche, des listes et de la lecture pour ce compte, quelles que soient ses bibliothèques. Les règles des groupes s’ajoutent : la plus restrictive l’emporte.</p>
  {#if loaded && effective.length < 3}<p>Types réellement accessibles avec les groupes : {effective.length ? kindOptions.filter(option => effective.includes(option.id)).map(option => option.label).join(', ') : 'aucun'}.</p>{/if}
</fieldset><fieldset disabled={busy || !loaded}><legend>Téléchargement et synchronisation</legend>
  <label><input type="checkbox" bind:checked={allow} />Autoriser le téléchargement des fichiers originaux et les nouvelles copies hors ligne</label>
  <p>Ce droit contrôle le téléchargement proposé par l’application. Un client autorisé à lire reçoit les octets du média et peut les enregistrer. Une copie déjà téléchargée reste sur l’appareil.</p>
  <button>Enregistrer les permissions</button>
</fieldset>{#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}</form>
<style>
form { display: grid; gap: 16px; font-family: var(--font-sans); color: var(--color-text); }
  fieldset { display: grid; gap: 12px; min-width: 0; margin: 0; padding: 0; border: 0; }
  fieldset:disabled { opacity: 0.7; }
  legend { margin: 0 0 16px; padding: 0; font-size: 17px; font-weight: 700; }
  label { display: flex; align-items: center; gap: 12px; min-height: 44px; font-size: 14px; font-weight: 600; cursor: pointer; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); }
  p { max-width: 720px; margin: 0; color: var(--color-muted); font-size: 13px; line-height: 1.5; }
  button {
    justify-self: start;
    height: 48px;
    margin-top: 8px;
    padding: 0 22px;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--color-accent);
    color: var(--color-on-accent);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity var(--transition-fast), transform var(--transition-fast);
  }
  button:hover:not(:disabled) { background: var(--color-accent-hover); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  p[role='alert'], p[role='status'] { max-width: none; padding: 12px 16px; border-radius: var(--radius-md); color: var(--color-text); font-size: 14px; }
  p[role='alert'] { background: var(--color-danger-soft); }
  p[role='status'] { background: var(--color-success-soft); }
  @media (max-width: 640px) {
    button { width: 100%; justify-self: stretch; }
  }
  @media (prefers-reduced-motion: reduce) {
    button { transition: none; }
  }
</style>

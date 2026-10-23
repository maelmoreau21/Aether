<script lang="ts">
  import { session } from '../auth/session';
  let { userId }: { userId: string } = $props();
  let allow = $state(false), loaded = $state(false), busy = $state(false), error = $state(''), notice = $state('');
  $effect(() => {
    const id = userId, controller = new AbortController(); loaded = false; error = ''; notice = '';
    fetch(`/api/v1/admin/users/${encodeURIComponent(id)}/media-policy`, { signal: controller.signal, cache: 'no-store' })
      .then(async response => { if (!response.ok) throw new Error('Permissions indisponibles.'); return response.json(); })
      .then(value => { if (!controller.signal.aborted) { allow = value.allowMediaDownload; loaded = true; } })
      .catch(cause => { if (!controller.signal.aborted) error = cause.message; });
    return () => controller.abort();
  });
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (busy || !loaded) return;
    busy = true; error = ''; notice = ''; const id = userId;
    try {
      const response = await fetch(`/api/v1/admin/users/${encodeURIComponent(id)}/media-policy`, { method:'PUT', headers:{'content-type':'application/json','x-csrf-token':$session?.csrfToken ?? ''}, body:JSON.stringify({allowMediaDownload:allow}) });
      if (!response.ok) throw new Error('Permissions non enregistrées.');
      if (id === userId) notice = 'Permissions enregistrées.';
    } catch (cause) { if (id === userId) error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
  }
</script>
<form onsubmit={save}><fieldset disabled={busy || !loaded}><legend>Téléchargement et synchronisation</legend>
  <label><input type="checkbox" bind:checked={allow} />Autoriser le téléchargement des fichiers originaux et les nouvelles copies hors ligne</label>
  <p>Ce droit contrôle le téléchargement proposé par l’application. Un client autorisé à lire reçoit les octets du média et peut les enregistrer. Une copie déjà téléchargée reste sur l’appareil.</p>
  <button>Enregistrer les permissions</button>
</fieldset>{#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}</form>
<style>fieldset { display:grid;gap:12px;border:1px solid var(--color-line);border-radius:var(--radius-md);margin-block:20px;padding:16px; } label { display:flex;gap:10px;align-items:center; } p { font-size:13px;line-height:1.5; } button { padding:12px;background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius: var(--radius-md); }</style>

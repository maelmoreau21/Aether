<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import type { Library } from './types';
  let { library, onclose, onchanged }: { library: Library; onclose: () => void; onchanged: () => Promise<void> } = $props();
  let dialog: HTMLDialogElement;
  let kinds = $state<string[]>([]), revision = $state(0), busy = $state(false), error = $state('');
  onMount(() => { kinds = [...library.imageKinds ?? []]; dialog.showModal(); });
  function url(kind: string) { return `/api/v1/admin/libraries/${encodeURIComponent(library.id)}/images/${kind}`; }
  async function change(kind: string, file?: File) {
    if (busy || !$session) return;
    if (file && (!['image/jpeg','image/png','image/webp'].includes(file.type) || file.size > 2 * 1024 * 1024)) { error = 'Choisissez une image PNG, JPEG ou WebP de 2 Mio maximum.'; return; }
    busy = true; error = '';
    try {
      const response = await fetch(url(kind), { method: file ? 'PUT' : 'DELETE', headers: { 'x-csrf-token': $session.csrfToken, ...(file ? {'content-type':file.type} : {}) }, body: file });
      if (!response.ok) throw new Error('Image non enregistrée. Vérifiez son format et votre session.');
      kinds = file ? [...new Set([...kinds, kind])] : kinds.filter(v => v !== kind); revision += 1;
      await onchanged();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
  }
</script>
<dialog bind:this={dialog} aria-labelledby="library-images-title" onclose={onclose} oncancel={event => { if (busy) event.preventDefault(); }}>
  <h2 id="library-images-title">Images de {library.name}</h2><p>Images conservées dans les données applicatives. Sans image personnalisée, la carte utilise les affiches disponibles.</p>
  {#each [['poster','Affiche'],['backdrop','Fond']] as [kind,label] (kind)}<section><h3>{label}</h3>
    {#if kinds.includes(kind)}<img src={`${url(kind)}?v=${revision}`} alt={`${label} de ${library.name}`} />{:else}<p>Aucune image personnalisée.</p>{/if}
    <label>Choisir une image<input type="file" accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={event => { const file = event.currentTarget.files?.[0]; if (file) void change(kind,file); event.currentTarget.value = ''; }} /></label>
    <button type="button" disabled={busy || !kinds.includes(kind)} onclick={() => change(kind)}>Retirer l’image</button>
  </section>{/each}
  {#if error}<p role="alert">{error}</p>{/if}<button type="button" disabled={busy} onclick={() => dialog.close()}>Fermer</button>
</dialog>
<style>
  dialog { width:min(640px,calc(100vw - 32px));max-height:calc(100dvh - 40px);overflow:auto;padding:24px;background:var(--bg-main);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:6px; } dialog::backdrop { background:rgb(0 0 0 / .65); } section { border-top:1px solid var(--border-subtle);padding-block:16px; } img { max-width:100%;max-height:180px;object-fit:contain;display:block;margin-bottom:12px; } label { display:grid;gap:8px;margin-block:12px; } input { max-width:100%; } button { padding:12px;background:var(--bg-surface);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:4px;font:inherit; } p { font-size:14px;line-height:1.5; }
</style>

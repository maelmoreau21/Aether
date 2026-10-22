<script lang="ts">
  import { session } from '../auth/session';
  import { avatarRevision } from '../auth/avatar';
  let busy = $state(false), error = $state(''), notice = $state('');
  type Extension = { id: string; name: string };
  type Pack = { id: string; name: string };
  let extensions = $state<Extension[]>([]), packs = $state<Pack[]>([]);
  let extensionId = $state(''), packId = $state(''), seed = $state('');
  let preview = $state('');
  $effect(() => {
    const userId = $session?.user.id;
    if (!userId) return;
    const controller = new AbortController();
    void fetch('/api/v1/account/avatar/extensions', { credentials:'same-origin', cache:'no-store', signal:controller.signal })
      .then(async response => { if (!response.ok) throw new Error('Extensions indisponibles.'); return response.json() as Promise<{extensions:Extension[]}>; })
      .then(value => { if ($session?.user.id === userId) extensions=value.extensions; })
      .catch(() => {});
    return () => controller.abort();
  });
  async function plugin(operation: string, apply = false) {
    const current=$session; if (!current || busy || !extensionId) return;
    if (operation === 'avatars.generate' && !seed) seed=crypto.randomUUID();
    busy=true; error=''; notice='';
    try {
      const response=await fetch(`/api/v1/account/avatar/extensions/${encodeURIComponent(extensionId)}`, {
        method:'POST',credentials:'same-origin',cache:'no-store',signal:AbortSignal.timeout(70000),
        headers:{'content-type':'application/json','x-csrf-token':current.csrfToken},
        body:JSON.stringify({operation,pack:packId || null,seed:seed || null,apply})
      });
      const value=await response.json() as {result: Pack[] | {svg:string}; error?:string; pluginError?:string};
      if (!response.ok) throw new Error(value.pluginError ?? value.error ?? 'Extension indisponible.');
      if ($session?.user.id !== current.user.id) return;
      if (operation==='avatars.list') { packs=value.result as Pack[]; packId=packs[0]?.id ?? ''; preview=''; }
      else {
        preview=`data:image/svg+xml;charset=utf-8,${encodeURIComponent((value.result as {svg:string}).svg)}`;
        if (apply) { avatarRevision.update(value=>value+1); notice='Avatar enregistré.'; }
      }
    } catch(cause) { if ($session?.user.id===current.user.id) error=cause instanceof Error ? cause.message : 'Extension indisponible.'; }
    finally { busy=false; }
  }
  async function update(file?: File) {
    const current = $session; if (!current || busy) return;
    error = ''; notice = '';
    if (file && (file.size > 2 * 1024 * 1024 || !['image/png', 'image/jpeg', 'image/webp'].includes(file.type))) { error = 'Choisissez une image PNG, JPEG ou WebP de 2 Mo maximum.'; return; }
    busy = true;
    try {
      const response = await fetch('/api/v1/account/avatar', { method: file ? 'PUT' : 'DELETE', credentials: 'same-origin', signal: AbortSignal.timeout(20000), headers: { 'x-csrf-token': current.csrfToken, ...(file ? { 'content-type': file.type } : {}) }, ...(file ? { body: file } : {}) });
      if (!response.ok) throw new Error(response.status === 413 ? 'Image trop volumineuse.' : 'Image refusée. Vérifiez son format et vos droits.');
      if ($session?.user.id !== current.user.id) return;
      avatarRevision.update(value => value + 1); notice = file ? 'Image de profil enregistrée.' : 'Image de profil supprimée.';
    } catch (cause) { if ($session?.user.id === current.user.id) error = cause instanceof Error ? cause.message : 'Modification impossible.'; }
    finally { busy = false; }
  }
</script>
<div class="avatar-editor">
  <label>Image de profil<input type="file" accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={(event) => { const file = event.currentTarget.files?.[0]; if (file) void update(file); event.currentTarget.value = ''; }} /></label>
  <p>PNG, JPEG ou WebP · 2 Mo maximum. L’image est recadrée au centre et enregistrée au format carré.</p>
  {#if extensions.length}
    <label>Extension d’avatars<select bind:value={extensionId} disabled={busy} onchange={() => { packs=[]; packId=''; preview=''; if (extensionId) void plugin('avatars.list'); }}><option value="">Choisir une extension</option>{#each extensions as extension (extension.id)}<option value={extension.id}>{extension.name}</option>{/each}</select></label>
    {#if packs.length}
      <label>Pack<select bind:value={packId} disabled={busy} onchange={() => preview=''}>{#each packs as pack (pack.id)}<option value={pack.id}>{pack.name}</option>{/each}</select></label>
      <label>Variante<input bind:value={seed} maxlength="128" placeholder="Un nom ou un numéro" disabled={busy} oninput={() => preview=''} /></label>
      <button type="button" disabled={busy} onclick={() => void plugin('avatars.generate')}>Aperçu</button>
      <button type="button" disabled={busy} onclick={() => void plugin('avatars.generate', true)}>Utiliser cet avatar</button>
      {#if preview}<img src={preview} alt="Aperçu de l’avatar proposé" width="128" height="128" />{/if}
    {/if}
  {/if}
  <button type="button" disabled={busy} onclick={() => void update()}>Retirer l’image</button>
  {#if busy}<p role="status">Enregistrement…</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}{#if error}<p role="alert">{error}</p>{/if}
</div>
<style>
  .avatar-editor { margin-bottom: 28px; padding-bottom: 24px; border-bottom: 1px solid var(--border-subtle); }
  label { display: grid; gap: 12px; font-size: 14px; }
  input { max-width: 100%; font: inherit; color: var(--text-primary); }
  p { font-size: 13px; line-height: 1.6; color: var(--text-secondary); }
  button { min-height: 44px; padding: 10px 16px; border: 1px solid var(--border-subtle); background: var(--bg-surface); color: var(--text-primary); border-radius: 4px; }
  [role='alert'] { color: var(--status-error); }
</style>

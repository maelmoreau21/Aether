<script lang="ts">
  import { session } from '../auth/session';
  import { avatarRevision } from '../auth/avatar';
  let busy = $state(false), error = $state(''), notice = $state('');
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

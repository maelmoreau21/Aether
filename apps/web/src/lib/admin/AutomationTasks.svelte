<script lang="ts">
  import { adminRequest } from './request';
  import type { Library } from './types';
  let { active = false, libraries }: { active?: boolean; libraries: Library[] } = $props();
  type Policy = { extensionId: string; libraryId: string; mode: string; priority: number; operation: string; configRevision: string; algorithmVersion: string; intervalSeconds: number; nextRunAt: number; maxConcurrent: number };
  type Extension = { manifest: { id: string; name: string; permissions: string[]; httpHosts?: string[] }; enabled: boolean };
  let extensions = $state<Extension[]>([]);
  let policies = $state<Policy[]>([]);
  let extensionId = $state('');
  let libraryId = $state('');
  let mode = $state('automatic');
  let hours = $state(24);
  let busy = $state(false);
  let loaded = $state(false);
  let attempted = $state(false);
  let error = $state('');
  let notice = $state('');
  const usable = $derived(libraries.filter(l => !l.id.startsWith('fixture-')));
  async function refresh() {
    busy = true; error = '';
    try {
      const [installed, configured] = await Promise.all([
        adminRequest<{ extensions: Extension[] }>('/api/v1/admin/extensions'),
        adminRequest<{ policies: Policy[] }>('/api/v1/admin/tasks/automation/policies')
      ]);
      extensions = installed.extensions.filter(e => e.enabled && e.manifest.permissions.includes('metadata.read'));
      policies = configured.policies;
      loaded = true;
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Chargement impossible.'; }
    finally { busy = false; }
  }
  $effect(() => { if (active && !attempted) { attempted = true; void refresh(); } });
  function selectPolicy() {
    const existing = policies.find(p => p.extensionId === extensionId && p.libraryId === libraryId);
    mode = existing?.mode ?? 'automatic'; hours = (existing?.intervalSeconds ?? 86400) / 3600;
  }
  async function save() {
    busy = true; error = ''; notice = '';
    const previous = policies.find(p => p.extensionId === extensionId && p.libraryId === libraryId);
    const network = extensions.find(e => e.manifest.id === extensionId)?.manifest.httpHosts?.length;
    const policy: Policy = { extensionId, libraryId, mode, priority: previous?.priority ?? 100,
      operation: previous?.operation ?? (network ? 'metadata.search' : 'metadata.propose'),
      configRevision: previous?.configRevision ?? '', algorithmVersion: previous?.algorithmVersion ?? '1',
      intervalSeconds: Math.round(hours * 3600), nextRunAt: 0, maxConcurrent: 1 };
    try {
      await adminRequest('/api/v1/admin/tasks/automation/policies', { method: 'PUT', body: JSON.stringify(policy) });
      policies = [...policies.filter(p => p.extensionId !== extensionId || p.libraryId !== libraryId), policy];
      notice = 'Déclencheur enregistré.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Enregistrement impossible.'; }
    finally { busy = false; }
  }
  async function run() {
    busy = true; error = ''; notice = '';
    try {
      await adminRequest('/api/v1/admin/tasks/automation/run', { method: 'POST', body: JSON.stringify({ extensionId, libraryId, idempotencyKey: crypto.randomUUID() }) });
      notice = 'Demande enregistrée dans la file. Les résultats inchangés sont réutilisés.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Admission impossible.'; }
    finally { busy = false; }
  }
</script>
<section aria-labelledby="automation-heading">
  <h3 id="automation-heading">Automatisation des métadonnées</h3>
  <p>Les extensions actives et autorisées traitent les nouveaux médias. Les corrections manuelles et la langue de la bibliothèque restent prioritaires.</p>
  {#if !loaded && busy}<p role="status">Chargement…</p>{/if}
  {#if loaded && !extensions.length}<p>Aucune extension de métadonnées active.</p>{/if}
  <form onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <label>Extension<select bind:value={extensionId} onchange={selectPolicy} disabled={busy} required><option value="">Choisir</option>{#each extensions as e (e.manifest.id)}<option value={e.manifest.id}>{e.manifest.name}</option>{/each}</select></label>
    <label>Bibliothèque<select bind:value={libraryId} onchange={selectPolicy} disabled={busy} required><option value="">Choisir</option>{#each usable as library (library.id)}<option value={library.id}>{library.name}</option>{/each}</select></label>
    <label>Déclenchement<select bind:value={mode} disabled={busy}><option value="automatic">Automatique après changement</option><option value="manual">Manuel</option><option value="scheduled">Programmé</option><option value="disabled">Désactivé</option></select></label>
    {#if mode === 'scheduled'}<label>Intervalle en heures<input type="number" min="1" max="8760" step="1" bind:value={hours} required disabled={busy} /></label>{/if}
    <button type="submit" disabled={busy || !extensionId || !libraryId}>Enregistrer</button>
    <button type="button" disabled={busy || !extensionId || !libraryId || mode === 'disabled'} onclick={() => void run()}>Exécuter maintenant</button>
    <button type="button" disabled={busy} onclick={() => void refresh()}>Actualiser</button>
  </form>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
</section>
<style>
  section { margin-top: 2rem; padding-top: 1rem; border-top: 1px solid var(--border); }
  form { display: flex; flex-wrap: wrap; align-items: end; gap: .8rem; }
  label { display: grid; gap: .35rem; }
  select, input, button { padding: .6rem; color: inherit; background: var(--surface); border: 1px solid var(--border); border-radius: .4rem; }
  p { max-width: 75ch; }
</style>

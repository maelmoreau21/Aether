<script module lang="ts">
  const scanLabels = { idle: 'Jamais scanné', queued: 'En attente', running: 'En cours', completed: 'Terminé', error: 'Erreur' } as const;
  const nodeStatusLabels = { connected: 'Connecté', disconnected: 'Hors ligne', revoked: 'Révoqué' } as const;
</script>
<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session, type AuthUser } from '../../../lib/auth/session';
  import ProfileMenu from '../../../lib/ui/ProfileMenu.svelte';
  interface Library { id: string; name: string; kind: 'films' | 'series'; nodeId: string; root: string; enabled: boolean; userIds: string[] }
  interface Scan { status: 'idle' | 'queued' | 'running' | 'completed' | 'error'; discovered: number; probed: number; reused: number; errors: string[] }
  interface NodeLibraryRef { id: string; name: string; kind: string; root: string; enabled: boolean }
  interface NodeScanSummary { libraryId: string; status: string; discovered: number; probed: number; reused: number; errorsCount: number; warningsCount: number }
  interface AdminNode {
    id: string;
    name: string;
    kind: 'local' | 'remote';
    baseUrl?: string | null;
    publicUrl?: string | null;
    status: 'connected' | 'disconnected' | 'revoked';
    keyId: string;
    roots: string[];
    pairedAt: number;
    lastSeenAt?: number | null;
    revokedAt?: number | null;
    libraries: NodeLibraryRef[];
    scans: NodeScanSummary[];
  }
  let scans = $state<Record<string, Scan>>({});
  let pending = $state<string[]>([]);
  let polling = false;
  let libraries = $state<Library[]>([]), users = $state<AuthUser[]>([]), roots = $state<string[]>([]), nodes = $state<AdminNode[]>([]);
  let loading = $state(true), saving = $state(false), pairing = $state(false), nodeActionBusy = $state<string | null>(null), error = $state(''), notice = $state('');
  let editing = $state<string | null>(null), name = $state(''), kind = $state<'films' | 'series'>('films'), nodeId = $state('local'), root = $state(''), enabled = $state(true), userIds = $state<string[]>([]);
  let pairName = $state(''), pairBaseUrl = $state(''), pairPublicUrl = $state(''), pairToken = $state('');
  const administrator = $derived($session?.user.role === 'administrator');
  const activeNodes = $derived(nodes.filter(n => n.status !== 'revoked'));
  const selectedNode = $derived(nodes.find(n => n.id === nodeId));
  const availableRootsForNode = $derived.by(() => {
    if (nodeId === 'local') {
      const localNode = nodes.find(n => n.id === 'local');
      return localNode && localNode.roots.length > 0 ? localNode.roots : roots;
    }
    return selectedNode?.roots ?? [];
  });
  function nodeLabel(id: string): string {
    if (id === 'local') return 'Node local';
    const found = nodes.find(n => n.id === id);
    return found ? `${found.name} (${found.id})` : id;
  }
  function formatTimestamp(ts?: number | null): string {
    if (!ts) return 'Jamais';
    try { return new Date(ts * 1000).toLocaleString('fr-FR'); } catch { return String(ts); }
  }
  async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await fetch(path, { ...init, credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (response.status === 401) throw new Error('Votre session a expiré. Reconnectez-vous.');
    if (response.status === 403) throw new Error('Accès refusé. Vérifiez votre session et vos droits.');
    if (response.status === 400 || response.status === 409) throw new Error('Configuration refusée. Vérifiez le nom, la racine et les utilisateurs.');
    if (response.status === 502) throw new Error('Le Media Node distant est injoignable ou le jeton d’appairage est invalide.');
    if (!response.ok) throw new Error('Le serveur est indisponible. Réessayez.');
    return response.json() as Promise<T>;
  }
  function reset() {
    editing = null;
    name = '';
    kind = 'films';
    nodeId = 'local';
    root = roots[0] ?? '';
    enabled = true;
    userIds = [];
  }
  function handleNodeChange() {
    const candidateRoots = availableRootsForNode;
    if (!candidateRoots.includes(root)) {
      root = candidateRoots[0] ?? '';
    }
  }
  function edit(library: Library) {
    editing = library.id;
    name = library.name;
    kind = library.kind;
    nodeId = library.nodeId;
    root = library.root;
    enabled = library.enabled;
    userIds = [...library.userIds];
    error = '';
    notice = '';
  }
  async function load() {
    if (!administrator) { loading = false; return; }
    loading = true; error = '';
    try {
      const [data, accounts, configured, nodeList] = await Promise.all([
        request<{ libraries: Library[] }>('/api/v1/admin/libraries'),
        request<{ users: AuthUser[] }>('/api/v1/admin/users'),
        request<{ roots: string[] }>('/api/v1/admin/roots'),
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes').catch(() => ({ nodes: [] as AdminNode[] }))
      ]);
      libraries = data.libraries;
      users = accounts.users;
      roots = configured.roots;
      nodes = nodeList.nodes;
      reset();
      await refreshScans();
    } catch (failure) { error = failure instanceof Error ? failure.message : 'Chargement impossible.'; }
    finally { loading = false; }
  }
  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!$session || !administrator || saving) return;
    saving = true; error = ''; notice = '';
    try {
      await request(editing ? `/api/v1/admin/libraries/${encodeURIComponent(editing)}` : '/api/v1/admin/libraries', {
        method: editing ? 'PUT' : 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
        body: JSON.stringify({ name: name.trim(), kind, nodeId, root, enabled, userIds })
      });
      const [data, nodeList] = await Promise.all([
        request<{ libraries: Library[] }>('/api/v1/admin/libraries'),
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes').catch(() => ({ nodes }))
      ]);
      libraries = data.libraries;
      nodes = nodeList.nodes;
      reset();
      notice = 'Bibliothèque enregistrée. Les permissions sont appliquées aux prochaines requêtes.';
    } catch (failure) { error = failure instanceof Error ? failure.message : 'Enregistrement impossible.'; }
    finally { saving = false; }
  }
  async function pairNode(event: SubmitEvent) {
    event.preventDefault();
    if (!$session || !administrator || pairing) return;
    pairing = true; error = ''; notice = '';
    try {
      const created = await request<AdminNode>('/api/v1/admin/nodes/pair', {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken },
        body: JSON.stringify({
          name: pairName.trim(),
          baseUrl: pairBaseUrl.trim(),
          publicUrl: pairPublicUrl.trim() || null,
          pairingToken: pairToken.trim()
        })
      });
      const [nodeList, configured] = await Promise.all([
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes'),
        request<{ roots: string[] }>('/api/v1/admin/roots')
      ]);
      nodes = nodeList.nodes;
      roots = configured.roots;
      pairName = ''; pairBaseUrl = ''; pairPublicUrl = ''; pairToken = '';
      nodeId = created.id;
      root = created.roots[0] ?? root;
      notice = `Media Node « ${created.name} » (${created.id}) appairé avec succès.`;
    } catch (failure) { error = failure instanceof Error ? failure.message : 'Appairage du Media Node impossible.'; }
    finally { pairing = false; }
  }
  async function nodeAction(targetNode: AdminNode, action: 'reconnect' | 'rotate-secret' | 'revoke') {
    if (!$session || !administrator || nodeActionBusy) return;
    nodeActionBusy = `${targetNode.id}:${action}`; error = ''; notice = '';
    try {
      await request<AdminNode>(`/api/v1/admin/nodes/${encodeURIComponent(targetNode.id)}/${action}`, {
        method: 'POST',
        headers: { 'x-csrf-token': $session.csrfToken }
      });
      const [nodeList, configured] = await Promise.all([
        request<{ nodes: AdminNode[] }>('/api/v1/admin/nodes'),
        request<{ roots: string[] }>('/api/v1/admin/roots').catch(() => ({ roots }))
      ]);
      nodes = nodeList.nodes;
      roots = configured.roots;
      notice = action === 'revoke'
        ? `Media Node « ${targetNode.name} » révoqué.`
        : action === 'rotate-secret'
          ? `Secret du Media Node « ${targetNode.name} » renouvelé.`
          : `Media Node « ${targetNode.name} » reconnecté.`;
    } catch (failure) { error = failure instanceof Error ? failure.message : 'Action sur le Media Node impossible.'; }
    finally { nodeActionBusy = null; }
  }
  async function refreshScans() {
    if (polling || !administrator) return;
    polling = true;
    try {
      const scannable = libraries.filter(library => (library.kind === 'films' || library.kind === 'series') && library.enabled && !library.id.startsWith('fixture-'));
      const results = await Promise.all(scannable.map(async library => [library.id, await request<Scan>(`/api/v1/admin/libraries/${encodeURIComponent(library.id)}/scan`)] as const));
      for (const [id, scan] of results) scans[id] = scan;
    } catch (failure) { error = failure instanceof Error ? failure.message : 'État du scan indisponible.'; }
    finally { polling = false; }
  }
  async function scan(library: Library) {
    if (!$session || !administrator || pending.includes(library.id)) return;
    pending = [...pending, library.id]; error = ''; notice = '';
    try {
      const started = await request<Pick<Scan, 'status'>>(`/api/v1/admin/libraries/${encodeURIComponent(library.id)}/scan`, { method: 'POST', headers: { 'x-csrf-token': $session.csrfToken } });
      scans[library.id] = { status: started.status, discovered: 0, probed: 0, reused: 0, errors: [] };
      await refreshScans();
    } catch (failure) { error = failure instanceof Error ? failure.message : 'Lancement du scan impossible.'; }
    finally { pending = pending.filter(id => id !== library.id); }
  }
  onMount(() => {
    void load();
    const timer = setInterval(() => { if (Object.values(scans).some(scan => scan.status === 'queued' || scan.status === 'running')) void refreshScans(); }, 1500);
    return () => clearInterval(timer);
  });
</script>

<svelte:head><title>Bibliothèques — Administration</title></svelte:head>
<header><a href={resolve('/')}>← Accueil</a><ProfileMenu /></header>
<main>
  <p class="eyebrow">Administration</p><h1>Bibliothèques</h1>
  {#if !administrator}<p role="alert">Cette page est réservée aux administrateurs.</p>
  {:else}
    <p class="muted">Gérez les Media Nodes (local ou distants), les bibliothèques associées et les comptes autorisés à les consulter.</p>
    {#if error}<div class="error" role="alert">{error} <button disabled={saving} onclick={load}>Recharger</button></div>{/if}
    {#if notice}<p role="status">{notice}</p>{/if}
    {#if loading}<p role="status">Chargement des bibliothèques…</p>
    {:else}
      <div class="columns">
        <section aria-labelledby="library-list"><h2 id="library-list">Bibliothèques configurées</h2>
          {#if libraries.length === 0}<p class="muted">Aucune bibliothèque configurée.</p>{/if}
          {#each libraries as library (library.id)}
            <article><div><h3>{library.name}</h3><p>{library.kind === 'films' ? 'Films' : 'Séries'} · {nodeLabel(library.nodeId)} · {library.enabled ? 'Active' : 'Désactivée'}</p><p class="path">{library.root}</p><p>{library.userIds.length} utilisateur(s) autorisé(s)</p>
              {#if scans[library.id]}{@const scan = scans[library.id]}<div class="scan-state" role="status"><p>Scan : {scanLabels[scan.status]} · {scan.discovered} fichier(s) découvert(s) · {scan.probed} analysé(s) · {scan.reused} réutilisé(s)</p>{#if scan.errors.length}<details><summary>{scan.errors.length} erreur(s) signalée(s)</summary><ul>{#each scan.errors as failure, index (index)}<li>{failure}</li>{/each}</ul></details>{/if}</div>{/if}
              {#if (library.kind === 'films' || library.kind === 'series') && library.enabled && !library.id.startsWith('fixture-')}<button disabled={pending.includes(library.id) || scans[library.id]?.status === 'queued' || scans[library.id]?.status === 'running'} onclick={() => scan(library)} aria-label={`Scanner ${library.name}`}>Scanner</button>{/if}
              </div><button disabled={saving} onclick={() => edit(library)} aria-label={`Modifier ${library.name}`}>Modifier</button></article>
          {/each}
        </section>
        <section aria-labelledby="library-form"><h2 id="library-form">{editing ? 'Modifier la bibliothèque' : 'Ajouter une bibliothèque'}</h2>
          <form onsubmit={save}><fieldset disabled={saving}>
            <label for="library-name">Nom</label><input id="library-name" bind:value={name} required maxlength="100" autocomplete="off" />
            <label for="library-kind">Type</label><select id="library-kind" bind:value={kind}><option value="films">Films</option><option value="series">Séries</option></select>
            <label for="library-node">Media Node</label>
            {#if activeNodes.length > 1}
              <select id="library-node" bind:value={nodeId} onchange={handleNodeChange}>
                <option value="local">Node local</option>
                {#each activeNodes.filter(n => n.id !== 'local') as node (node.id)}
                  <option value={node.id}>{node.name} ({node.id})</option>
                {/each}
              </select>
            {:else}
              <input id="library-node" value="Node local" readonly />
            {/if}
            <label for="library-root">Racine autorisée</label><select id="library-root" bind:value={root} required><option value="" disabled>Choisir une racine</option>{#each availableRootsForNode as item (item)}<option value={item}>{item}</option>{/each}</select>
            {#if availableRootsForNode.length === 0}<p class="muted">Aucune racine autorisée n’est configurée sur ce Media Node.</p>{/if}
            {#if editing && !availableRootsForNode.includes(root)}<p class="error">Cette racine n’est plus autorisée. Choisissez une racine disponible avant d’enregistrer.</p>{/if}
            <label class="check"><input type="checkbox" bind:checked={enabled} />Bibliothèque active</label>
            <fieldset class="grants"><legend>Utilisateurs autorisés</legend><p class="muted">Les administrateurs gèrent le serveur. Les utilisateurs ont uniquement les accès cochés.</p>
              {#each users.filter(user => user.role === 'user') as user (user.id)}<label class="check"><input type="checkbox" bind:group={userIds} value={user.id} />{user.login}</label>{/each}
              {#if !users.some(user => user.role === 'user')}<p class="muted">Aucun compte utilisateur disponible.</p>{/if}
            </fieldset>
            <div class="actions"><button class="primary" type="submit" disabled={!availableRootsForNode.includes(root)}>{saving ? 'Enregistrement…' : 'Enregistrer'}</button><button type="button" onclick={reset}>{editing ? 'Annuler' : 'Réinitialiser'}</button></div>
          </fieldset></form>
          <p class="muted">Les fichiers restent en lecture seule. Lancez le scan après l’ajout d’une bibliothèque. Aucun scan ne se déclenche automatiquement.</p>
        </section>
      </div>

      <div class="columns">
        <section aria-labelledby="node-list"><h2 id="node-list">Media Nodes enregistrés</h2>
          {#if nodes.length === 0}<p class="muted">Aucun Media Node enregistré.</p>{/if}
          {#each nodes as node (node.id)}
            <article>
              <div>
                <h3>{node.name} <span class="badge" data-status={node.status}>{nodeStatusLabels[node.status]}</span></h3>
                <p>Identité : <code>{node.id}</code> · Type : {node.kind === 'local' ? 'Standalone local' : 'Node distant'}</p>
                {#if node.baseUrl}<p class="path">URL interne : {node.baseUrl}{node.publicUrl ? ` · URL publique : ${node.publicUrl}` : ''}</p>{/if}
                <p>Dernier contact : {formatTimestamp(node.lastSeenAt)} · Clé active : <code>{node.keyId}</code></p>
                {#if node.roots.length > 0}<p class="path">Racines exposées : {node.roots.join(', ')}</p>{/if}
                <p>Bibliothèques associées : {node.libraries.length ? node.libraries.map(l => `${l.name} (${l.kind})`).join(', ') : 'Aucune'}</p>
              </div>
              {#if node.kind === 'remote'}
                <div class="actions">
                  {#if node.status !== 'revoked'}
                    <button disabled={!!nodeActionBusy} onclick={() => nodeAction(node, 'reconnect')} aria-label={`Reconnecter ${node.name}`}>Reconnecter</button>
                    <button disabled={!!nodeActionBusy} onclick={() => nodeAction(node, 'rotate-secret')} aria-label={`Renouveler le secret de ${node.name}`}>Renouveler secret</button>
                    <button disabled={!!nodeActionBusy} onclick={() => nodeAction(node, 'revoke')} aria-label={`Révoquer ${node.name}`}>Révoquer</button>
                  {/if}
                </div>
              {/if}
            </article>
          {/each}
        </section>
        <section aria-labelledby="node-pair-form"><h2 id="node-pair-form">Appairer un Media Node distant</h2>
          <form onsubmit={pairNode}><fieldset disabled={pairing}>
            <label for="pair-node-name">Nom du Media Node</label><input id="pair-node-name" bind:value={pairName} required maxlength="100" placeholder="Ex. Node Films NAS" autocomplete="off" />
            <label for="pair-node-base-url">URL interne du Node (Core → Node)</label><input id="pair-node-base-url" bind:value={pairBaseUrl} required placeholder="http://aether-node-films:3101" autocomplete="off" />
            <label for="pair-node-public-url">URL publique de streaming direct (Navigateur → Node, facultatif)</label><input id="pair-node-public-url" bind:value={pairPublicUrl} placeholder="http://127.0.0.1:3101" autocomplete="off" />
            <label for="pair-node-token">Jeton d’appairage initial</label><input id="pair-node-token" type="password" bind:value={pairToken} required autocomplete="off" />
            <div class="actions"><button class="primary" type="submit">{pairing ? 'Appairage…' : 'Appairer le Node'}</button></div>
          </fieldset></form>
          <p class="muted">L’appairage vérifie mutuellement l’identité du Node et établit un secret HMAC dédié sans partager les sessions Web.</p>
        </section>
      </div>
    {/if}
  {/if}
</main>

<style>
  header { display:flex; align-items:center; gap:24px; padding:20px var(--page-gutter); border-bottom:1px solid var(--color-line); font-size:13px; }
  main { max-width:1440px; margin:auto; padding:48px var(--page-gutter); }h1 { font-size:clamp(30px,4vw,46px); margin:10px 0 16px; }h2 { font-size:19px; margin:0 0 24px; }h3 { font-size:16px; margin:0 0 8px; }
  p { line-height:1.7; font-size:13px; }.muted,article p { color:var(--color-muted); }.columns { display:grid; grid-template-columns:1fr 1fr; gap:32px; margin-top:32px; }section { background:var(--color-surface); border:1px solid var(--color-line); border-radius:var(--radius-md); padding:28px; min-width:0; }
  article { display:flex; align-items:flex-start; gap:16px; justify-content:space-between; padding:20px 0; border-bottom:1px solid var(--color-line); }article>div { min-width:0; }article p { margin:4px 0; }.path { overflow-wrap:anywhere; }fieldset { border:0; padding:0; margin:0; min-width:0; }label { display:block; font-size:13px; margin:18px 0 8px; }input:not([type='checkbox']), select { width:100%; min-height:46px; border:1px solid var(--color-line); border-radius:6px; padding:10px 14px; color:var(--color-text); background:var(--color-raised); }input[readonly] { color:var(--color-muted); }.check { display:flex; gap:10px; align-items:center; min-height:32px; }.check input { accent-color:var(--color-accent); }.grants { margin:24px 0; padding-top:20px; border-top:1px solid var(--color-line); }legend { font-size:14px; }.actions { display:flex; gap:12px; flex-wrap:wrap; }button { border:1px solid var(--color-line); border-radius:8px; padding:10px 16px; color:var(--color-text); background:var(--color-raised); min-height:42px; }button:hover:not(:disabled) { background:var(--color-accent-soft); }.primary { background:var(--color-accent); color:#191322; }.error { color:var(--color-danger); margin:20px 0; }.error button { margin-left:10px; }
  .badge { font-size:11px; padding:2px 8px; border-radius:99px; border:1px solid var(--color-line); margin-left:8px; vertical-align:middle; }
  .badge[data-status='connected'] { color:#9be7b4; border-color:#2e6b44; }
  .badge[data-status='disconnected'] { color:#f5c97b; border-color:#75541e; }
  .badge[data-status='revoked'] { color:var(--color-danger); border-color:var(--color-danger); }
  @media(max-width:850px) { .columns { grid-template-columns:1fr; }main { padding-top:32px; }section { padding:20px; } }
</style>

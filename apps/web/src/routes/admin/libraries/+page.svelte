<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session, type AuthUser } from '../../../lib/auth/session';
  import ProfileMenu from '../../../lib/ui/ProfileMenu.svelte';
  interface Library { id: string; name: string; kind: 'films' | 'series'; nodeId: string; root: string; enabled: boolean; userIds: string[] }
  let libraries = $state<Library[]>([]), users = $state<AuthUser[]>([]), roots = $state<string[]>([]);
  let loading = $state(true), saving = $state(false), error = $state(''), notice = $state('');
  let editing = $state<string | null>(null), name = $state(''), kind = $state<'films' | 'series'>('films'), root = $state(''), enabled = $state(true), userIds = $state<string[]>([]);
  const administrator = $derived($session?.user.role === 'administrator');
  async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await fetch(path, { ...init, credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (response.status === 401) throw new Error('Votre session a expiré. Reconnectez-vous.');
    if (response.status === 403) throw new Error('Accès refusé. Vérifiez votre session et vos droits.');
    if (response.status === 400 || response.status === 409) throw new Error('Configuration refusée. Vérifiez le nom, la racine et les utilisateurs.');
    if (!response.ok) throw new Error('Le serveur est indisponible. Réessayez.');
    return response.json() as Promise<T>;
  }
  function reset() { editing = null; name = ''; kind = 'films'; root = roots[0] ?? ''; enabled = true; userIds = []; }
  function edit(library: Library) { editing = library.id; name = library.name; kind = library.kind; root = library.root; enabled = library.enabled; userIds = [...library.userIds]; error = ''; notice = ''; }
  async function load() {
    if (!administrator) { loading = false; return; }
    loading = true; error = '';
    try {
      const [data, accounts, configured] = await Promise.all([request<{ libraries: Library[] }>('/api/v1/admin/libraries'), request<{ users: AuthUser[] }>('/api/v1/admin/users'), request<{ roots: string[] }>('/api/v1/admin/roots')]);
      libraries = data.libraries; users = accounts.users; roots = configured.roots; reset();
    } catch (failure) { error = failure instanceof Error ? failure.message : 'Chargement impossible.'; }
    finally { loading = false; }
  }
  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!$session || !administrator || saving) return;
    saving = true; error = ''; notice = '';
    try {
      await request(editing ? `/api/v1/admin/libraries/${encodeURIComponent(editing)}` : '/api/v1/admin/libraries', { method: editing ? 'PUT' : 'POST', headers: { 'content-type': 'application/json', 'x-csrf-token': $session.csrfToken }, body: JSON.stringify({ name: name.trim(), kind, nodeId: 'local', root, enabled, userIds }) });
      const data = await request<{ libraries: Library[] }>('/api/v1/admin/libraries');
      libraries = data.libraries; reset(); notice = 'Bibliothèque enregistrée. Les permissions sont appliquées aux prochaines requêtes.';
    } catch (failure) { error = failure instanceof Error ? failure.message : 'Enregistrement impossible.'; }
    finally { saving = false; }
  }
  onMount(() => { void load(); });
</script>

<svelte:head><title>Bibliothèques — Administration</title></svelte:head>
<header><a href={resolve('/')}>← Accueil</a><ProfileMenu /></header>
<main>
  <p class="eyebrow">Administration</p><h1>Bibliothèques</h1>
  {#if !administrator}<p role="alert">Cette page est réservée aux administrateurs.</p>
  {:else}
    <p class="muted">Gérez les bibliothèques du Media Node local et les comptes autorisés à les consulter.</p>
    {#if error}<div class="error" role="alert">{error} <button disabled={saving} onclick={load}>Recharger</button></div>{/if}
    {#if notice}<p role="status">{notice}</p>{/if}
    {#if loading}<p role="status">Chargement des bibliothèques…</p>
    {:else}
      <div class="columns">
        <section aria-labelledby="library-list"><h2 id="library-list">Bibliothèques configurées</h2>
          {#if libraries.length === 0}<p class="muted">Aucune bibliothèque configurée.</p>{/if}
          {#each libraries as library (library.id)}
            <article><div><h3>{library.name}</h3><p>{library.kind === 'films' ? 'Films' : 'Séries'} · Node local · {library.enabled ? 'Active' : 'Désactivée'}</p><p class="path">{library.root}</p><p>{library.userIds.length} utilisateur(s) autorisé(s)</p></div><button disabled={saving} onclick={() => edit(library)} aria-label={`Modifier ${library.name}`}>Modifier</button></article>
          {/each}
        </section>
        <section aria-labelledby="library-form"><h2 id="library-form">{editing ? 'Modifier la bibliothèque' : 'Ajouter une bibliothèque'}</h2>
          <form onsubmit={save}><fieldset disabled={saving}>
            <label for="library-name">Nom</label><input id="library-name" bind:value={name} required maxlength="100" autocomplete="off" />
            <label for="library-kind">Type</label><select id="library-kind" bind:value={kind}><option value="films">Films</option><option value="series">Séries</option></select>
            <label for="library-node">Media Node</label><input id="library-node" value="Node local" readonly />
            <label for="library-root">Racine autorisée</label><select id="library-root" bind:value={root} required><option value="" disabled>Choisir une racine</option>{#each roots as item (item)}<option value={item}>{item}</option>{/each}</select>
            {#if roots.length === 0}<p class="muted">Aucune racine autorisée n’est configurée sur le Node local.</p>{/if}
            {#if editing && !roots.includes(root)}<p class="error">Cette racine n’est plus autorisée. Choisissez une racine disponible avant d’enregistrer.</p>{/if}
            <label class="check"><input type="checkbox" bind:checked={enabled} />Bibliothèque active</label>
            <fieldset class="grants"><legend>Utilisateurs autorisés</legend><p class="muted">Les administrateurs gèrent le serveur. Les utilisateurs ont uniquement les accès cochés.</p>
              {#each users.filter(user => user.role === 'user') as user (user.id)}<label class="check"><input type="checkbox" bind:group={userIds} value={user.id} />{user.login}</label>{/each}
              {#if !users.some(user => user.role === 'user')}<p class="muted">Aucun compte utilisateur disponible.</p>{/if}
            </fieldset>
            <div class="actions"><button class="primary" type="submit" disabled={!roots.includes(root)}>{saving ? 'Enregistrement…' : 'Enregistrer'}</button><button type="button" onclick={reset}>{editing ? 'Annuler' : 'Réinitialiser'}</button></div>
          </fieldset></form>
          <p class="muted">Les fichiers restent en lecture seule. Une nouvelle bibliothèque reste vide jusqu’au prochain jalon scanner ; aucun scan ne se déclenche à l’ajout.</p>
        </section>
      </div>
    {/if}
  {/if}
</main>

<style>
  header { display:flex; align-items:center; gap:24px; padding:20px var(--page-gutter); border-bottom:1px solid var(--color-line); font-size:13px; }
  main { max-width:1440px; margin:auto; padding:48px var(--page-gutter); }h1 { font-size:clamp(30px,4vw,46px); margin:10px 0 16px; }h2 { font-size:19px; margin:0 0 24px; }h3 { font-size:16px; margin:0 0 8px; }
  p { line-height:1.7; font-size:13px; }.muted,article p { color:var(--color-muted); }.columns { display:grid; grid-template-columns:1fr 1fr; gap:32px; margin-top:32px; }section { background:var(--color-surface); border:1px solid var(--color-line); border-radius:var(--radius-md); padding:28px; min-width:0; }
  article { display:flex; align-items:flex-start; gap:16px; justify-content:space-between; padding:20px 0; border-bottom:1px solid var(--color-line); }article>div { min-width:0; }article p { margin:4px 0; }.path { overflow-wrap:anywhere; }fieldset { border:0; padding:0; margin:0; min-width:0; }label { display:block; font-size:13px; margin:18px 0 8px; }input:not([type='checkbox']) { width:100%; min-height:46px; border:1px solid var(--color-line); border-radius:6px; padding:10px 14px; color:var(--color-text); background:var(--color-raised); }input[readonly] { color:var(--color-muted); }.check { display:flex; gap:10px; align-items:center; min-height:32px; }.check input { accent-color:var(--color-accent); }.grants { margin:24px 0; padding-top:20px; border-top:1px solid var(--color-line); }legend { font-size:14px; }.actions { display:flex; gap:12px; flex-wrap:wrap; }button { border:1px solid var(--color-line); border-radius:8px; padding:10px 16px; color:var(--color-text); background:var(--color-raised); min-height:42px; }button:hover:not(:disabled) { background:var(--color-accent-soft); }.primary { background:var(--color-accent); color:#191322; }.error { color:var(--color-danger); margin:20px 0; }.error button { margin-left:10px; }
  @media(max-width:850px) { .columns { grid-template-columns:1fr; }main { padding-top:32px; }section { padding:20px; } }
</style>

<script lang="ts">
  import { session } from '../auth/session';
  type Options = { enabled: boolean; excludeWatched: boolean; historyDays: number };
  let options = $state<Options>({ enabled: false, excludeWatched: true, historyDays: 180 });
  let loading = $state(true);
  let saving = $state(false);
  let error = $state('');
  let notice = $state('');
  $effect(() => {
    const owner = $session?.user.id;
    options = { enabled: false, excludeWatched: true, historyDays: 180 };
    error = ''; notice = ''; loading = true;
    const controller = new AbortController();
    if (!owner) { loading = false; return; }
    void fetch('/api/v1/account/recommendations/preferences', { credentials: 'same-origin', cache: 'no-store', signal: controller.signal })
      .then(async response => { if (!response.ok) throw new Error(); const value = await response.json(); if (!controller.signal.aborted) options = value; })
      .catch(() => { if (!controller.signal.aborted) error = 'Chargement des préférences impossible.'; })
      .finally(() => { loading = false; });
    return () => controller.abort();
  });
  async function save(event: SubmitEvent) {
    event.preventDefault(); saving = true; error = ''; notice = '';
    const owner = $session?.user.id;
    try {
      const response = await fetch('/api/v1/account/recommendations/preferences', {
        method: 'PUT', credentials: 'same-origin', cache: 'no-store',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify(options), signal: AbortSignal.timeout(10000)
      });
      if (!response.ok) throw new Error();
      const value = await response.json();
      if ($session?.user.id !== owner) return;
      options = value; notice = 'Préférences enregistrées.';
      window.dispatchEvent(new Event('media-recommendations-changed'));
    } catch { if ($session?.user.id === owner) error = 'Enregistrement impossible. Vérifiez votre session et les valeurs.'; }
    finally { saving = false; }
  }
</script>
<section class="card" aria-labelledby="recommendations-heading">
  <h2 id="recommendations-heading">Recommandations personnelles</h2>
  <p>Calcul local à partir des genres, personnes, favoris, notes et contenus vus de votre compte. Aucun service externe ni profil calculé conservé. Les contenus similaires d’une fiche restent distincts.</p>
  <form onsubmit={save}>
    <fieldset disabled={loading || saving}>
      <label><input type="checkbox" bind:checked={options.enabled} /> Activer les recommandations sur l’accueil</label>
      <label><input type="checkbox" bind:checked={options.excludeWatched} /> Exclure les contenus déjà vus</label>
      <label>Historique utilisé (jours)<input type="number" min="1" max="3650" required bind:value={options.historyDays} /></label>
      <p>Cette durée limite le calcul ; elle ne supprime pas votre historique de lecture. Vos favoris et notes actuels restent pris en compte. Le calcul examine au maximum les 1 000 titres les plus récents accessibles.</p>
      <button type="submit">{saving ? 'Enregistrement…' : 'Enregistrer les recommandations'}</button>
    </fieldset>
  </form>
  {#if loading}<p role="status">Chargement…</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  .card { padding:24px; margin-bottom:24px; border:1px solid var(--color-line); border-radius:var(--radius-md); background:var(--color-surface); }
  fieldset { border:0; padding:0; display:grid; gap:12px; } label { display:flex; align-items:center; gap:8px; flex-wrap:wrap; }
  p { color:var(--color-muted); line-height:1.5; } input[type='number'] { width:100px; } button,input { color:var(--color-text); background:var(--color-raised); border:1px solid var(--color-line-strong); padding:8px; border-radius:8px; }
</style>

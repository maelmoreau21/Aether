<script lang="ts">
  import { preferences } from '../preferences';
  import Icon from '../ui/Icon.svelte';
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import type { Library } from '../admin/types';

  type Entry = {
    id: string;
    kind: string;
    url: string;
    origin: string;
    width?: number | null;
    height?: number | null;
    language?: string | null;
    provider?: string | null;
  };

  type CandidateImage = {
    url: string;
    provider: string;
    providerName?: string;
    imageType: string;
    width?: number | null;
    height?: number | null;
    language?: string | null;
    rating?: number | null;
    voteCount?: number | null;
    isPrimary?: boolean;
  };

  let {
    title,
    workId,
    library,
    onclose,
    onchanged = async () => {}
  }: {
    title: string;
    workId?: string;
    library?: Library;
    onclose: () => void;
    onchanged?: () => Promise<void>;
  } = $props();

  const labels: Record<string, string> = {
    poster: 'Principal',
    thumb: 'Vignette',
    logo: 'Logo',
    illustration: 'Illustration',
    banner: 'Bannière',
    disc: 'Disque',
    landscape: 'Paysage',
    backdrop: 'Arrière-plan'
  };

  let dialog: HTMLDialogElement, preview: HTMLDialogElement, searchDialog: HTMLDialogElement;
  let entries = $state<Entry[]>([]), selected = $state<Entry | null>(null), kind = $state('poster'), replacing = $state<Entry | null>(null);
  let error = $state(''), busy = $state(false), loading = $state(true), adding = $state(false);
  let fileInput = $state<HTMLInputElement>();
  let imageLanguages = $state(preferences().locale === 'en' ? 'en, fr' : 'fr, en'), includeNeutral = $state(true), collectionMessage = $state('');
  let similarityThreshold = $state(5), similarPairs = $state<{ first: string; second: string }[] | null>(null), uncompared = $state(0), similarVisible = $state(20), similarTruncated = $state(false);

  // Live visual search state (Jellyfin-style image picker)
  let candidates = $state<CandidateImage[]>([]);
  let providersList = $state<{ id: string; name: string }[]>([]);
  let searchProvider = $state('all');
  let searchType = $state('poster');
  let searchAllLanguages = $state(false);
  let searching = $state(false);
  let searchError = $state('');
  let applyingUrl = $state<string | null>(null);
  let appliedUrls = $state<Set<string>>(new Set());

  const kinds = $derived(library ? ['poster', 'backdrop'] : Object.keys(labels));
  const base = $derived(workId ? `/api/v1/works/${encodeURIComponent(workId)}/images` : `/api/v1/admin/libraries/${encodeURIComponent(library?.id ?? '')}/images`);
  const imageUrl = (entry: Entry, small = true) => workId ? `${entry.url}${entry.url.includes('?') ? '&' : '?'}width=${small ? 320 : 1280}` : entry.url;

  async function load() {
    if (library) {
      entries = (library.imageKinds ?? []).map(kind => ({
        id: kind,
        kind,
        url: `${base}/${kind}?v=${library?.imageVersions?.[kind] ?? ''}`,
        origin: 'manual'
      }));
      loading = false;
      return;
    }
    const response = await fetch(base, { cache: 'no-store', signal: AbortSignal.timeout(8000) });
    if (!response.ok) throw new Error('Images indisponibles.');
    const incoming: Entry[] = (await response.json()).images;
    entries = incoming.map(entry => {
      const previous = entries.find(old => old.id === entry.id);
      return { ...entry, width: entry.width ?? previous?.width, height: entry.height ?? previous?.height };
    });
    loading = false;
  }

  onMount(() => {
    dialog.showModal();
    void load().catch(cause => {
      error = cause.message;
      loading = false;
    });
  });

  function add(type = 'poster', entry: Entry | null = null) {
    kind = type;
    replacing = entry;
    adding = true;
  }

  async function upload(event: Event) {
    const file = (event.currentTarget as HTMLInputElement).files?.[0];
    if (!file || busy) return;
    if (!['image/jpeg', 'image/png', 'image/webp'].includes(file.type) || file.size > 2 * 1024 * 1024) {
      error = 'Choisissez une image PNG, JPEG ou WebP de 2 Mio maximum.';
      if (fileInput) fileInput.value = '';
      return;
    }
    busy = true;
    error = '';
    try {
      const response = await fetch(`${base}/${kind}${workId && replacing ? `?replace=${encodeURIComponent(replacing.id)}` : ''}`, {
        method: workId ? 'POST' : 'PUT',
        headers: { 'x-csrf-token': $session?.csrfToken ?? '', 'content-type': file.type },
        body: file,
        signal: AbortSignal.timeout(20000)
      });
      if (!response.ok) throw new Error(response.status === 422 ? 'Image trop grande, corrompue ou dimensions non acceptées (4096 px maximum).' : `Image non enregistrée (HTTP ${response.status}).`);
      if (library) {
        entries = [...entries.filter(e => e.kind !== kind), { id: kind, kind, url: `${base}/${kind}?v=${crypto.randomUUID()}`, origin: 'manual' }];
      } else await load();
      adding = false;
      replacing = null;
      similarPairs = null;
      await onchanged();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Erreur';
    } finally {
      busy = false;
      if (fileInput) fileInput.value = '';
    }
  }

  async function remove(entry: Entry) {
    if (busy) return;
    busy = true;
    error = '';
    try {
      const response = await fetch(`${base}/${entry.id}`, {
        method: 'DELETE',
        headers: { 'x-csrf-token': $session?.csrfToken ?? '' },
        signal: AbortSignal.timeout(8000)
      });
      if (!response.ok) throw new Error('Image non retirée.');
      if (library) entries = entries.filter(e => e.id !== entry.id);
      else await load();
      similarPairs = null;
      await onchanged();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Erreur';
    } finally {
      busy = false;
    }
  }

  async function move(entry: Entry, direction: number) {
    const images = entries.filter(e => e.kind === 'backdrop');
    const index = images.findIndex(e => e.id === entry.id), next = index + direction;
    if (busy || next < 0 || next >= images.length) return;
    [images[index], images[next]] = [images[next], images[index]];
    busy = true;
    error = '';
    try {
      const response = await fetch(`${base}/order`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify({ ids: images.map(e => e.id) }),
        signal: AbortSignal.timeout(8000)
      });
      if (!response.ok) throw new Error('Ordre non enregistré.');
      entries = [...entries.filter(e => e.kind !== 'backdrop'), ...images];
      await onchanged();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Erreur';
    } finally {
      busy = false;
    }
  }

  function dimensions(event: Event, entry: Entry) {
    if (!entry.width) {
      const image = event.currentTarget as HTMLImageElement;
      entry.width = image.naturalWidth;
      entry.height = image.naturalHeight;
    }
  }

  function provenance(entry: Entry) {
    if (entry.origin === 'manual' || entry.origin === 'legacy') return 'Image manuelle · prioritaire';
    if (entry.origin === 'local') return 'Image locale';
    return [entry.language || 'Langue non déclarée', entry.provider || 'Fournisseur'].join(' · ');
  }

  async function collect() {
    if (busy || !workId) return;
    const languages = [...new Set(imageLanguages.split(',').map(language => language.trim().toLowerCase()).filter(Boolean))];
    if (!languages.length || languages.length > 5 || languages.some(language => !/^[a-z]{2,3}(?:-[a-z0-9]{2,8})*$/.test(language))) {
      error = 'Indiquez une à cinq langues, dans l’ordre de préférence (ex. fr, en).';
      return;
    }
    busy = true;
    error = '';
    collectionMessage = 'Recherche et téléchargement auprès des fournisseurs activés…';
    try {
      const response = await fetch('/api/v1/admin/extensions/images/collect', {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify({ workId, languages, includeNeutral, maxImages: 12 }),
        signal: AbortSignal.timeout(70000)
      });
      if (!response.ok) throw new Error(`Collecte indisponible (HTTP ${response.status}). Vérifiez les fournisseurs activés et les identifiants du média.`);
      const result = await response.json();
      collectionMessage = `${result.downloaded ?? 0} images téléchargées · ${result.duplicates ?? 0} doublons évités · ${result.skipped ?? 0} ignorées · ${Array.isArray(result.errors) ? result.errors.length : result.errors ?? 0} erreurs.${result.partial ? ' Collecte partielle : relancez pour continuer.' : ''}`;
      await load();
      similarPairs = null;
      await onchanged();
    } catch (cause) {
      collectionMessage = '';
      error = cause instanceof Error ? cause.message : 'Collecte indisponible.';
    } finally {
      busy = false;
    }
  }

  // Visual Image Search (Jellyfin-like)
  function openSearch(type = 'poster') {
    if (!workId) return;
    searchType = type;
    searchError = '';
    searchDialog.showModal();
    void fetchCandidates();
  }

  async function fetchCandidates() {
    if (!workId) return;
    searching = true;
    searchError = '';
    candidates = [];
    try {
      const languages = [...new Set(imageLanguages.split(',').map(s => s.trim().toLowerCase()).filter(Boolean))];
      const response = await fetch('/api/v1/admin/extensions/images/candidates', {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify({
          workId,
          imageType: searchType === 'all' ? null : searchType,
          provider: searchProvider === 'all' ? null : searchProvider,
          languages,
          includeNeutral,
          allLanguages: searchAllLanguages
        }),
        signal: AbortSignal.timeout(50000)
      });
      if (!response.ok) {
        const detail = await response.json().catch(() => null);
        if (detail?.error === 'external_ids_required') {
          throw new Error('Ce média n’a pas encore d’identifiant externe (TMDb ou IMDb). Utilisez l’option "Identifier" dans le menu d’actions (...) pour lier la fiche et récupérer ses images officielles.');
        }
        throw new Error(`Recherche indisponible (HTTP ${response.status}).`);
      }
      const data = await response.json();
      candidates = data.images ?? [];
      providersList = data.providers ?? [];
    } catch (cause) {
      searchError = cause instanceof Error ? cause.message : 'Erreur lors de la recherche.';
    } finally {
      searching = false;
    }
  }

  async function applyCandidate(candidate: CandidateImage) {
    if (!workId || applyingUrl) return;
    applyingUrl = candidate.url;
    searchError = '';
    try {
      const response = await fetch(`/api/v1/admin/extensions/${encodeURIComponent(candidate.provider)}/image`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify({
          workId,
          url: candidate.url,
          kind: candidate.imageType,
          language: candidate.language,
          append: candidate.imageType === 'backdrop'
        }),
        signal: AbortSignal.timeout(30000)
      });
      if (!response.ok) throw new Error(`Échec de l’enregistrement (HTTP ${response.status}).`);
      appliedUrls = new Set([...appliedUrls, candidate.url]);
      await load();
      await onchanged();
    } catch (cause) {
      searchError = cause instanceof Error ? cause.message : 'Erreur de téléchargement.';
    } finally {
      applyingUrl = null;
    }
  }

  async function findSimilar() {
    if (busy || !workId) return;
    if (!Number.isInteger(similarityThreshold) || similarityThreshold < 0 || similarityThreshold > 16) {
      error = 'Choisissez une tolérance entre 0 et 16.';
      return;
    }
    busy = true;
    error = '';
    similarPairs = null;
    similarVisible = 20;
    try {
      const response = await fetch(`${base}/similar?threshold=${similarityThreshold}`, { cache: 'no-store', signal: AbortSignal.timeout(15000) });
      if (!response.ok) throw new Error('Comparaison des images indisponible.');
      const result = await response.json() as { pairs: { first: string; second: string }[]; uncompared: number; truncated: boolean };
      similarPairs = result.pairs;
      uncompared = result.uncompared;
      similarTruncated = result.truncated;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Comparaison impossible.';
    } finally {
      busy = false;
    }
  }

  function removeSimilar(id: string) {
    const entry = entries.find(item => item.id === id);
    if (entry && window.confirm('Retirer cette image ? Les fichiers des médias ne seront pas modifiés.')) void remove(entry);
  }
</script>

<dialog bind:this={dialog} class="image-manager" aria-labelledby="image-manager-title" onclose={onclose} oncancel={event => { if (busy) event.preventDefault(); }}>
  <header>
    <button type="button" aria-label="Fermer l’éditeur d’images" disabled={busy} onclick={() => dialog.close()}>←</button>
    <h2 id="image-manager-title">Modifier les images · {title}</h2>
  </header>
  <div class="manager-content">
    {#if error}<p role="alert" class="alert-box">{error}</p>{/if}
    {#if loading}<p role="status">Chargement des images…</p>{/if}

    {#if workId && $session?.user.role === 'administrator'}
      <section class="collection-panel" aria-label="Collecte d’images">
        <h3>Enrichir les images</h3>
        <p>Les fournisseurs activés recherchent les images à partir des identifiants de ce média. Vous pouvez utiliser la recherche visuelle ci-dessous avec la loupe 🔍 ou lancer une collecte automatique.</p>
        <label>Langues et secours, dans l’ordre
          <input bind:value={imageLanguages} disabled={busy} placeholder="fr, en" maxlength="100" />
        </label>
        <label class="neutral-language">
          <input type="checkbox" bind:checked={includeNeutral} disabled={busy} />
          Inclure les images sans langue déclarée
        </label>
        <button type="button" disabled={busy || loading} onclick={collect}>Collecter automatiquement les images</button>
        {#if collectionMessage}<p role="status">{collectionMessage}</p>{/if}
      </section>

      <section class="collection-panel" aria-label="Images similaires">
        <h3>Comparer les images enregistrées</h3>
        <p>Repérez les images proches avant de choisir celles à retirer. La comparaison est une estimation et ne supprime rien automatiquement.</p>
        <label>Tolérance aux différences
          <input type="number" min="0" max="16" step="1" bind:value={similarityThreshold} disabled={busy} />
        </label>
        <button type="button" disabled={busy || loading} onclick={findSimilar}>Rechercher les images similaires</button>
        {#if similarPairs !== null}
          <p role="status">{similarPairs.length} paire{similarPairs.length > 1 ? 's' : ''} trouvée{similarPairs.length > 1 ? 's' : ''}. Images sans empreinte non comparées : {uncompared}.</p>
          {#if similarTruncated}<p>La liste est limitée à 128 paires. Relancez la comparaison après avoir retiré les images voulues.</p>{/if}
          {#each similarPairs.slice(0, similarVisible) as pair (`${pair.first}:${pair.second}`)}
            <div class="similar-pair">
              {#each [pair.first, pair.second] as id (id)}
                {@const entry = entries.find(item => item.id === id)}
                {#if entry}
                  <div>
                    <img src={imageUrl(entry)} alt={`${labels[entry.kind]} de ${title}`} loading="lazy" />
                    <p>{provenance(entry)}</p>
                    <button type="button" disabled={busy} onclick={() => removeSimilar(id)}>Retirer cette image</button>
                  </div>
                {/if}
              {/each}
            </div>
          {/each}
          {#if similarVisible < similarPairs.length}
            <button type="button" onclick={() => similarVisible += 20}>Afficher davantage de résultats</button>
          {/if}
        {/if}
      </section>
    {/if}

    {#if adding}
      <form class="upload-panel" onsubmit={event => event.preventDefault()}>
        <fieldset disabled={busy}>
          <legend>{replacing ? 'Remplacer une image' : 'Ajouter une image'}</legend>
          <label for="artwork-type">Type d’image</label>
          <select id="artwork-type" bind:value={kind} disabled={!!replacing}>
            {#each kinds as type(type)}<option value={type}>{labels[type]}</option>{/each}
          </select>
          <label for="artwork-file">PNG, JPEG ou WebP · 2 Mio maximum</label>
          <input id="artwork-file" bind:this={fileInput} type="file" accept="image/png,image/jpeg,image/webp" onchange={upload} />
          <p>Les images sont conservées dans les données applicatives. Les fichiers de vos médias restent intacts.</p>
          <button type="button" onclick={() => adding = false}>Annuler</button>
        </fieldset>
      </form>
    {/if}

    {#each [false, true] as backdrops(backdrops)}
      {@const sectionType = backdrops ? 'backdrop' : 'poster'}
      <section aria-label={backdrops ? 'Arrière-plans' : 'Images'}>
        <div class="section-heading">
          <h3>{backdrops ? 'Arrière-plans' : 'Images'}</h3>
          <div class="heading-actions">
            {#if workId && $session?.user.role === 'administrator'}
              <button
                class="round search-action-btn"
                type="button"
                disabled={busy}
                title={`Rechercher des ${backdrops ? 'arrière-plans' : 'affiches'} en ligne`}
                aria-label={`Rechercher des ${backdrops ? 'arrière-plans' : 'affiches'}`}
                onclick={() => openSearch(sectionType)}
              >
                <Icon name="search" size={18} />
              </button>
            {/if}
            <button
              class="round"
              type="button"
              disabled={busy}
              aria-label={backdrops ? 'Ajouter un arrière-plan' : 'Ajouter une image'}
              onclick={() => add(sectionType)}
            >+</button>
          </div>
        </div>

        <div class="image-grid">
          {#each entries.filter(e => (e.kind === 'backdrop') === backdrops).sort((a, b) => kinds.indexOf(a.kind) - kinds.indexOf(b.kind)) as entry(entry.id)}
            <article class="image-card" data-kind={entry.kind}>
              <button class="image-preview" type="button" aria-label={`Agrandir ${labels[entry.kind]}`} onclick={() => { selected = entry; preview.showModal(); }}>
                <img src={imageUrl(entry)} alt={`${labels[entry.kind]} de ${title}`} loading="lazy" decoding="async" onload={event => dimensions(event, entry)} />
              </button>
              <div class="card-caption">
                <h4>{labels[entry.kind]}</h4>
                <p>{provenance(entry)}</p>
                {#if entry.width && entry.height}<p class="res-tag">{entry.width} × {entry.height}</p>{/if}
                <div class="card-actions">
                  <button type="button" aria-label={`Agrandir ${labels[entry.kind]}`} title="Agrandir" onclick={() => { selected = entry; preview.showModal(); }}>
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/></svg>
                  </button>
                  {#if workId && $session?.user.role === 'administrator'}
                    <button type="button" aria-label={`Rechercher des remplaçants pour ${labels[entry.kind]}`} title="Rechercher des images" onclick={() => openSearch(entry.kind)}>
                      <Icon name="search" size={18} />
                    </button>
                  {/if}
                  <button type="button" disabled={busy} aria-label={`Remplacer ${labels[entry.kind]}`} title="Remplacer par un fichier" onclick={() => add(entry.kind, entry)}>↥</button>
                  {#if backdrops && workId}
                    <button type="button" disabled={busy || entries.filter(e => e.kind === 'backdrop')[0]?.id === entry.id} aria-label="Déplacer l’arrière-plan à gauche" onclick={() => move(entry, -1)}>‹</button>
                    <button type="button" disabled={busy || entries.filter(e => e.kind === 'backdrop').at(-1)?.id === entry.id} aria-label="Déplacer l’arrière-plan à droite" onclick={() => move(entry, 1)}>›</button>
                  {/if}
                  <button type="button" class="danger" disabled={busy} aria-label={`Supprimer ${labels[entry.kind]}`} title={entry.origin === 'local' ? 'Masquer cette image dans le catalogue, sans supprimer le fichier' : 'Supprimer cette image personnalisée'} onclick={() => remove(entry)}>
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7" /></svg>
                  </button>
                </div>
              </div>
            </article>
          {:else}
            <p class="empty">{loading ? 'Chargement…' : 'Aucune image. Utilisez 🔍 pour en chercher ou + pour en ajouter.'}</p>
          {/each}
        </div>
      </section>
    {/each}
    {#if busy}<p role="status">Enregistrement…</p>{/if}
  </div>
</dialog>

<!-- Visual Search Modal (Jellyfin-like candidate selector) -->
<dialog bind:this={searchDialog} class="search-modal" aria-labelledby="search-modal-title">
  <header class="search-header">
    <div class="search-title-bar">
      <button type="button" class="back-btn" aria-label="Retour" onclick={() => searchDialog.close()}>←</button>
      <h3 id="search-modal-title">Rechercher des images</h3>
    </div>
    <div class="search-controls">
      <label>Source
        <select bind:value={searchProvider} onchange={fetchCandidates}>
          <option value="all">Tout</option>
          {#each providersList as p (p.id)}
            <option value={p.id}>{p.name}</option>
          {/each}
        </select>
      </label>
      <label>Type
        <select bind:value={searchType} onchange={fetchCandidates}>
          <option value="all">Tous types</option>
          <option value="poster">Affiche (Principal)</option>
          <option value="backdrop">Arrière-plan</option>
          <option value="logo">Logo</option>
          <option value="banner">Bannière</option>
          <option value="thumb">Vignette</option>
        </select>
      </label>
      <label class="all-lang-toggle">
        <input type="checkbox" bind:checked={searchAllLanguages} onchange={fetchCandidates} />
        Toutes les langues
      </label>
      <span class="count-badge">
        {candidates.length > 0 ? `1-${candidates.length} de ${candidates.length}` : '0 image'}
      </span>
    </div>
  </header>

  <div class="search-body">
    {#if searchError}
      <p class="search-error" role="alert">{searchError}</p>
    {/if}
    {#if searching}
      <div class="search-loading" role="status">
        <div class="spinner"></div>
        <p>Recherche d’images auprès des fournisseurs…</p>
      </div>
    {:else if candidates.length === 0}
      <p class="search-empty">Aucune image trouvée pour ces critères auprès des fournisseurs configurés.</p>
    {:else}
      <div class="candidates-grid">
        {#each candidates as candidate(candidate.url)}
          <article class="candidate-card">
            <div class="candidate-preview" class:is-backdrop={candidate.imageType === 'backdrop'}>
              <img src={candidate.url} alt={`Aperçu ${candidate.imageType}`} loading="lazy" />
            </div>
            <div class="candidate-meta">
              <h4>{candidate.providerName || candidate.provider}</h4>
              <p class="candidate-res">{candidate.width && candidate.height ? `${candidate.width} × ${candidate.height}` : labels[candidate.imageType] || candidate.imageType}</p>
              {#if candidate.rating !== undefined && candidate.rating !== null}
                <p class="candidate-score">★ {candidate.rating.toFixed(1)}{candidate.voteCount ? ` • ${candidate.voteCount} votes` : ''}</p>
              {/if}
              {#if candidate.language}
                <span class="lang-tag">{candidate.language.toUpperCase()}</span>
              {/if}
              <div class="candidate-action">
                {#if applyingUrl === candidate.url}
                  <span class="applying-tag">Téléchargement…</span>
                {:else if appliedUrls.has(candidate.url)}
                  <span class="applied-tag">✓ Enregistré</span>
                {:else}
                  <button
                    type="button"
                    class="download-btn"
                    title="Télécharger et appliquer cette image"
                    aria-label="Télécharger et appliquer"
                    onclick={() => applyCandidate(candidate)}
                  >
                    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M4 14.899A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.242M12 12v9m-4-4 4 4 4-4"/>
                    </svg>
                  </button>
                {/if}
              </div>
            </div>
          </article>
        {/each}
      </div>
    {/if}
  </div>
</dialog>

<dialog class="zoom" bind:this={preview} aria-label="Aperçu de l’image">
  <button type="button" onclick={() => preview.close()}>Fermer</button>
  {#if selected}<img src={imageUrl(selected, false)} alt={`${labels[selected.kind]} de ${title}`} />{/if}
</dialog>

<style>
  .collection-panel { display: grid; gap: 12px; padding: 18px; background: var(--color-raised); border-radius: var(--radius-md); }
  .collection-panel p { margin: 0; }
  .collection-panel label { display: grid; gap: 6px; }
  .collection-panel .neutral-language { display: flex; align-items: center; gap: 10px; }
  .neutral-language input { min-height: 20px; width: 20px; }
  .collection-panel button { justify-self: start; padding: 8px 14px; }
  .similar-pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .similar-pair img { width: 100%; height: 180px; object-fit: contain; background: var(--color-bg); }
  .similar-pair div { min-width: 0; }
  .image-manager { width: min(1500px, calc(100vw - 24px)); max-width: none; max-height: calc(100dvh - 24px); padding: 0; border: 1px solid var(--color-line); border-radius: 14px; background: var(--color-surface); color: var(--color-text); }
  dialog::backdrop { background: rgb(0 0 0 / .85); }
  header { position: sticky; top: 0; display: flex; align-items: center; gap: 14px; padding: 12px 18px; background: var(--color-surface); border-bottom: 1px solid var(--color-line); z-index: 2; }
  h2 { font-size: 17px; font-weight: 600; margin: 0; overflow-wrap: anywhere; }
  .manager-content { padding: 24px; }
  section { margin-bottom: 36px; }
  .section-heading { display: flex; align-items: center; justify-content: space-between; gap: 14px; margin-bottom: 22px; }
  .heading-actions { display: flex; align-items: center; gap: 10px; }
  h3 { font-size: 18px; margin: 0; }
  button { color: var(--color-text); background: transparent; border: 1px solid var(--color-line); border-radius: 6px; min-width: 40px; min-height: 40px; font: inherit; cursor: pointer; display: inline-flex; align-items: center; justify-content: center; }
  button:disabled { opacity: .4; cursor: default; }
  button:hover:not(:disabled) { background: var(--color-raised); }
  .round { border-radius: 50%; font-size: 26px; }
  .search-action-btn { background: var(--color-raised); }
  .image-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 270px), 1fr)); gap: 24px; }
  .image-card { border: 1px solid var(--color-line); border-radius: 12px; overflow: hidden; min-width: 0; background: var(--color-surface); }
  .image-preview { display: block; width: 100%; aspect-ratio: 16/9; border: 0; border-radius: 0; padding: 0; background: var(--color-bg); }
  .image-card[data-kind='poster'] .image-preview { aspect-ratio: 2/3; }
  .image-preview img { width: 100%; height: 100%; object-fit: contain; }
  .card-caption { text-align: center; padding: 10px; }
  h4 { font-size: 16px; font-weight: 500; margin: 0; }
  p { color: var(--color-muted); font-size: 13px; line-height: 1.5; }
  .card-caption p { margin: 4px 0; }
  .res-tag { font-family: monospace; font-size: 12px; opacity: .8; }
  .card-actions { display: flex; justify-content: center; gap: 6px; margin-top: 8px; }
  .card-actions button { border: 0; font-size: 20px; }
  .card-actions .danger:hover { color: var(--color-danger, #ff4d4f); }
  .upload-panel { background: var(--color-raised); border-radius: var(--radius-md); margin-bottom: 26px; padding: 18px; }
  fieldset { min-width: 0; display: grid; gap: 12px; border: 0; margin: 0; padding: 0; }
  input, select { max-width: 100%; min-height: 44px; color: var(--color-text); background: var(--color-surface); border: 1px solid var(--color-line); border-radius: 6px; padding: 8px; }
  .alert-box { padding: 10px 14px; background: rgba(239, 68, 68, 0.15); border: 1px solid rgba(239, 68, 68, 0.3); border-radius: 6px; color: #ff6b6b; margin-bottom: 16px; }
  .zoom { max-width: calc(100vw - 24px); max-height: calc(100dvh - 24px); background: var(--color-surface); color: var(--color-text); border: 1px solid var(--color-line); border-radius: var(--radius-md); }
  .zoom img { display: block; max-width: 100%; max-height: 80dvh; object-fit: contain; margin-top: 12px; }

  /* Jellyfin-style Image Search Modal */
  .search-modal { width: min(1400px, calc(100vw - 24px)); max-height: calc(100dvh - 24px); padding: 0; border: 1px solid var(--color-line-strong); border-radius: var(--radius-xl); background: var(--color-glass-raised); backdrop-filter: blur(24px); -webkit-backdrop-filter: blur(24px); color: var(--color-text-bright); box-shadow: var(--shadow-lg); }
  .search-header { display: flex; flex-direction: column; gap: 12px; padding: 16px 22px; background: var(--color-surface); border-bottom: 1px solid var(--color-line); position: sticky; top: 0; z-index: 10; }
  .search-title-bar { display: flex; align-items: center; gap: 12px; }
  .back-btn { background: transparent; border: none; font-size: 24px; cursor: pointer; color: inherit; width: 38px; height: 38px; border-radius: 50%; }
  .back-btn:hover { background: var(--color-raised); }
  .search-title-bar h3 { margin: 0; font-size: 18px; font-weight: 650; }
  .search-controls { display: flex; align-items: center; gap: 20px; flex-wrap: wrap; }
  .search-controls label { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--color-muted); }
  .search-controls select { background: var(--color-raised); color: var(--color-text-bright); border: 1px solid var(--color-line); border-radius: var(--radius-sm); padding: 6px 12px; min-height: 36px; }
  .all-lang-toggle { cursor: pointer; }
  .all-lang-toggle input { width: 16px; height: 16px; min-height: 16px; }
  .count-badge { margin-left: auto; font-size: 13px; color: var(--color-muted); }
  .search-body { padding: 24px; min-height: 360px; }
  .search-error { color: var(--color-danger); background: var(--color-danger-soft); padding: 12px; border-radius: var(--radius-sm); border: 1px solid rgba(239, 68, 68, 0.3); }
  .search-loading { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 60px 0; gap: 16px; color: var(--color-muted); }
  .spinner { width: 36px; height: 36px; border: 3px solid var(--color-line-strong); border-top-color: var(--color-accent); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .search-empty { text-align: center; color: var(--color-muted); padding: 60px 0; font-size: 15px; }

  /* Candidate Cards Grid */
  .candidates-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: 20px; }
  .candidate-card { background: var(--color-surface); border: 1px solid var(--color-line); border-radius: var(--radius-md); overflow: hidden; display: flex; flex-direction: column; transition: transform var(--transition-fast), border-color var(--transition-fast), box-shadow var(--transition-fast); }
  .candidate-card:hover { transform: translateY(-3px); border-color: var(--color-accent); box-shadow: var(--shadow-md); }
  .candidate-preview { width: 100%; aspect-ratio: 2 / 3; background: var(--color-canvas); display: flex; align-items: center; justify-content: center; overflow: hidden; }
  .candidate-preview.is-backdrop { aspect-ratio: 16 / 9; }
  .candidate-preview img { width: 100%; height: 100%; object-fit: contain; }
  .candidate-meta { padding: 14px; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 4px; }
  .candidate-meta h4 { font-size: 14px; font-weight: 600; color: var(--color-text-bright); margin: 0; }
  .candidate-res { font-size: 12px; color: var(--color-muted); font-family: monospace; margin: 2px 0; }
  .candidate-score { font-size: 12px; color: #FBBF24; margin: 0; font-weight: 600; }
  .lang-tag { display: inline-block; font-size: 10px; font-weight: 700; background: var(--color-accent-soft); color: var(--color-accent); padding: 2px 7px; border-radius: var(--radius-pill); margin-top: 4px; }
  .candidate-action { margin-top: 10px; }
  .download-btn { background: var(--color-raised); border: 1px solid var(--color-line-strong); color: var(--color-text-bright); border-radius: 50%; width: 44px; height: 44px; display: inline-flex; align-items: center; justify-content: center; cursor: pointer; transition: background var(--transition-fast), color var(--transition-fast), transform var(--transition-fast); }
  .download-btn:hover { background: var(--color-play); color: var(--color-on-play); border-color: var(--color-play); transform: scale(1.06); }
  .applied-tag { color: var(--color-success); font-size: 13px; font-weight: 600; }
  .applying-tag { color: var(--color-accent); font-size: 13px; }

  @media(max-width: 600px) {
    .manager-content { padding: 16px; }
    .image-grid { grid-template-columns: repeat(auto-fill, minmax(min(100%, 220px), 1fr)); }
    .candidates-grid { grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); }
  }
</style>

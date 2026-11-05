<script lang="ts">
  import { providerErrorText } from './provider-errors';
  import { providerImagePreview } from './provider-image';
  import { preferences } from '../preferences';
  import { artworkCategories } from './artwork-language';
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
    pinned?: boolean;
    representationCount?: number;
    originalReleaseVerified?: boolean;
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
  let uploadLanguage = $state(''), originalLanguage = $state<string | null>(null);
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
    const result = await response.json();
    const incoming: Entry[] = result.images;
    originalLanguage = result.originalLanguage ?? null;
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
      const query = workId ? new URLSearchParams(replacing ? { replace: replacing.id } : { append: 'true' }) : new URLSearchParams();
      if (workId && uploadLanguage.trim()) query.set('language', uploadLanguage.trim());
      const response = await fetch(`${base}/${kind}?${query}`, {
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

  async function togglePin(entry: Entry) {
    if (busy || !workId) return;
    busy = true; error = '';
    try {
      const response = await fetch(`${base}/${encodeURIComponent(entry.id)}/pin`, {
        method: 'PUT', headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify({ pinned: !entry.pinned }), signal: AbortSignal.timeout(8000)
      });
      if (!response.ok) throw new Error('Épinglage non enregistré.');
      await load(); await onchanged();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
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

  // Les vignettes sont réduites (?width=320) : leur taille naturelle n'est pas
  // celle du fichier. Quand le Core ne connaît pas les dimensions (images locales),
  // on mesure l'original une seule fois, en arrière-plan, avec un cache par URL.
  // Caches internes non affichés : les dimensions sont reportées sur l'entrée réactive.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const originalSizes = new Map<string, { width: number; height: number }>();
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const probes = new Set<string>();
  function dimensions(event: Event, entry: Entry) {
    if (entry.width && entry.height) return;
    if (!workId) {
      // Sans paramètre width, l'aperçu est déjà l'original.
      const image = event.currentTarget as HTMLImageElement;
      entry.width = image.naturalWidth;
      entry.height = image.naturalHeight;
      return;
    }
    const known = originalSizes.get(entry.url);
    if (known) { entry.width = known.width; entry.height = known.height; return; }
    if (probes.has(entry.url)) return;
    probes.add(entry.url);
    const original = new Image();
    original.decoding = 'async';
    original.onload = () => {
      probes.delete(entry.url);
      if (!original.naturalWidth) return;
      const size = { width: original.naturalWidth, height: original.naturalHeight };
      originalSizes.set(entry.url, size);
      entry.width = size.width;
      entry.height = size.height;
    };
    original.onerror = () => probes.delete(entry.url);
    original.src = entry.url;
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
      if (!candidates.length && data.errors?.length) {
        searchError = data.errors.map((failure: {provider: string; providerName?: string; error?: string; status: number}) => `${failure.providerName ?? failure.provider} : ${failure.error ? providerErrorText(failure.error) : `HTTP ${failure.status}`}`).join(' · ');
      }
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
        <p>Les fournisseurs activés recherchent les images à partir des identifiants de ce média. Vous pouvez utiliser la recherche visuelle ci-dessous avec le bouton loupe « Rechercher des images » ou lancer une collecte automatique.</p>
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
          {#if workId}<label>Langue de l’image (facultatif, ex. fr-CA)<input bind:value={uploadLanguage} maxlength="35" placeholder="Inconnue" /></label>{/if}
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
                {#if workId}<p>{artworkCategories(entry.language, originalLanguage).join(' · ')}{entry.pinned ? ' · Épinglée' : ''}</p>{/if}
                {#if entry.representationCount}<p>{entry.representationCount} représentations</p>{/if}
                {#if entry.width && entry.height}<p class="res-tag">{entry.width} × {entry.height}</p>{/if}
                <div class="card-actions">
                  <button type="button" aria-label={`Agrandir ${labels[entry.kind]}`} title="Agrandir" onclick={() => { selected = entry; preview.showModal(); }}>
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/></svg>
                  </button>
                  {#if workId && $session?.user.role === 'administrator'}
                    <button type="button" disabled={busy} aria-label={entry.pinned ? 'Désépingler cette image' : 'Épingler cette image'} title={entry.pinned ? 'Désépingler' : 'Épingler pour tous les utilisateurs'} onclick={() => togglePin(entry)}>{entry.pinned ? '◆' : '◇'}</button>
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
            <p class="empty">{loading ? 'Chargement…' : 'Aucune image. Utilisez le bouton loupe pour en chercher ou + pour en ajouter.'}</p>
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
              <img src={providerImagePreview(candidate.provider, workId ?? '', candidate.url)} alt={`Aperçu ${candidate.imageType}`} loading="lazy" />
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
  /* Dialog shells */
  .image-manager, .search-modal, .zoom {
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    background: var(--bg-3);
    color: var(--tx);
    box-shadow: var(--shadow);
    overscroll-behavior: contain;
  }
  .image-manager { width: min(1500px, calc(100vw - 48px)); max-width: none; max-height: calc(100dvh - 48px); }
  .search-modal { width: min(1400px, calc(100vw - 48px)); max-width: none; max-height: calc(100dvh - 48px); }
  dialog::backdrop { background: rgb(6 8 14 / 0.55); backdrop-filter: blur(4px); -webkit-backdrop-filter: blur(4px); }
  :global(body:has(.image-manager[open])) { overflow: hidden; }

  header { position: sticky; top: 0; z-index: 2; display: flex; align-items: center; gap: 14px; padding: 20px 28px 16px; background: var(--bg-3); border-bottom: 1px solid var(--line); }
  h2 { font-size: 24px; font-weight: 800; letter-spacing: -0.02em; margin: 0; overflow-wrap: anywhere; min-width: 0; }
  .manager-content { padding: 24px 28px 28px; }
  section { margin-bottom: 36px; }
  .section-heading { display: flex; align-items: center; justify-content: space-between; gap: 14px; margin-bottom: 18px; }
  .heading-actions { display: flex; align-items: center; gap: 10px; }
  h3 { font-size: 17px; font-weight: 700; margin: 0; }
  h4 { font-size: 14px; font-weight: 700; margin: 0; }
  p { color: var(--mu); font-size: 13px; line-height: 1.5; }

  /* Buttons */
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-width: 44px;
    min-height: 44px;
    padding: 0 16px;
    color: var(--tx);
    background: var(--pn);
    border: 1px solid transparent;
    border-radius: var(--r-md);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease), color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button:hover:not(:disabled) { background: var(--pn-2); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  header > button, .back-btn { width: 44px; padding: 0; border-radius: 14px; font-size: 20px; flex: none; }
  .round { width: 44px; padding: 0; border-radius: 14px; font-size: 24px; font-weight: 500; }
  .search-action-btn { background: var(--pn); }

  /* Panels */
  .collection-panel { display: grid; gap: 12px; padding: 20px; margin-bottom: 20px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .collection-panel p { margin: 0; }
  .collection-panel label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  .collection-panel .neutral-language { display: flex; align-items: center; gap: 10px; min-height: 44px; font-weight: 600; }
  .neutral-language input { min-height: 18px; width: 18px; height: 18px; }
  .collection-panel button { justify-self: start; }
  .similar-pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .similar-pair img { display: block; width: 100%; height: 180px; object-fit: contain; background: var(--pn); border-radius: var(--r-sm); }
  .similar-pair div { min-width: 0; }
  .similar-pair p { font-size: 13px; margin: 6px 0; }

  .upload-panel { margin-bottom: 26px; padding: 20px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  fieldset { min-width: 0; display: grid; gap: 10px; border: 0; margin: 0; padding: 0; }
  legend { padding: 0; margin-bottom: 8px; font-size: 17px; font-weight: 700; color: var(--tx); }
  fieldset > label { font-size: 14px; font-weight: 700; color: var(--tx); }
  fieldset > button { justify-self: start; }

  /* Inputs */
  input, select {
    box-sizing: border-box;
    max-width: 100%;
    min-height: 46px;
    padding: 0 14px;
    color: var(--tx);
    background: var(--pn);
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--t-fast) var(--ease), background-color var(--t-fast) var(--ease);
  }
  input[type='file'] { padding: 10px 14px; }
  input[type='checkbox'] { padding: 0; accent-color: var(--ac); background: none; }
  select option { background: var(--bg-3); color: var(--tx); }
  input:focus, select:focus { outline: none; border-color: var(--ac); background: var(--bg-3); box-shadow: none; }

  .alert-box { padding: 12px 16px; margin: 0 0 16px; background: var(--err-soft); border-radius: var(--r-sm); color: var(--err); font-size: 14px; font-weight: 600; }

  /* Image grid: the image is the card */
  .image-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 240px), 1fr)); gap: 24px; }
  .image-card { min-width: 0; }
  .image-preview { display: block; width: 100%; aspect-ratio: 16/9; padding: 0; border: 0; border-radius: var(--r-md); overflow: hidden; background: var(--pn); }
  .image-preview:hover:not(:disabled) { background: var(--pn-2); }
  .image-card[data-kind='poster'] .image-preview { aspect-ratio: 2/3; }
  .image-preview img { display: block; width: 100%; height: 100%; object-fit: contain; }
  .card-caption { text-align: center; padding: 10px 4px 0; }
  .card-caption p { margin: 2px 0; font-size: 13px; }
  .res-tag { font-family: var(--font-mono); font-size: 12px; font-variant-numeric: tabular-nums; }
  .card-actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 4px; margin-top: 8px; }
  .card-actions button { width: 44px; padding: 0; border-radius: 14px; background: transparent; font-size: 20px; font-weight: 500; }
  .card-actions button:hover:not(:disabled) { background: var(--pn); }
  .card-actions .danger { color: var(--err); }
  .empty { margin: 0; }

  /* Zoom preview */
  .zoom { max-width: calc(100vw - 48px); max-height: calc(100dvh - 48px); padding: 16px; }
  .zoom img { display: block; max-width: 100%; max-height: 80dvh; object-fit: contain; margin-top: 12px; border-radius: var(--r-md); }

  /* Image search dialog */
  .search-header { display: flex; flex-direction: column; align-items: stretch; gap: 14px; padding: 20px 28px 16px; background: var(--bg-3); border-bottom: 1px solid var(--line); position: sticky; top: 0; z-index: 10; }
  .search-title-bar { display: flex; align-items: center; gap: 12px; }
  .search-title-bar h3 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  .search-controls { display: flex; align-items: center; gap: 12px 20px; flex-wrap: wrap; }
  .search-controls label { display: flex; align-items: center; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  .all-lang-toggle { min-height: 44px; cursor: pointer; font-weight: 600; }
  .all-lang-toggle input { width: 18px; height: 18px; min-height: 18px; }
  .count-badge { margin-left: auto; font-size: 13px; color: var(--mu); font-variant-numeric: tabular-nums; }
  .search-body { padding: 24px 28px 28px; min-height: 360px; }
  .search-error { margin: 0 0 16px; padding: 12px 16px; color: var(--err); background: var(--err-soft); border-radius: var(--r-sm); font-size: 14px; font-weight: 600; }
  .search-loading { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 60px 0; gap: 16px; color: var(--mu); }
  .spinner { width: 36px; height: 36px; border: 3px solid var(--line-2); border-top-color: var(--ac); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .search-empty { text-align: center; color: var(--mu); padding: 60px 0; font-size: 15px; }

  .candidates-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 20px; }
  .candidate-card { display: flex; flex-direction: column; min-width: 0; }
  .candidate-preview { width: 100%; aspect-ratio: 2 / 3; background: var(--pn); border-radius: var(--r-md); display: flex; align-items: center; justify-content: center; overflow: hidden; outline: 3px solid transparent; outline-offset: 2px; transition: outline-color var(--t-fast) var(--ease); }
  .candidate-preview.is-backdrop { aspect-ratio: 16 / 9; }
  .candidate-preview img { width: 100%; height: 100%; object-fit: contain; }
  .candidate-card:has(.applied-tag) .candidate-preview { outline-color: var(--ac); }
  .candidate-meta { padding: 10px 4px 0; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 4px; }
  .candidate-meta h4 { font-size: 14px; font-weight: 700; color: var(--tx); margin: 0; }
  .candidate-res { font-size: 13px; color: var(--mu); font-family: var(--font-mono); margin: 0; }
  .candidate-score { font-size: 13px; color: var(--ac); margin: 0; font-weight: 700; }
  .lang-tag { display: inline-block; font-size: 12px; font-weight: 700; background: var(--pn-2); color: var(--mu); padding: 2px 8px; border-radius: var(--r-pill); margin-top: 2px; }
  .candidate-action { margin-top: 8px; min-height: 44px; display: flex; align-items: center; }
  .download-btn { width: 44px; height: 44px; padding: 0; border-radius: 14px; background: var(--pn); }
  .download-btn:hover:not(:disabled) { background: var(--btn); color: var(--btn-tx); }
  .applied-tag { color: var(--ok); font-size: 13px; font-weight: 700; }
  .applying-tag { color: var(--mu); font-size: 13px; font-weight: 600; }

  @media (max-width: 639px) {
    .image-manager, .search-modal {
      width: 100vw;
      height: 100dvh;
      max-height: 100dvh;
      margin: 0;
      border: 0;
      border-radius: 0;
    }
    header, .search-header { padding: 16px; }
    h2, .search-title-bar h3 { font-size: 20px; }
    .manager-content, .search-body { padding: 16px; }
    .image-grid { grid-template-columns: repeat(auto-fill, minmax(min(100%, 160px), 1fr)); gap: 16px; }
    .candidates-grid { grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 14px; }
    .count-badge { margin-left: 0; }
  }
</style>

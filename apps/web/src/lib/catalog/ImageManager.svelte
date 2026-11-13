<script lang="ts">
  import { providerErrorText } from './provider-errors';
  import { providerImagePreview } from './provider-image';
  import { preferences } from '../preferences';
  import { artworkCategories } from './artwork-language';
  import Icon from '../ui/Icon.svelte';
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import type { Library } from '../admin/types';
  import { t } from '../i18n';

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
  let uploadLanguage = $state(''), originalLanguage = $state<string | null>(null), artworkLocked = $state(false);
  let similarPairs = $state<{ first: string; second: string }[] | null>(null), uncompared = $state(0), similarVisible = $state(20), similarTruncated = $state(false);

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
  const groups = $derived(library
    ? [{ id: 'poster', title: $t('main_image'), kinds: ['poster'] }, { id: 'backdrop', title: labels.backdrop, kinds: ['backdrop'] }]
    : [
      { id: 'poster', title: $t('posters'), kinds: ['poster'] },
      { id: 'logo', title: labels.logo, kinds: ['logo'] },
      { id: 'backdrop', title: $t('backdrops'), kinds: ['backdrop'] },
      { id: 'other', title: $t('other_images'), kinds: ['thumb', 'landscape', 'banner', 'disc', 'illustration'] }
    ]);
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
    if (!response.ok) throw new Error($t('images_unavailable'));
    const result = await response.json();
    const incoming: Entry[] = result.images;
    originalLanguage = result.originalLanguage ?? null;
    artworkLocked = result.artworkLocked === true;
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
      error = $t('catalog_choose_a_png_jpeg_or_webp');
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
      if (!response.ok) throw new Error(response.status === 422 ? $t('image_too_large_corrupted_or_dimensions') : $t('image_not_saved_http', { status: response.status }));
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
      if (!response.ok) throw new Error($t('image_not_removed'));
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
      if (!response.ok) throw new Error($t('pin_not_saved'));
      await load(); await onchanged();
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Erreur'; }
    finally { busy = false; }
  }

  async function toggleLock() {
    if (busy || !workId) return;
    busy = true; error = '';
    const next = !artworkLocked;
    try {
      const response = await fetch(`${base}/lock`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify({ locked: next }),
        signal: AbortSignal.timeout(8000)
      });
      if (!response.ok) throw new Error($t('pin_not_saved'));
      artworkLocked = next;
      if (next) collectionMessage = $t('images_locked_aether_will_not_fill_in');
      else collectionMessage = $t('images_unlocked');
      await onchanged();
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
      if (!response.ok) throw new Error($t('order_not_saved'));
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
    if (entry.origin === 'manual' || entry.origin === 'legacy') return $t('manual_image_priority');
    if (entry.origin === 'local') return $t('local_image');
    return [entry.language || $t('language_not_declared'), entry.provider || 'Fournisseur'].join(' · ');
  }

  async function collect() {
    if (busy || !workId) return;
    busy = true;
    error = '';
    collectionMessage = $t('searching_and_downloading_from_enabled');
    try {
      const response = await fetch('/api/v1/admin/extensions/images/collect', {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-csrf-token': $session?.csrfToken ?? '' },
        body: JSON.stringify({ workId }),
        signal: AbortSignal.timeout(70000)
      });
      if (!response.ok) throw new Error($t('collection_unavailable_http_check_the', { status: response.status }));
      const result = await response.json();
      collectionMessage = `${result.downloaded ?? 0} images téléchargées · ${result.duplicates ?? 0} doublons évités · ${result.skipped ?? 0} ignorées · ${Array.isArray(result.errors) ? result.errors.length : result.errors ?? 0} erreurs.${result.partial ? ' Collecte partielle : relancez pour continuer.' : ''}`;
      await load();
      similarPairs = null;
      await onchanged();
    } catch (cause) {
      collectionMessage = '';
      error = cause instanceof Error ? cause.message : $t('catalog_collection_unavailable');
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
          throw new Error($t('this_media_item_has_no_external'));
        }
        throw new Error($t('search_unavailable_http', { status: response.status }));
      }
      const data = await response.json();
      candidates = data.images ?? [];
      providersList = data.providers ?? [];
      if (!candidates.length && data.errors?.length) {
        searchError = data.errors.map((failure: {provider: string; providerName?: string; error?: string; status: number}) => `${failure.providerName ?? failure.provider} : ${failure.error ? providerErrorText(failure.error) : `HTTP ${failure.status}`}`).join(' · ');
      }
    } catch (cause) {
      searchError = cause instanceof Error ? cause.message : $t('error_during_the_search');
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
      if (response.status === 409) throw new Error($t('this_work_has_reached_the_limit'));
      if (!response.ok) throw new Error($t('saving_failed_http', { status: response.status }));
      appliedUrls = new Set([...appliedUrls, candidate.url]);
      await load();
      await onchanged();
    } catch (cause) {
      searchError = cause instanceof Error ? cause.message : $t('download_error');
    } finally {
      applyingUrl = null;
    }
  }

  async function findSimilar() {
    if (busy || !workId) return;
    busy = true;
    error = '';
    similarPairs = null;
    similarVisible = 20;
    try {
      const response = await fetch(`${base}/similar`, { cache: 'no-store', signal: AbortSignal.timeout(15000) });
      if (!response.ok) throw new Error($t('image_comparison_unavailable'));
      const result = await response.json() as { pairs: { first: string; second: string }[]; uncompared: number; truncated: boolean };
      similarPairs = result.pairs;
      uncompared = result.uncompared;
      similarTruncated = result.truncated;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('comparison_failed');
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
    <button type="button" aria-label={$t('close_the_image_editor')} disabled={busy} onclick={() => dialog.close()}>←</button>
    <h2 id="image-manager-title">{$t('edit_images', { title })}</h2>
  </header>
  <div class="manager-content">
    {#if error}<p role="alert" class="alert-box">{error}</p>{/if}
    {#if loading}<p role="status">{$t('loading_images')}</p>{/if}

    {#if workId && $session?.user.role === 'administrator'}
      <div class="toolbar" role="group" aria-label={$t('image_tools')}>
        <button type="button" disabled={busy || loading || artworkLocked} onclick={collect} title={artworkLocked ? $t('images_locked') : $t('adds_provider_images_according_to_the')}>{$t('collect_images')}</button>
        <button type="button" class:locked-btn={artworkLocked} aria-pressed={artworkLocked} disabled={busy || loading} onclick={toggleLock} title={artworkLocked ? $t('unlock_images_to_allow_automatic_fill') : $t('lock_images_to_prevent_automatic_fill')}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            {#if artworkLocked}<rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" />{:else}<rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 7.5-2" />{/if}
          </svg>
          {artworkLocked ? $t('images_locked') : $t('lock_images')}
        </button>
        <button type="button" disabled={busy || loading} onclick={findSimilar} title={$t('compares_every_saved_image_without_deleting')}>{$t('find_duplicates')}</button>
        <span class="toolbar-hint">{artworkLocked ? $t('locked_no_further_image_download') : $t('languages_number_of_images_and_tolerance')}</span>
      </div>
      {#if collectionMessage}<p role="status" class="toolbar-status">{collectionMessage}</p>{/if}
      {#if similarPairs !== null}
        <section class="similar-panel" aria-labelledby="similar-title">
          <div class="section-heading">
            <h3 id="similar-title">{$t('likely_duplicates')}<span class="count">{similarPairs.length}</span></h3>
            <button type="button" class="ghost" onclick={() => similarPairs = null}>{$t('close')}</button>
          </div>
          {#if uncompared}<p>{$t('uncompared_images', { count: uncompared })}</p>{/if}
          {#if similarTruncated}<p>{$t('list_limited_to_128_pairs_run')}</p>{/if}
          {#if !similarPairs.length}<p>{$t('no_similar_images')}</p>{/if}
          <div class="similar-list">
            {#each similarPairs.slice(0, similarVisible) as pair (`${pair.first}:${pair.second}`)}
              <div class="similar-pair">
                {#each [pair.first, pair.second] as id (id)}
                  {@const entry = entries.find(item => item.id === id)}
                  {#if entry}
                    <div>
                      <img src={imageUrl(entry)} alt={`${labels[entry.kind]} de ${title}`} loading="lazy" />
                      <p>{provenance(entry)}</p>
                      <button type="button" class="ghost danger" disabled={busy} onclick={() => removeSimilar(id)}>{$t('remove')}</button>
                    </div>
                  {/if}
                {/each}
              </div>
            {/each}
          </div>
          {#if similarVisible < similarPairs.length}
            <button type="button" onclick={() => similarVisible += 20}>{$t('show_more')}</button>
          {/if}
        </section>
      {/if}
    {/if}

    {#if adding}
      <form class="upload-panel" onsubmit={event => event.preventDefault()}>
        <fieldset disabled={busy}>
          <legend>{replacing ? $t('replace_an_image') : $t('add_an_image')}</legend>
          <label for="artwork-type">{$t('image_type')}</label>
          <select id="artwork-type" bind:value={kind} disabled={!!replacing}>
            {#each kinds as type(type)}<option value={type}>{labels[type]}</option>{/each}
          </select>
          <label for="artwork-file">{$t('png_jpeg_or_webp_2_mib')}</label>
          {#if workId}<label>{$t('image_language_optional_e_g_fr')}<input bind:value={uploadLanguage} maxlength="35" placeholder={$t('stats_res_unknown')} /></label>{/if}
          <input id="artwork-file" bind:this={fileInput} type="file" accept="image/png,image/jpeg,image/webp" onchange={upload} />
          <p>{$t('images_are_kept_in_the_application')}</p>
          <button type="button" onclick={() => adding = false}>{$t('sched_cancel')}</button>
        </fieldset>
      </form>
    {/if}

    {#each groups as group (group.id)}
      {@const items = entries.filter(e => group.kinds.includes(e.kind)).sort((a, b) => group.kinds.indexOf(a.kind) - group.kinds.indexOf(b.kind))}
      {#if group.id !== 'other' || items.length}
        <section class="image-section" data-group={group.id} aria-labelledby={`images-${group.id}`}>
          <div class="section-heading">
            <h3 id={`images-${group.id}`}>{group.title}{#if items.length}<span class="count">{items.length}</span>{/if}</h3>
            <div class="heading-actions">
              {#if workId && $session?.user.role === 'administrator'}
                <button class="round" type="button" disabled={busy} title={$t('search_online', { value: group.title.toLowerCase() })} aria-label={$t('search_online', { value: group.title.toLowerCase() })} onclick={() => openSearch(group.kinds[0])}>
                  <Icon name="search" size={18} />
                </button>
              {/if}
              <button class="round" type="button" disabled={busy} aria-label={$t('add', { value: group.title.toLowerCase() })} title={$t('add_a_file')} onclick={() => add(group.kinds[0])}>+</button>
            </div>
          </div>
          <div class="image-grid">
            {#each items as entry (entry.id)}
              <article class="image-card" data-kind={entry.kind}>
                <button class="image-preview" type="button" aria-label={$t('enlarge', { value: labels[entry.kind] })} onclick={() => { selected = entry; preview.showModal(); }}>
                  <img src={imageUrl(entry)} alt={`${labels[entry.kind]} de ${title}`} loading="lazy" decoding="async" onload={event => dimensions(event, entry)} />
                  {#if entry.pinned}<span class="badge pin-badge">{$t('pinned')}</span>{/if}
                  {#if group.id === 'other'}<span class="badge kind-badge">{labels[entry.kind]}</span>{/if}
                </button>
                <p class="meta" title={provenance(entry)}>
                  {#if workId && entry.origin === 'provider'}{artworkCategories(entry.language, originalLanguage).join(' · ')}{#if entry.provider} · {entry.provider}{/if}{:else}{provenance(entry)}{/if}{#if entry.width && entry.height} · <span class="res">{entry.width}×{entry.height}</span>{/if}
                </p>
                <div class="card-actions">
                  {#if workId}
                    <button type="button" class="pin-btn" class:active={entry.pinned} disabled={busy} aria-label={entry.pinned ? $t('unpin_this_image') : $t('pin_this_image')} title={entry.pinned ? $t('unpin_this_image') : $t('pin_for_all_users')} onclick={() => togglePin(entry)}>
                      <svg width="15" height="15" viewBox="0 0 24 24" fill={entry.pinned ? 'currentColor' : 'none'} stroke="currentColor" stroke-width="1.9" aria-hidden="true"><path d="M12 2a6 6 0 0 0-6 6c0 2.6 1.4 3.9 2.4 5.6.5.8.9 1.6 1.2 2.4l2.4 6 2.4-6c.3-.8.7-1.6 1.2-2.4C16.6 11.9 18 10.6 18 8a6 6 0 0 0-6-6Z" /></svg>
                    </button>
                  {/if}
                  {#if entry.kind === 'backdrop'}
                    {@const position = items.findIndex(e => e.id === entry.id)}
                    <button type="button" class="reorder-btn" disabled={busy || position <= 0} aria-label={$t('move_the_backdrop_left')} title={$t('move_the_backdrop_left')} onclick={() => move(entry, -1)}>
                      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>
                    </button>
                    <button type="button" class="reorder-btn" disabled={busy || position < 0 || position >= items.length - 1} aria-label={$t('move_the_backdrop_right')} title={$t('move_the_backdrop_right')} onclick={() => move(entry, 1)}>
                      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
                    </button>
                  {/if}
                  <button type="button" class="delete-btn" disabled={busy} aria-label={$t('catalog_delete', { value: labels[entry.kind] })} title={entry.origin === 'local' ? $t('hide_this_image_in_the_catalogue') : $t('delete_this_custom_image')} onclick={() => remove(entry)}>
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" aria-hidden="true"><path d="M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7" /></svg>
                  </button>
                </div>
              </article>
            {:else}
              <p class="empty">{loading ? 'Chargement…' : $t('no_images')}</p>
            {/each}
          </div>
        </section>
      {/if}
    {/each}
    {#if busy}<p role="status">{$t('saving')}</p>{/if}
  </div>
</dialog>

<!-- Visual Search Modal (Jellyfin-like candidate selector) -->
<dialog bind:this={searchDialog} class="search-modal" aria-labelledby="search-modal-title">
  <header class="search-header">
    <div class="search-title-bar">
      <button type="button" class="back-btn" aria-label={$t('back')} onclick={() => searchDialog.close()}>←</button>
      <h3 id="search-modal-title">{$t('search_for_images')}</h3>
    </div>
    <div class="search-controls">
      <label><span>{$t('source')}</span>
        <select bind:value={searchProvider} onchange={fetchCandidates}>
          <option value="all">{$t('all')}</option>
          {#each providersList as p (p.id)}
            <option value={p.id}>{p.name}</option>
          {/each}
        </select>
      </label>
      <label><span>{$t('request_type')}</span>
        <select bind:value={searchType} onchange={fetchCandidates}>
          <option value="all">{$t('all_types')}</option>
          <option value="poster">{$t('poster_main')}</option>
          <option value="backdrop">{$t('backdrop')}</option>
          <option value="logo">{$t('logo')}</option>
          <option value="banner">{$t('banner')}</option>
          <option value="thumb">{$t('thumbnail')}</option>
        </select>
      </label>
      <label class="all-lang-toggle">
        <input type="checkbox" bind:checked={searchAllLanguages} onchange={fetchCandidates} />
        {$t('all_languages')}
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
        <p>{$t('searching_providers_for_images')}</p>
      </div>
    {:else if candidates.length === 0}
      <p class="search-empty">{$t('no_images_found_for_these_criteria')}</p>
    {:else}
      <div class="candidates-grid">
        {#each candidates as candidate(candidate.url)}
          <article class="candidate-card">
            <div class="candidate-preview" class:is-backdrop={candidate.imageType === 'backdrop'}>
              <img src={providerImagePreview(candidate.provider, workId ?? '', candidate.url)} alt={$t('preview', { imageType: candidate.imageType })} loading="lazy" />
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
                  <span class="applying-tag">{$t('downloading')}</span>
                {:else if appliedUrls.has(candidate.url)}
                  <span class="applied-tag">{$t('catalog_saved')}</span>
                {:else}
                  <button
                    type="button"
                    class="download-btn"
                    title={$t('download_and_apply_this_image')}
                    aria-label={$t('download_and_apply')}
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

<dialog class="zoom" bind:this={preview} aria-label={$t('image_preview')}>
  <button type="button" onclick={() => preview.close()}>{$t('close')}</button>
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
  .manager-content { padding: 18px 28px 28px; }
  section { margin-bottom: 0; }
  .section-heading { display: flex; align-items: center; justify-content: space-between; gap: 14px; margin-bottom: 12px; }
  .count { margin-left: 8px; padding: 1px 8px; border-radius: var(--r-pill); background: var(--pn); color: var(--mu); font-size: 12px; font-weight: 700; font-variant-numeric: tabular-nums; vertical-align: 2px; }
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

  /* Toolbar & duplicates */
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 10px; margin-bottom: 18px; }
  .toolbar button { display: inline-flex; align-items: center; gap: 6px; min-height: 40px; padding: 0 14px; font-size: 13px; border-radius: var(--r-sm); border: 1px solid var(--line); background: var(--pn); color: var(--tx); cursor: pointer; }
  .toolbar button:hover:not(:disabled) { background: var(--pn-2); border-color: var(--line-2); }
  .toolbar button:disabled { opacity: 0.45; cursor: not-allowed; }
  .toolbar button.locked-btn { background: var(--ac-soft); border-color: var(--ac); color: var(--ac); }
  .toolbar-hint { flex: 1 1 260px; color: var(--mu); font-size: 12px; line-height: 1.4; }
  .toolbar-status { margin: -8px 0 16px; }
  .ghost { background: transparent; }
  .ghost:hover:not(:disabled) { background: var(--pn); }
  .danger { color: var(--err); }
  .similar-panel { padding: 16px; margin-bottom: 20px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .similar-panel p { margin: 0 0 8px; }
  .similar-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 320px), 1fr)); gap: 12px; margin-bottom: 8px; }
  .similar-pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; padding: 8px; background: var(--bg-3); border-radius: var(--r-md); }
  .similar-pair img { display: block; width: 100%; height: 130px; object-fit: contain; background: var(--pn); border-radius: var(--r-sm); }
  .similar-pair div { min-width: 0; text-align: center; }
  .similar-pair p { font-size: 12px; margin: 4px 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .similar-pair button { min-height: 36px; padding: 0 10px; font-size: 13px; }

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

  /* Sections: one per role, compact grids sized to the image shape */
  .image-section { padding: 16px 0 20px; border-top: 1px solid var(--line); }
  .image-section:first-of-type { border-top: 0; padding-top: 0; }
  .image-grid { --min: 150px; display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, var(--min)), 1fr)); gap: 14px; }
  [data-group='logo'] .image-grid { --min: 220px; }
  [data-group='backdrop'] .image-grid { --min: 260px; }
  [data-group='other'] .image-grid { --min: 200px; }
  .image-card { min-width: 0; display: flex; flex-direction: column; }
  .image-preview { position: relative; display: block; width: 100%; aspect-ratio: 16/9; padding: 0; border: 0; border-radius: var(--r-md); overflow: hidden; background: var(--pn); }
  .image-preview:hover:not(:disabled) { background: var(--pn-2); }
  .image-card[data-kind='poster'] .image-preview { aspect-ratio: 2/3; }
  .image-card[data-kind='logo'] .image-preview { aspect-ratio: 5/2; padding: 12px; background: repeating-conic-gradient(var(--pn) 0% 25%, var(--pn-2) 0% 50%) 50% / 16px 16px; }
  .image-card[data-kind='disc'] .image-preview { aspect-ratio: 1; }
  .image-card[data-kind='banner'] .image-preview { aspect-ratio: 5/1; }
  .image-preview img { display: block; width: 100%; height: 100%; object-fit: contain; }
  .badge { position: absolute; top: 6px; left: 6px; padding: 2px 8px; border-radius: var(--r-pill); background: rgb(0 0 0 / 0.6); color: #fff; font-size: 11px; font-weight: 700; backdrop-filter: blur(6px); -webkit-backdrop-filter: blur(6px); }
  .kind-badge { left: auto; right: 6px; }
  .meta { margin: 6px 2px 0; font-size: 12px; line-height: 1.35; text-align: center; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .res { font-family: var(--font-mono); font-variant-numeric: tabular-nums; }
  .card-actions { display: flex; justify-content: center; gap: 6px; margin-top: 8px; }
  .card-actions .delete-btn, .card-actions .pin-btn, .card-actions .reorder-btn { display: inline-flex; align-items: center; justify-content: center; width: 34px; height: 34px; padding: 0; border-radius: 50%; border: 1px solid var(--line); background: var(--pn); color: var(--mu); cursor: pointer; transition: background-color var(--t-fast) var(--ease), border-color var(--t-fast) var(--ease), color var(--t-fast) var(--ease); }
  .card-actions .delete-btn:hover:not(:disabled) { background: var(--err-soft); border-color: var(--err); color: var(--err); }
  .card-actions .pin-btn:hover:not(:disabled), .card-actions .pin-btn.active { background: var(--ac-soft); border-color: var(--ac); color: var(--ac); }
  .card-actions .reorder-btn:hover:not(:disabled) { background: var(--pn-2); color: var(--tx); }
  .card-actions .delete-btn:disabled, .card-actions .pin-btn:disabled, .card-actions .reorder-btn:disabled { opacity: 0.45; cursor: not-allowed; }
  .empty { margin: 0; font-size: 13px; }
  @media (hover: hover) and (pointer: fine) {
    .card-actions { opacity: 0.55; transition: opacity var(--t-fast) var(--ease); }
    .image-card:hover .card-actions, .image-card:focus-within .card-actions { opacity: 1; }
  }

  /* Zoom preview */
  .zoom { max-width: calc(100vw - 48px); max-height: calc(100dvh - 48px); padding: 16px; }
  .zoom img { display: block; max-width: 100%; max-height: 80dvh; object-fit: contain; margin-top: 12px; border-radius: var(--r-md); }

  /* Image search dialog */
  .search-header { display: flex; flex-direction: column; align-items: stretch; gap: 14px; padding: 20px 28px 16px; background: var(--bg-3); border-bottom: 1px solid var(--line); position: sticky; top: 0; z-index: 10; }
  .search-title-bar { display: flex; align-items: center; gap: 12px; }
  .search-title-bar h3 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  .search-controls { display: flex; align-items: center; gap: 12px 20px; flex-wrap: wrap; }
  .search-controls label { display: flex; flex-wrap: nowrap; align-items: center; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); white-space: nowrap; }
  .search-controls label > span { flex: none; }
  .search-controls select { width: auto; min-width: 9rem; }
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
    .image-grid { --min: 130px; gap: 10px; }
    [data-group='backdrop'] .image-grid, [data-group='logo'] .image-grid, [data-group='other'] .image-grid { --min: 100%; }
    .candidates-grid { grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 14px; }
    .count-badge { margin-left: 0; }
  }
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { locale, t } from '../i18n';
  import Icon from '../ui/Icon.svelte';
  import type { Work } from './api';
  import {
    buildMetadataPutPayload,
    createMetadataDraft,
    CUSTOM_AGE_OPTIONS,
    defaultSortTitle,
    LOCKABLE_METADATA_FIELDS,
    METADATA_LANGUAGE_OPTIONS,
    ORIGINAL_LANGUAGE_OPTIONS,
    PARENTAL_RATING_OPTIONS,
    switchMetadataDraftLocale,
    syncActiveLocaleTranslation,
    type AdminWorkMetadataResponse,
    type LockableMetadataField,
    type MetadataPersonType,
    type WorkMetadataDraft
  } from './work-metadata';

  let {
    work,
    onclose,
    onsaved
  }: {
    work: Work;
    onclose: () => void;
    onsaved?: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let draft = $state<WorkMetadataDraft>(createMetadataDraft(work, null, $locale));
  let loadingRemote = $state(true);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');

  // Inputs for adding rows to interactive lists
  let showAddGenre = $state(false);
  let newGenre = $state('');

  let showAddPerson = $state(false);
  let newPersonName = $state('');
  let newPersonRole = $state('');
  let newPersonType = $state<MetadataPersonType>('actor');

  let showAddStudio = $state(false);
  let newStudio = $state('');

  let showAddCountry = $state(false);
  let newCountry = $state('');

  let showAddTag = $state(false);
  let newTag = $state('');

  let showAddExtraId = $state(false);
  let newExtraIdKey = $state('');
  let newExtraIdValue = $state('');

  // Multi-node copy grouping state
  let copyId = $state('');
  let candidates = $state<{
    id: string;
    leftWorkId: string;
    leftTitle: string;
    rightWorkId: string;
    rightTitle: string;
    score: number;
    reason: string;
    status: string;
  }[]>([]);
  let merges = $state<{ id: string; groupId: string; origin: string }[]>([]);

  const isFrench = $derived(($locale ?? 'fr').toLowerCase().startsWith('fr'));

  const lockableFieldLabels = $derived<Record<LockableMetadataField, string>>({
    title: isFrench ? 'Nom' : 'Name',
    plot: isFrench ? 'Synopsis' : 'Overview',
    genres: isFrench ? 'Genres' : 'Genres',
    mpaa: isFrench ? 'Classification parentale' : 'Parental rating',
    people: isFrench ? 'Personnes' : 'People',
    countries: isFrench ? 'Pays de production' : 'Production countries',
    studios: isFrench ? 'Studios' : 'Studios',
    tags: isFrench ? 'Étiquettes' : 'Tags'
  });

  const tmdbLabel = $derived(
    work.kind === 'film'
      ? isFrench
        ? 'ID TheMovieDb Film'
        : 'TheMovieDb Movie ID'
      : work.kind === 'series'
        ? isFrench
          ? 'ID TheMovieDb Série'
          : 'TheMovieDb Series ID'
        : isFrench
          ? 'ID TheMovieDb Épisode'
          : 'TheMovieDb Episode ID'
  );

  const tvdbLabel = $derived(
    work.kind === 'series'
      ? isFrench
        ? 'ID TheTVDB Série'
        : 'TheTVDB Series ID'
      : work.kind === 'episode'
        ? isFrench
          ? 'ID TheTVDB Épisode'
          : 'TheTVDB Episode ID'
        : 'ID TheTVDB'
  );

  onMount(() => {
    void loadFullMetadata();
    if (work.kind === 'film' || work.kind === 'series') {
      void loadCandidates();
    }
  });

  async function loadFullMetadata() {
    loadingRemote = true;
    try {
      const response = await fetch(`/api/v1/admin/works/${encodeURIComponent(work.id)}/metadata`);
      if (response.ok) {
        const remote = (await response.json()) as AdminWorkMetadataResponse;
        draft = createMetadataDraft(work, remote, $locale);
      }
    } catch {
      // Keep local fallback draft from work prop
    } finally {
      loadingRemote = false;
    }
  }

  async function loadCandidates() {
    try {
      const response = await fetch('/api/v1/admin/media-groups/candidates?status=pending');
      if (!response.ok) return;
      const data = await response.json();
      candidates = (data.candidates ?? []).filter(
        (c: { leftWorkId: string; rightWorkId: string }) =>
          c.leftWorkId === work.id || c.rightWorkId === work.id
      );
      merges = (data.merges ?? []).filter(
        (m: { groupId: string }) => m.groupId === work.id || m.groupId === work.globalMediaId
      );
    } catch {}
  }

  async function mutate(path: string, method: string, body?: unknown) {
    busy = true;
    error = '';
    message = '';
    try {
      const response = await fetch(path, {
        method,
        headers: {
          'content-type': 'application/json',
          'x-csrf-token': $session?.csrfToken ?? ''
        },
        body: body === undefined ? undefined : JSON.stringify(body)
      });
      if (!response.ok) {
        throw new Error($t('catalog_saving_failed_http', { status: response.status }));
      }
      message = $t('saved_successfully');
      return response.status === 204 ? true : await response.json();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : $t('saving_failed');
      return false;
    } finally {
      busy = false;
    }
  }

  function handleLocaleChange(nextCode: string) {
    draft = { ...switchMetadataDraftLocale(draft, nextCode) };
  }

  function hasTranslationForLocale(code: string): boolean {
    const norm = code.toLowerCase();
    if (norm === draft.activeLocale) {
      return Boolean(draft.title.trim() || draft.plot.trim() || draft.tagline.trim());
    }
    const tr = draft.translations[norm];
    return Boolean(tr && (tr.title.trim() || tr.plot.trim() || tr.tagline.trim()));
  }

  function isFieldUnlocked(field: LockableMetadataField): boolean {
    return !draft.lockedFields.includes(field);
  }

  function toggleFieldUnlocked(field: LockableMetadataField, unlocked: boolean) {
    if (unlocked) {
      draft.lockedFields = draft.lockedFields.filter(f => f !== field);
    } else if (!draft.lockedFields.includes(field)) {
      draft.lockedFields = [...draft.lockedFields, field];
    }
  }

  function addGenre() {
    const trimmed = newGenre.trim();
    if (!trimmed) return;
    if (!draft.genres.includes(trimmed)) {
      draft.genres = [...draft.genres, trimmed];
    }
    newGenre = '';
    showAddGenre = false;
  }

  function removeGenre(index: number) {
    draft.genres = draft.genres.filter((_, i) => i !== index);
  }

  function addPerson() {
    const name = newPersonName.trim();
    if (!name) return;
    draft.people = [
      ...draft.people,
      {
        name,
        role:
          newPersonRole.trim() ||
          (newPersonType === 'director'
            ? isFrench
              ? 'Réalisateur'
              : 'Director'
            : newPersonType === 'writer'
              ? isFrench
                ? 'Scénariste'
                : 'Writer'
              : ''),
        type: newPersonType
      }
    ];
    newPersonName = '';
    newPersonRole = '';
    newPersonType = 'actor';
    showAddPerson = false;
  }

  function removePerson(index: number) {
    draft.people = draft.people.filter((_, i) => i !== index);
  }

  function addStudio() {
    const trimmed = newStudio.trim();
    if (!trimmed) return;
    if (!draft.studios.includes(trimmed)) {
      draft.studios = [...draft.studios, trimmed];
    }
    newStudio = '';
    showAddStudio = false;
  }

  function removeStudio(index: number) {
    draft.studios = draft.studios.filter((_, i) => i !== index);
  }

  function addCountry() {
    const trimmed = newCountry.trim();
    if (!trimmed) return;
    if (!draft.countries.includes(trimmed)) {
      draft.countries = [...draft.countries, trimmed];
    }
    newCountry = '';
    showAddCountry = false;
  }

  function removeCountry(index: number) {
    draft.countries = draft.countries.filter((_, i) => i !== index);
  }

  function addTag() {
    const trimmed = newTag.trim();
    if (!trimmed) return;
    if (!draft.tags.includes(trimmed)) {
      draft.tags = [...draft.tags, trimmed];
    }
    newTag = '';
    showAddTag = false;
  }

  function removeTag(index: number) {
    draft.tags = draft.tags.filter((_, i) => i !== index);
  }

  function addExtraExternalId() {
    const key = newExtraIdKey.trim().toLowerCase();
    const value = newExtraIdValue.trim();
    if (!key || !value) return;
    draft.extraExternalIds = [
      ...draft.extraExternalIds.filter(item => item.key !== key),
      { key, value }
    ];
    newExtraIdKey = '';
    newExtraIdValue = '';
    showAddExtraId = false;
  }

  function removeExtraExternalId(index: number) {
    draft.extraExternalIds = draft.extraExternalIds.filter((_, i) => i !== index);
  }

  function personTypeLabel(type: MetadataPersonType): string {
    if (type === 'director') return isFrench ? 'Réalisateur' : 'Director';
    if (type === 'writer') return isFrench ? 'Scénariste' : 'Writer';
    return isFrench ? 'Acteur' : 'Actor';
  }

  async function handleSave(event: SubmitEvent) {
    event.preventDefault();
    syncActiveLocaleTranslation(draft);
    if (!draft.title.trim()) {
      error = isFrench ? 'Le titre est obligatoire.' : 'Title is required.';
      return;
    }
    const payload = buildMetadataPutPayload(draft);
    const ok = await mutate(`/api/v1/admin/works/${encodeURIComponent(work.id)}/metadata`, 'PUT', payload);
    if (ok) {
      if (onsaved) {
        onsaved();
      } else {
        window.location.reload();
      }
    }
  }

  async function handleReset() {
    const ok = await mutate(`/api/v1/admin/works/${encodeURIComponent(work.id)}/metadata`, 'DELETE');
    if (ok) {
      if (onsaved) {
        onsaved();
      } else {
        window.location.reload();
      }
    }
  }
</script>

<div class="modal-overlay" role="dialog" aria-modal="true" aria-labelledby="metadata-edit-title">
  <button
    type="button"
    class="modal-backdrop-click"
    aria-label={$t('close_the_window')}
    tabindex="-1"
    onclick={onclose}
  ></button>

  <div class="modal-card metadata-editor-card">
    <div class="modal-header">
      <div class="modal-title-group">
        <Icon name="edit" size={20} />
        <div>
          <h3 id="metadata-edit-title">{$t('edit_metadata')}</h3>
          <p class="modal-subtitle">{work.title}</p>
        </div>
      </div>

      <button
        type="button"
        class="close-modal-btn"
        aria-label={$t('close')}
        onclick={onclose}
      >
        <Icon name="close" size={18} />
      </button>
    </div>

    <form onsubmit={handleSave} class="metadata-form">
      <div class="modal-body">
        {#if error}
          <div class="alert-banner error" role="alert">{error}</div>
        {/if}
        {#if message}
          <div class="alert-banner success" role="status">{message}</div>
        {/if}

        <!-- Multi-language switcher bar (Aether multi-language adaptation) -->
        <section class="lang-switcher-box" aria-label={isFrench ? 'Langue des métadonnées' : 'Metadata language'}>
          <div class="lang-switcher-head">
            <div class="lang-switcher-title">
              <Icon name="globe" size={16} />
              <strong>{isFrench ? 'Langue d’édition des textes' : 'Text editing language'}</strong>
              {#if loadingRemote}
                <span class="loading-pill">{isFrench ? 'Synchronisation…' : 'Syncing…'}</span>
              {/if}
            </div>
            <span class="lang-switcher-hint">
              {isFrench
                ? 'Le titre, le titre de tri, le slogan et le synopsis sont enregistrés par langue.'
                : 'Title, sort title, tagline and synopsis are saved per language.'}
            </span>
          </div>

          <div class="lang-pills" role="tablist">
            {#each METADATA_LANGUAGE_OPTIONS as opt (opt.code)}
              {@const active = draft.activeLocale === opt.code}
              {@const filled = hasTranslationForLocale(opt.code)}
              {@const isDefault = draft.defaultLocale === opt.code}
              <button
                type="button"
                role="tab"
                aria-selected={active}
                class="lang-pill"
                class:active
                class:filled
                onclick={() => handleLocaleChange(opt.code)}
              >
                <span class="lang-code">{opt.code.toUpperCase()}</span>
                {#if isDefault}
                  <span class="lang-badge default">{isFrench ? 'Défaut' : 'Default'}</span>
                {:else if filled}
                  <span class="lang-dot" aria-hidden="true"></span>
                {/if}
              </button>
            {/each}
          </div>
        </section>

        <fieldset disabled={busy} class="editor-fieldset">
          <legend class="sr-only">{$t('metadata_form')}</legend>

          <!-- Screenshot 1 & 2: General & Localized Fields -->
          <div class="fields-stack">
            <label class="field-block">
              <span class="field-label">
                {$t('title')}
                <span class="locale-tag">{draft.activeLocale.toUpperCase()}</span>
              </span>
              <input
                required
                maxlength="512"
                bind:value={draft.title}
                oninput={() => syncActiveLocaleTranslation(draft)}
              />
            </label>

            <div class="fields-row two-cols">
              <label class="field-block">
                <span class="field-label">{$t('original_title')}</span>
                <input maxlength="512" bind:value={draft.originalTitle} />
              </label>

              <label class="field-block">
                <span class="field-label">{isFrench ? 'Langue d’origine' : 'Original language'}</span>
                <select bind:value={draft.originalLanguage}>
                  {#each ORIGINAL_LANGUAGE_OPTIONS as lang (lang.code)}
                    <option value={lang.code}>{isFrench ? lang.labelFr : lang.labelEn}</option>
                  {/each}
                  {#if draft.originalLanguage && !ORIGINAL_LANGUAGE_OPTIONS.some(l => l.code === draft.originalLanguage)}
                    <option value={draft.originalLanguage}>{draft.originalLanguage}</option>
                  {/if}
                </select>
              </label>
            </div>

            <div class="field-block">
              <div class="field-label-row">
                <span class="field-label">
                  {isFrench ? 'Titre de tri' : 'Sort title'}
                  <span class="locale-tag">{draft.activeLocale.toUpperCase()}</span>
                </span>
                <button
                  type="button"
                  class="inline-link-btn"
                  onclick={() => {
                    draft.sortTitle = defaultSortTitle(draft.title);
                    syncActiveLocaleTranslation(draft);
                  }}
                >
                  {isFrench ? 'Générer depuis le titre' : 'Generate from title'}
                </button>
              </div>
              <input
                maxlength="512"
                bind:value={draft.sortTitle}
                oninput={() => syncActiveLocaleTranslation(draft)}
              />
            </div>

            <div class="fields-row three-cols">
              <label class="field-block">
                <span class="field-label">{isFrench ? 'Date d’ajout' : 'Date added'}</span>
                <input type="date" bind:value={draft.addedAtDate} />
              </label>

              <label class="field-block">
                <span class="field-label">{isFrench ? 'Date de sortie' : 'Release date'}</span>
                <input type="date" bind:value={draft.premieredDate} />
              </label>

              <label class="field-block">
                <span class="field-label">{$t('stats_w_year')}</span>
                <input type="number" min="1800" max="2200" bind:value={draft.year} />
              </label>
            </div>

            <div class="fields-row two-cols">
              <label class="field-block">
                <span class="field-label">{isFrench ? 'Note de la communauté' : 'Community rating'}</span>
                <input
                  type="number"
                  step="0.001"
                  min="0"
                  max="10"
                  placeholder="7.842"
                  bind:value={draft.rating}
                />
              </label>

              <label class="field-block">
                <span class="field-label">{isFrench ? 'Note des critiques' : 'Critic rating'}</span>
                <input
                  type="number"
                  step="0.1"
                  min="0"
                  max="100"
                  placeholder="85"
                  bind:value={draft.criticRating}
                />
              </label>
            </div>

            <div class="fields-row two-cols">
              <label class="field-block">
                <span class="field-label">{isFrench ? 'Classification parentale' : 'Parental rating'}</span>
                <select bind:value={draft.mpaa}>
                  {#each PARENTAL_RATING_OPTIONS as opt (opt)}
                    <option value={opt}>{opt || (isFrench ? 'Non classé' : 'Unrated')}</option>
                  {/each}
                  {#if draft.mpaa && !PARENTAL_RATING_OPTIONS.includes(draft.mpaa)}
                    <option value={draft.mpaa}>{draft.mpaa}</option>
                  {/if}
                </select>
              </label>

              <label class="field-block">
                <span class="field-label">{isFrench ? 'Note personnalisée (âge minimum)' : 'Custom rating (minimum age)'}</span>
                <select bind:value={draft.minimumAge}>
                  {#each CUSTOM_AGE_OPTIONS as opt (opt.value)}
                    <option value={opt.value}>{isFrench ? opt.labelFr : opt.labelEn}</option>
                  {/each}
                </select>
              </label>
            </div>

            <label class="field-block">
              <span class="field-label">
                {isFrench ? 'Slogan' : 'Tagline'}
                <span class="locale-tag">{draft.activeLocale.toUpperCase()}</span>
              </span>
              <input
                maxlength="1024"
                bind:value={draft.tagline}
                oninput={() => syncActiveLocaleTranslation(draft)}
              />
            </label>

            <label class="field-block">
              <span class="field-label">
                {isFrench ? 'Synopsis' : 'Overview'}
                <span class="locale-tag">{draft.activeLocale.toUpperCase()}</span>
              </span>
              <textarea
                maxlength="20000"
                rows="5"
                bind:value={draft.plot}
                oninput={() => syncActiveLocaleTranslation(draft)}
              ></textarea>
            </label>
          </div>

          <hr class="section-divider" />

          <!-- Screenshot 2 & 3: External Identifiers -->
          <section class="editor-section">
            <div class="section-head">
              <h4>{isFrench ? 'Identifiants externes' : 'External IDs'}</h4>
              <button
                type="button"
                class="add-circle-btn"
                title={isFrench ? 'Ajouter un identifiant externe' : 'Add external ID'}
                aria-label={isFrench ? 'Ajouter un identifiant externe' : 'Add external ID'}
                onclick={() => (showAddExtraId = !showAddExtraId)}
              >
                +
              </button>
            </div>

            <div class="fields-row two-cols">
              <label class="field-block">
                <span class="field-label">ID IMDb</span>
                <input maxlength="64" placeholder="tt35856133" bind:value={draft.imdbId} />
              </label>

              <label class="field-block">
                <span class="field-label">{tmdbLabel}</span>
                <input maxlength="64" placeholder="1165369" bind:value={draft.tmdbId} />
              </label>

              <label class="field-block">
                <span class="field-label">{tvdbLabel}</span>
                <input maxlength="64" placeholder="412345" bind:value={draft.tvdbId} />
              </label>

              {#if work.kind === 'film'}
                <label class="field-block">
                  <span class="field-label">{isFrench ? 'ID TheMovieDb Coffret' : 'TheMovieDb Collection ID'}</span>
                  <input maxlength="64" placeholder="1165374" bind:value={draft.collectionId} />
                </label>
              {/if}
            </div>

            {#if work.kind === 'film' && draft.collectionId.trim()}
              <label class="field-block top-gap">
                <span class="field-label">{isFrench ? 'Nom du coffret / saga' : 'Collection name'}</span>
                <input maxlength="256" bind:value={draft.collectionName} />
              </label>
            {/if}

            {#if draft.extraExternalIds.length > 0}
              <div class="interactive-list top-gap">
                {#each draft.extraExternalIds as item, idx (item.key + idx)}
                  <div class="list-row">
                    <div class="list-row-left">
                      <span class="list-row-icon"><Icon name="globe" size={16} /></span>
                      <div class="list-row-text">
                        <strong>{item.key.toUpperCase()}</strong>
                        <span>{item.value}</span>
                      </div>
                    </div>
                    <button
                      type="button"
                      class="trash-btn"
                      aria-label={isFrench ? `Supprimer ${item.key}` : `Remove ${item.key}`}
                      onclick={() => removeExtraExternalId(idx)}
                    >
                      <Icon name="trash" size={16} />
                    </button>
                  </div>
                {/each}
              </div>
            {/if}

            {#if showAddExtraId}
              <div class="inline-add-box top-gap">
                <input
                  placeholder={isFrench ? 'Clé (ex. anilist, trakt)' : 'Key (e.g. anilist, trakt)'}
                  bind:value={newExtraIdKey}
                />
                <input
                  placeholder={isFrench ? 'Valeur / ID' : 'Value / ID'}
                  bind:value={newExtraIdValue}
                />
                <button type="button" class="btn small primary-btn" onclick={addExtraExternalId}>
                  {isFrench ? 'Ajouter' : 'Add'}
                </button>
              </div>
            {/if}
          </section>

          <hr class="section-divider" />

          <!-- Screenshot 3: Genres -->
          <section class="editor-section">
            <div class="section-head">
              <h4>{isFrench ? 'Genres' : 'Genres'}</h4>
              <button
                type="button"
                class="add-circle-btn"
                title={isFrench ? 'Ajouter un genre' : 'Add genre'}
                aria-label={isFrench ? 'Ajouter un genre' : 'Add genre'}
                onclick={() => (showAddGenre = !showAddGenre)}
              >
                +
              </button>
            </div>

            {#if showAddGenre}
              <div class="inline-add-box">
                <input
                  placeholder={isFrench ? 'Nouveau genre (ex. Drama, History, War)' : 'New genre (e.g. Drama, Sci-Fi)'}
                  bind:value={newGenre}
                  onkeydown={e => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      addGenre();
                    }
                  }}
                />
                <button type="button" class="btn small primary-btn" onclick={addGenre}>
                  {isFrench ? 'Ajouter' : 'Add'}
                </button>
              </div>
            {/if}

            <div class="interactive-list">
              {#each draft.genres as genre, idx (`${genre}-${idx}`)}
                <div class="list-row">
                  <div class="list-row-left">
                    <span class="list-row-icon"><Icon name="tv" size={16} /></span>
                    <span class="list-row-title">{genre}</span>
                  </div>
                  <button
                    type="button"
                    class="trash-btn"
                    aria-label={isFrench ? `Supprimer ${genre}` : `Remove ${genre}`}
                    onclick={() => removeGenre(idx)}
                  >
                    <Icon name="trash" size={16} />
                  </button>
                </div>
              {:else}
                <p class="empty-list-hint">{isFrench ? 'Aucun genre défini.' : 'No genres set.'}</p>
              {/each}
            </div>
          </section>

          <hr class="section-divider" />

          <!-- Screenshot 3: Personnes -->
          <section class="editor-section">
            <div class="section-head">
              <h4>{isFrench ? 'Personnes' : 'People'}</h4>
              <button
                type="button"
                class="add-circle-btn"
                title={isFrench ? 'Ajouter une personne' : 'Add person'}
                aria-label={isFrench ? 'Ajouter une personne' : 'Add person'}
                onclick={() => (showAddPerson = !showAddPerson)}
              >
                +
              </button>
            </div>

            {#if showAddPerson}
              <div class="inline-add-box person-add-box">
                <input
                  placeholder={isFrench ? 'Nom (ex. Simon Abkarian)' : 'Name'}
                  bind:value={newPersonName}
                />
                <input
                  placeholder={isFrench ? 'Rôle / Personnage (ex. Charles de Gaulle)' : 'Role / Character'}
                  bind:value={newPersonRole}
                />
                <select bind:value={newPersonType}>
                  <option value="actor">{isFrench ? 'Acteur' : 'Actor'}</option>
                  <option value="director">{isFrench ? 'Réalisateur' : 'Director'}</option>
                  <option value="writer">{isFrench ? 'Scénariste' : 'Writer'}</option>
                </select>
                <button type="button" class="btn small primary-btn" onclick={addPerson}>
                  {isFrench ? 'Ajouter' : 'Add'}
                </button>
              </div>
            {/if}

            <div class="interactive-list people-list">
              {#each draft.people as person, idx (`${person.name}-${person.type}-${idx}`)}
                <div class="list-row">
                  <div class="list-row-left">
                    <span class="list-row-icon person-avatar">
                      {#if person.imageUrl}
                        <img src={person.imageUrl} alt="" loading="lazy" />
                      {:else}
                        <Icon name="profile" size={16} />
                      {/if}
                    </span>
                    <div class="list-row-text">
                      <strong>{person.name}</strong>
                      <span>
                        {person.role || personTypeLabel(person.type)}
                        {#if person.type !== 'actor'}
                          · <em>{personTypeLabel(person.type)}</em>
                        {/if}
                      </span>
                    </div>
                  </div>
                  <button
                    type="button"
                    class="trash-btn"
                    aria-label={isFrench ? `Supprimer ${person.name}` : `Remove ${person.name}`}
                    onclick={() => removePerson(idx)}
                  >
                    <Icon name="trash" size={16} />
                  </button>
                </div>
              {:else}
                <p class="empty-list-hint">{isFrench ? 'Aucune personne enregistrée.' : 'No people listed.'}</p>
              {/each}
            </div>
          </section>

          <hr class="section-divider" />

          <!-- Screenshot 4: Studios, Pays de production & Étiquettes -->
          <div class="fields-row three-cols">
            <!-- Pays de production -->
            <section class="editor-section">
              <div class="section-head">
                <h4>{isFrench ? 'Pays de production' : 'Countries'}</h4>
                <button
                  type="button"
                  class="add-circle-btn"
                  aria-label={isFrench ? 'Ajouter un pays' : 'Add country'}
                  onclick={() => (showAddCountry = !showAddCountry)}
                >
                  +
                </button>
              </div>

              {#if showAddCountry}
                <div class="inline-add-box compact">
                  <input
                    placeholder="France, US…"
                    bind:value={newCountry}
                    onkeydown={e => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        addCountry();
                      }
                    }}
                  />
                  <button type="button" class="btn small primary-btn" onclick={addCountry}>+</button>
                </div>
              {/if}

              <div class="interactive-list compact">
                {#each draft.countries as country, idx (`${country}-${idx}`)}
                  <div class="list-row compact">
                    <span class="list-row-title">{country}</span>
                    <button
                      type="button"
                      class="trash-btn"
                      aria-label={isFrench ? `Supprimer ${country}` : `Remove ${country}`}
                      onclick={() => removeCountry(idx)}
                    >
                      <Icon name="trash" size={15} />
                    </button>
                  </div>
                {:else}
                  <p class="empty-list-hint">{isFrench ? 'Aucun pays' : 'No countries'}</p>
                {/each}
              </div>
            </section>

            <!-- Studios -->
            <section class="editor-section">
              <div class="section-head">
                <h4>{isFrench ? 'Studios' : 'Studios'}</h4>
                <button
                  type="button"
                  class="add-circle-btn"
                  aria-label={isFrench ? 'Ajouter un studio' : 'Add studio'}
                  onclick={() => (showAddStudio = !showAddStudio)}
                >
                  +
                </button>
              </div>

              {#if showAddStudio}
                <div class="inline-add-box compact">
                  <input
                    placeholder="Pathé…"
                    bind:value={newStudio}
                    onkeydown={e => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        addStudio();
                      }
                    }}
                  />
                  <button type="button" class="btn small primary-btn" onclick={addStudio}>+</button>
                </div>
              {/if}

              <div class="interactive-list compact">
                {#each draft.studios as studio, idx (`${studio}-${idx}`)}
                  <div class="list-row compact">
                    <span class="list-row-title">{studio}</span>
                    <button
                      type="button"
                      class="trash-btn"
                      aria-label={isFrench ? `Supprimer ${studio}` : `Remove ${studio}`}
                      onclick={() => removeStudio(idx)}
                    >
                      <Icon name="trash" size={15} />
                    </button>
                  </div>
                {:else}
                  <p class="empty-list-hint">{isFrench ? 'Aucun studio' : 'No studios'}</p>
                {/each}
              </div>
            </section>

            <!-- Étiquettes -->
            <section class="editor-section">
              <div class="section-head">
                <h4>{isFrench ? 'Étiquettes' : 'Tags'}</h4>
                <button
                  type="button"
                  class="add-circle-btn"
                  aria-label={isFrench ? 'Ajouter une étiquette' : 'Add tag'}
                  onclick={() => (showAddTag = !showAddTag)}
                >
                  +
                </button>
              </div>

              {#if showAddTag}
                <div class="inline-add-box compact">
                  <input
                    placeholder="4k, vf…"
                    bind:value={newTag}
                    onkeydown={e => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        addTag();
                      }
                    }}
                  />
                  <button type="button" class="btn small primary-btn" onclick={addTag}>+</button>
                </div>
              {/if}

              <div class="interactive-list compact">
                {#each draft.tags as tag, idx (`${tag}-${idx}`)}
                  <div class="list-row compact">
                    <span class="list-row-title">{tag}</span>
                    <button
                      type="button"
                      class="trash-btn"
                      aria-label={isFrench ? `Supprimer ${tag}` : `Remove ${tag}`}
                      onclick={() => removeTag(idx)}
                    >
                      <Icon name="trash" size={15} />
                    </button>
                  </div>
                {:else}
                  <p class="empty-list-hint">{isFrench ? 'Aucune étiquette' : 'No tags'}</p>
                {/each}
              </div>
            </section>
          </div>

          <hr class="section-divider" />

          <!-- Screenshot 4: Verrouiller cet élément & Activer les champs -->
          <section class="editor-section lock-section">
            <label class="checkbox-row master-lock">
              <input type="checkbox" bind:checked={draft.locked} />
              <span>
                {isFrench
                  ? 'Verrouiller cet élément pour éviter de futures modifications automatiques'
                  : 'Lock this item to prevent future automatic changes'}
              </span>
            </label>

            <div class="lock-fields-block">
              <h4>{isFrench ? 'Activer les champs' : 'Enabled fields'}</h4>
              <p class="lock-hint">
                {isFrench
                  ? 'Décocher un champ pour le verrouiller et empêcher ses données d’être modifiées.'
                  : 'Uncheck a field to lock it and prevent its data from being overwritten.'}
              </p>

              <div class="lock-checkboxes-grid">
                {#each LOCKABLE_METADATA_FIELDS as field (field)}
                  <label class="checkbox-row">
                    <input
                      type="checkbox"
                      checked={isFieldUnlocked(field)}
                      disabled={draft.locked}
                      onchange={e => toggleFieldUnlocked(field, e.currentTarget.checked)}
                    />
                    <span>{lockableFieldLabels[field]}</span>
                  </label>
                {/each}
              </div>
            </div>
          </section>

          <!-- Multi-node copy grouping (Films & Series) -->
          {#if work.kind === 'film' || work.kind === 'series'}
            <hr class="section-divider" />
            <section class="editor-section">
              <h4>{$t('group_multi_node_copies')}</h4>
              <div class="copy-group-row">
                <label class="field-block flex-grow">
                  <span class="field-label">{$t('exact_identifier_of_the_same_work')}</span>
                  <input bind:value={copyId} placeholder={$t('work_id')} />
                </label>
                <button
                  type="button"
                  class="btn secondary-btn self-end"
                  disabled={!copyId || copyId === work.id || busy}
                  onclick={async () => {
                    if (
                      await mutate('/api/v1/admin/media-groups', 'POST', {
                        workIds: [work.id, copyId],
                        label: work.title
                      })
                    ) {
                      window.location.reload();
                    }
                  }}
                >
                  {$t('link_the_two_works')}
                </button>
                <button
                  type="button"
                  class="btn danger-btn self-end"
                  disabled={busy}
                  onclick={async () => {
                    if (await mutate(`/api/v1/admin/media-groups/${work.id}`, 'DELETE')) {
                      window.location.reload();
                    }
                  }}
                >
                  {$t('unlink_this_copy')}
                </button>
              </div>

              {#if merges.length > 0}
                <div class="merges-list">
                  {#each merges as merge (merge.id)}
                    <button
                      type="button"
                      class="btn outline-btn"
                      onclick={async () => {
                        if (
                          await mutate(
                            `/api/v1/admin/media-groups/merges/${encodeURIComponent(merge.id)}/undo`,
                            'POST'
                          )
                        ) {
                          window.location.reload();
                        }
                      }}
                    >
                      {$t('undo_the_merge', { origin: merge.origin })}
                    </button>
                  {/each}
                </div>
              {/if}

              {#if candidates.length > 0}
                <div class="candidates-box">
                  <strong>{$t('likely_matches_found')}</strong>
                  {#each candidates as candidate (candidate.id)}
                    <div class="candidate-row">
                      <span>
                        <code>
                          {candidate.leftWorkId === work.id ? candidate.rightTitle : candidate.leftTitle}
                        </code>
                        ({candidate.score}% · {candidate.reason})
                      </span>
                      <div class="candidate-actions">
                        <button
                          type="button"
                          class="btn small primary-btn"
                          onclick={async () => {
                            if (
                              await mutate(
                                `/api/v1/admin/media-groups/candidates/${encodeURIComponent(candidate.id)}/confirm`,
                                'POST'
                              )
                            ) {
                              window.location.reload();
                            }
                          }}
                        >
                          {$t('confirm')}
                        </button>
                        <button
                          type="button"
                          class="btn small outline-btn"
                          onclick={async () => {
                            if (
                              await mutate(
                                `/api/v1/admin/media-groups/candidates/${encodeURIComponent(candidate.id)}/reject`,
                                'POST'
                              )
                            ) {
                              await loadCandidates();
                            }
                          }}
                        >
                          {$t('catalog_reject')}
                        </button>
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            </section>
          {/if}
        </fieldset>
      </div>

      <!-- Footer matching Jellyfin: Annuler | Réinitialiser | Sauvegarder -->
      <div class="modal-footer jellyfin-footer">
        <button type="button" class="btn secondary-btn" onclick={onclose}>
          {$t('sched_cancel')}
        </button>
        <button
          type="button"
          class="btn outline-btn"
          disabled={busy}
          onclick={handleReset}
        >
          {isFrench ? 'Réinitialiser' : 'Reset'}
        </button>
        <button type="submit" class="btn primary-btn" disabled={busy}>
          {busy
            ? isFrench
              ? 'Sauvegarde…'
              : 'Saving…'
            : isFrench
              ? 'Sauvegarder'
              : 'Save'}
        </button>
      </div>
    </form>
  </div>
</div>

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    z-index: 950;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
  }

  .modal-backdrop-click {
    position: fixed;
    inset: 0;
    background: var(--overlay, rgba(6, 8, 12, 0.82));
    backdrop-filter: blur(8px);
    border: none;
    cursor: default;
  }

  .metadata-editor-card {
    position: relative;
    z-index: 1;
    width: min(880px, 100%);
    max-height: min(90vh, 960px);
    display: flex;
    flex-direction: column;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    color: var(--tx);
    overflow: hidden;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 18px 24px;
    background: var(--bg-3);
    border-bottom: 1px solid var(--line);
  }

  .modal-title-group {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .modal-title-group h3 {
    margin: 0;
    font-size: 18px;
    font-weight: 800;
  }

  .modal-subtitle {
    margin: 2px 0 0;
    font-size: 13px;
    color: var(--mu);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .close-modal-btn {
    display: inline-grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 10px;
    border: 1px solid transparent;
    background: var(--pn);
    color: var(--tx);
    cursor: pointer;
  }

  .close-modal-btn:hover {
    background: var(--pn-2);
  }

  .metadata-form {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  .modal-body {
    flex: 1;
    overflow-y: auto;
    padding: 22px 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .alert-banner {
    padding: 12px 16px;
    border-radius: var(--r-sm);
    font-size: 14px;
    font-weight: 600;
  }

  .alert-banner.error {
    background: rgba(239, 68, 68, 0.14);
    border: 1px solid rgba(239, 68, 68, 0.4);
    color: var(--er, #fca5a5);
  }

  .alert-banner.success {
    background: rgba(34, 197, 94, 0.14);
    border: 1px solid rgba(34, 197, 94, 0.4);
    color: #86efac;
  }

  /* Language Switcher */
  .lang-switcher-box {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 16px;
    background: var(--pn);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
  }

  .lang-switcher-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .lang-switcher-title {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
  }

  .lang-switcher-hint {
    font-size: 12px;
    color: var(--mu);
  }

  .loading-pill {
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--pn-2);
    color: var(--mu);
  }

  .lang-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .lang-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-2);
    color: var(--mu);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    transition: all var(--t-fast) var(--ease);
  }

  .lang-pill:hover {
    color: var(--tx);
    border-color: var(--ac);
  }

  .lang-pill.active {
    background: var(--ac);
    border-color: var(--ac);
    color: var(--ac-ink, #090c12);
  }

  .lang-badge.default {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.16);
  }

  .lang-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ac);
  }

  .lang-pill.active .lang-dot {
    background: currentColor;
  }

  .editor-fieldset {
    border: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-width: 0;
  }

  .fields-stack {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .fields-row {
    display: grid;
    gap: 14px;
  }

  .fields-row.two-cols {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .fields-row.three-cols {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  @media (max-width: 720px) {
    .fields-row.two-cols,
    .fields-row.three-cols {
      grid-template-columns: 1fr;
    }
  }

  .field-block {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .field-label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .field-label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--mu);
  }

  .locale-tag {
    font-size: 10px;
    font-weight: 800;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--pn-2);
    color: var(--tx);
  }

  .inline-link-btn {
    border: none;
    background: none;
    padding: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--ac);
    cursor: pointer;
  }

  .inline-link-btn:hover {
    text-decoration: underline;
  }

  .field-block input,
  .field-block select,
  .field-block textarea,
  .inline-add-box input,
  .inline-add-box select {
    width: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    border-radius: var(--r-sm);
    border: 1px solid var(--line);
    background: var(--bg-1, #0c0f16);
    color: var(--tx);
    font-size: 14px;
    font-family: inherit;
  }

  .field-block textarea {
    resize: vertical;
    line-height: 1.5;
  }

  .field-block input:focus,
  .field-block select:focus,
  .field-block textarea:focus,
  .inline-add-box input:focus,
  .inline-add-box select:focus {
    outline: 2px solid var(--ac);
    outline-offset: -1px;
  }

  .section-divider {
    width: 100%;
    height: 1px;
    border: none;
    background: var(--line);
    margin: 4px 0;
  }

  .editor-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }

  .section-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .section-head h4,
  .editor-section h4 {
    margin: 0;
    font-size: 16px;
    font-weight: 700;
    color: var(--tx);
  }

  .add-circle-btn {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 1px solid var(--line);
    background: var(--pn-2);
    color: var(--tx);
    font-size: 18px;
    font-weight: 700;
    line-height: 1;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }

  .add-circle-btn:hover {
    background: var(--ac);
    color: var(--ac-ink, #090c12);
    border-color: var(--ac);
  }

  .inline-add-box {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 8px;
    align-items: center;
    padding: 10px;
    border-radius: var(--r-sm);
    background: var(--pn);
    border: 1px solid var(--line);
  }

  .inline-add-box.person-add-box {
    grid-template-columns: 1fr 1fr 140px auto;
  }

  @media (max-width: 720px) {
    .inline-add-box.person-add-box {
      grid-template-columns: 1fr;
    }
  }

  .interactive-list {
    display: flex;
    flex-direction: column;
    border-radius: var(--r-sm);
    background: var(--pn);
    border: 1px solid var(--line);
    overflow: hidden;
  }

  .interactive-list.people-list {
    max-height: 320px;
    overflow-y: auto;
  }

  .list-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--line);
  }

  .list-row:last-child {
    border-bottom: none;
  }

  .list-row.compact {
    padding: 8px 12px;
  }

  .list-row-left {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .list-row-icon {
    display: inline-grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--bg-2);
    color: var(--mu);
    flex-shrink: 0;
    overflow: hidden;
  }

  .person-avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .list-row-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--tx);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .list-row-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .list-row-text strong {
    font-size: 14px;
    color: var(--tx);
  }

  .list-row-text span {
    font-size: 12px;
    color: var(--mu);
  }

  .trash-btn {
    display: inline-grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    border: none;
    background: transparent;
    color: var(--mu);
    cursor: pointer;
    flex-shrink: 0;
  }

  .trash-btn:hover {
    background: rgba(239, 68, 68, 0.16);
    color: #fca5a5;
  }

  .empty-list-hint {
    margin: 0;
    padding: 12px 14px;
    font-size: 13px;
    color: var(--mu);
  }

  .top-gap {
    margin-top: 10px;
  }

  /* Locking section (Screenshot 4) */
  .lock-section {
    gap: 16px;
  }

  .checkbox-row {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    font-size: 14px;
    font-weight: 600;
    color: var(--tx);
    cursor: pointer;
  }

  .checkbox-row input[type='checkbox'] {
    width: 18px;
    height: 18px;
    accent-color: var(--ac);
    cursor: pointer;
  }

  .lock-fields-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .lock-hint {
    margin: 0;
    font-size: 13px;
    color: var(--mu);
  }

  .lock-checkboxes-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    margin-top: 4px;
  }

  @media (max-width: 600px) {
    .lock-checkboxes-grid {
      grid-template-columns: 1fr;
    }
  }

  /* Multi-node copy grouping */
  .copy-group-row {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: flex-end;
  }

  .flex-grow {
    flex: 1;
    min-width: 220px;
  }

  .self-end {
    align-self: flex-end;
  }

  .merges-list,
  .candidates-box {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 8px;
  }

  .candidate-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 12px;
    border-radius: var(--r-sm);
    background: var(--pn);
  }

  .candidate-actions {
    display: flex;
    gap: 6px;
  }

  /* Footer */
  .modal-footer.jellyfin-footer {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 16px 24px;
    background: var(--bg-3);
    border-top: 1px solid var(--line);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 40px;
    padding: 0 20px;
    border-radius: 10px;
    border: 1px solid transparent;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: all var(--t-fast) var(--ease);
  }

  .btn.small {
    min-height: 34px;
    padding: 0 12px;
    font-size: 13px;
  }

  .btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .primary-btn {
    background: var(--ac);
    color: var(--ac-ink, #090c12);
  }

  .secondary-btn {
    background: var(--pn-2);
    color: var(--tx);
  }

  .outline-btn {
    background: transparent;
    border-color: var(--line);
    color: var(--tx);
  }

  .danger-btn {
    background: rgba(239, 68, 68, 0.16);
    border-color: rgba(239, 68, 68, 0.35);
    color: #fca5a5;
  }
</style>

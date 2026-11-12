<script lang="ts">
  import { t } from '../i18n';
  import Icon from '../ui/Icon.svelte';
  import type { Work } from './api';
  import { type MetadataPersonType, type WorkMetadataDraft } from './work-metadata';

  let { draft = $bindable(), work }: { draft: WorkMetadataDraft; work: Work } = $props();

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

  const tmdbLabel = $derived(
    work.kind === 'film'
      ? $t('meta_tmdb_movie_id')
      : work.kind === 'series'
        ? $t('meta_tmdb_series_id')
        : $t('meta_tmdb_episode_id')
  );

  const tvdbLabel = $derived(
    work.kind === 'series'
      ? $t('meta_tvdb_series_id')
      : work.kind === 'episode'
        ? $t('meta_tvdb_episode_id')
        : $t('meta_tvdb_id')
  );

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
            ? $t('meta_director')
            : newPersonType === 'writer'
              ? $t('meta_writer')
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
    if (type === 'director') return $t('meta_director');
    if (type === 'writer') return $t('meta_writer');
    return $t('meta_actor');
  }
</script>

<hr class="section-divider" />

<!-- Screenshot 2 & 3: External Identifiers -->
<section class="editor-section">
  <div class="section-head">
    <h4>{$t('meta_external_ids')}</h4>
    <button
      type="button"
      class="add-circle-btn"
      title={$t('meta_add_external_id')}
      aria-label={$t('meta_add_external_id')}
      onclick={() => (showAddExtraId = !showAddExtraId)}
    >
      +
    </button>
  </div>

  <div class="fields-row two-cols">
    <label class="field-block">
      <span class="field-label">{$t('meta_imdb_id')}</span>
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
        <span class="field-label">{$t('meta_tmdb_collection_id')}</span>
        <input maxlength="64" placeholder="1165374" bind:value={draft.collectionId} />
      </label>
    {/if}
  </div>

  {#if work.kind === 'film' && draft.collectionId.trim()}
    <label class="field-block top-gap">
      <span class="field-label">{$t('meta_collection_name')}</span>
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
            aria-label={$t('meta_remove_item', { name: item.key })}
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
        placeholder={$t('meta_key_placeholder')}
        bind:value={newExtraIdKey}
      />
      <input
        placeholder={$t('meta_value_placeholder')}
        bind:value={newExtraIdValue}
      />
      <button type="button" class="btn small primary-btn" onclick={addExtraExternalId}>
        {$t('meta_add_button')}
      </button>
    </div>
  {/if}
</section>

<hr class="section-divider" />

<!-- Screenshot 3: Genres -->
<section class="editor-section">
  <div class="section-head">
    <h4>{$t('meta_genres_title')}</h4>
    <button
      type="button"
      class="add-circle-btn"
      title={$t('meta_add_genre')}
      aria-label={$t('meta_add_genre')}
      onclick={() => (showAddGenre = !showAddGenre)}
    >
      +
    </button>
  </div>

  {#if showAddGenre}
    <div class="inline-add-box">
      <input
        placeholder={$t('action_adventure_drama')}
        bind:value={newGenre}
        onkeydown={e => {
          if (e.key === 'Enter') {
            e.preventDefault();
            addGenre();
          }
        }}
      />
      <button type="button" class="btn small primary-btn" onclick={addGenre}>
        {$t('meta_add_button')}
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
          aria-label={$t('meta_remove_item', { name: genre })}
          onclick={() => removeGenre(idx)}
        >
          <Icon name="trash" size={16} />
        </button>
      </div>
    {:else}
      <p class="empty-list-hint">{$t('meta_no_genres')}</p>
    {/each}
  </div>
</section>

<hr class="section-divider" />

<!-- Screenshot 3: Personnes -->
<section class="editor-section">
  <div class="section-head">
    <h4>{$t('meta_people_title')}</h4>
    <button
      type="button"
      class="add-circle-btn"
      title={$t('meta_add_person')}
      aria-label={$t('meta_add_person')}
      onclick={() => (showAddPerson = !showAddPerson)}
    >
      +
    </button>
  </div>

  {#if showAddPerson}
    <div class="inline-add-box person-add-box">
      <input
        placeholder={$t('meta_person_name_placeholder')}
        bind:value={newPersonName}
      />
      <input
        placeholder={$t('meta_person_role_placeholder')}
        bind:value={newPersonRole}
      />
      <select bind:value={newPersonType}>
        <option value="actor">{$t('meta_actor')}</option>
        <option value="director">{$t('meta_director')}</option>
        <option value="writer">{$t('meta_writer')}</option>
      </select>
      <button type="button" class="btn small primary-btn" onclick={addPerson}>
        {$t('meta_add_button')}
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
          aria-label={$t('meta_remove_item', { name: person.name })}
          onclick={() => removePerson(idx)}
        >
          <Icon name="trash" size={16} />
        </button>
      </div>
    {:else}
      <p class="empty-list-hint">{$t('meta_no_people')}</p>
    {/each}
  </div>
</section>

<hr class="section-divider" />

<!-- Screenshot 4: Studios, Pays de production & Étiquettes -->
<div class="fields-row three-cols">
  <!-- Pays de production -->
  <section class="editor-section">
    <div class="section-head">
      <h4>{$t('meta_countries_title')}</h4>
      <button
        type="button"
        class="add-circle-btn"
        aria-label={$t('meta_add_country')} title={$t('meta_add_country')}
        onclick={() => (showAddCountry = !showAddCountry)}
      >
        +
      </button>
    </div>

    {#if showAddCountry}
      <div class="inline-add-box compact">
        <input
          placeholder={$t('meta_country_placeholder')}
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
            aria-label={$t('meta_remove_item', { name: country })}
            onclick={() => removeCountry(idx)}
          >
            <Icon name="trash" size={15} />
          </button>
        </div>
      {:else}
        <p class="empty-list-hint">{$t('meta_no_countries')}</p>
      {/each}
    </div>
  </section>

  <!-- Studios -->
  <section class="editor-section">
    <div class="section-head">
      <h4>{$t('meta_studios_title')}</h4>
      <button
        type="button"
        class="add-circle-btn"
        aria-label={$t('meta_add_studio')} title={$t('meta_add_studio')}
        onclick={() => (showAddStudio = !showAddStudio)}
      >
        +
      </button>
    </div>

    {#if showAddStudio}
      <div class="inline-add-box compact">
        <input
          placeholder={$t('meta_studio_placeholder')}
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
            aria-label={$t('meta_remove_item', { name: studio })}
            onclick={() => removeStudio(idx)}
          >
            <Icon name="trash" size={15} />
          </button>
        </div>
      {:else}
        <p class="empty-list-hint">{$t('meta_no_studios')}</p>
      {/each}
    </div>
  </section>

  <!-- Étiquettes -->
  <section class="editor-section">
    <div class="section-head">
      <h4>{$t('meta_tags_title')}</h4>
      <button
        type="button"
        class="add-circle-btn"
        aria-label={$t('meta_add_tag')} title={$t('meta_add_tag')}
        onclick={() => (showAddTag = !showAddTag)}
      >
        +
      </button>
    </div>

    {#if showAddTag}
      <div class="inline-add-box compact">
        <input
          placeholder={$t('meta_tag_placeholder')}
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
            aria-label={$t('meta_remove_item', { name: tag })}
            onclick={() => removeTag(idx)}
          >
            <Icon name="trash" size={15} />
          </button>
        </div>
      {:else}
        <p class="empty-list-hint">{$t('meta_no_tags')}</p>
      {/each}
    </div>
  </section>
</div>

<style>
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

  .field-label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--mu);
  }

  .field-block input,
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

  .field-block input:focus,
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
</style>

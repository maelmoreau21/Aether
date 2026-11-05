<script lang="ts">
  import { providerErrorText } from './provider-errors';
  import { providerImagePreview } from './provider-image';
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../../lib/auth/session';
  import Icon from '../ui/Icon.svelte';
  import type { Work } from './api';

  type IdentifyResult = {
    provider: string;
    providerName?: string;
    id: string;
    title: string;
    originalTitle?: string;
    year?: number;
    overview?: string;
    posterUrl?: string;
  };

  let {
    work,
    onclose,
    onsaved = () => {}
  }: {
    work: Work;
    onclose: () => void;
    onsaved?: () => void;
  } = $props();

  let searchTitle = $state('');
  let searchYear = $state<number | undefined>(undefined);
  let searchProvider = $state('all');
  let replaceImages = $state(true);
  let tmdbId = $state('');
  let imdbId = $state('');
  let tvdbId = $state('');

  $effect(() => {
    searchTitle = work.title || '';
    searchYear = work.year ? Number(work.year) : undefined;
    tmdbId = work.externalIds?.tmdb ?? '';
    imdbId = work.externalIds?.imdb ?? '';
    tvdbId = work.externalIds?.tvdb ?? '';
  });

  let results = $state<IdentifyResult[]>([]);
  let providersList = $state<{ id: string; name: string }[]>([]);
  let searching = $state(false);
  let applying = $state(false);
  let applyingId = $state<string | null>(null);
  let error = $state('');
  let successMsg = $state('');

  async function performSearch() {
    const externalIds = Object.fromEntries(
      Object.entries({ tmdb: tmdbId, imdb: imdbId, tvdb: tvdbId })
        .map(([key, value]) => [key, value.trim()])
        .filter(([, value]) => value)
    );
    if (!searchTitle.trim() && Object.keys(externalIds).length === 0) {
      error = 'Veuillez saisir un titre ou un identifiant à rechercher.';
      return;
    }
    searching = true;
    error = '';
    results = [];
    try {
      const response = await fetch('/api/v1/admin/extensions/identify/search', {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          'x-csrf-token': $session?.csrfToken ?? ''
        },
        body: JSON.stringify({
          workId: work.id,
          externalIds,
          title: searchTitle.trim(),
          year: searchYear ? Number(searchYear) : undefined,
          provider: searchProvider === 'all' ? null : searchProvider
        }),
        signal: AbortSignal.timeout(45000)
      });

      if (!response.ok) {
        throw new Error(`Recherche indisponible (HTTP ${response.status}). Vérifiez vos extensions activées.`);
      }

      const data = await response.json();
      results = data.results ?? [];
      providersList = data.providers ?? [];
      const failures = (data.errors ?? []) as { providerName?: string; provider: string; error: string }[];
      if (results.length === 0 && failures.length > 0) {
        error = failures
          .map((f) => `${f.providerName ?? f.provider} : ${providerErrorText(f.error)}`)
          .join(' · ');
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Erreur lors de la recherche.';
    } finally {
      searching = false;
    }
  }

  async function applyResult(item: IdentifyResult) {
    if (applying) return;
    applying = true;
    applyingId = item.id;
    error = '';
    try {
      const response = await fetch('/api/v1/admin/extensions/identify/apply', {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          'x-csrf-token': $session?.csrfToken ?? ''
        },
        body: JSON.stringify({
          workId: work.id,
          provider: item.provider,
          candidateId: item.id,
          replaceImages
        }),
        signal: AbortSignal.timeout(45000)
      });

      if (!response.ok) {
        throw new Error(`Échec de l'application des métadonnées (HTTP ${response.status}).`);
      }

      successMsg = 'Identification réussie ! Mise à jour des métadonnées…';
      setTimeout(() => {
        onsaved();
        onclose();
      }, 700);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Erreur lors de l’enregistrement.';
      applying = false;
      applyingId = null;
    }
  }

  onMount(() => {
    void performSearch();
  });
</script>

<div class="modal-overlay" role="dialog" aria-modal="true" aria-labelledby="identify-modal-title">
  <button
    type="button"
    class="modal-backdrop-click"
    aria-label="Fermer la fenêtre"
    tabindex="-1"
    onclick={onclose}
  ></button>

  <div class="modal-card">
    <div class="modal-header">
      <div class="modal-title-group">
        <Icon name="search" size={20} />
        <h3 id="identify-modal-title">Identifier · {work.title}</h3>
      </div>
      <button
        type="button"
        class="close-modal-btn"
        aria-label="Fermer"
        disabled={applying}
        onclick={onclose}
      >
        <Icon name="close" size={18} />
      </button>
    </div>

    <div class="modal-body">
      <!-- Search Form -->
      <form class="identify-search-bar" onsubmit={(e) => { e.preventDefault(); void performSearch(); }}>
        <div class="input-row">
          <label class="field-group flex-2">
            <span>Titre</span>
            <input
              type="text"
              maxlength="256"
              placeholder="Ex. Inception"
              bind:value={searchTitle}
              disabled={searching || applying}
            />
          </label>

          <label class="field-group flex-1">
            <span>Année</span>
            <input
              type="number"
              min="1880"
              max="2100"
              placeholder="Ex. 2010"
              bind:value={searchYear}
              disabled={searching || applying}
            />
          </label>

          {#if providersList.length > 1}
            <label class="field-group flex-1">
              <span>Source</span>
              <select bind:value={searchProvider} disabled={searching || applying}>
                <option value="all">Toutes sources</option>
                {#each providersList as p (p.id)}
                  <option value={p.id}>{p.name}</option>
                {/each}
              </select>
            </label>
          {/if}

          <div class="field-group search-btn-group">
            <span class="spacer-label">&nbsp;</span>
            <button
              type="submit"
              class="btn primary-btn search-submit-btn"
              disabled={searching || applying}
            >
              {#if searching}
                <span class="mini-spinner"></span>
                <span>Recherche…</span>
              {:else}
                <Icon name="search" size={16} />
                <span>Rechercher</span>
              {/if}
            </button>
          </div>
        </div>

        <details class="identify-ids">
          <summary>Identifiants des fournisseurs</summary>
          <p>Les identifiants du NFO sont repris automatiquement. Videz-les pour rechercher uniquement par titre.</p>
          <div class="input-row">
            <label class="field-group flex-1"><span>TMDb</span><input aria-label="Identifiant TMDb" type="text" inputmode="numeric" maxlength="32" placeholder="Ex. 27205" bind:value={tmdbId} disabled={searching || applying} /></label>
            <label class="field-group flex-1"><span>IMDb</span><input aria-label="Identifiant IMDb" type="text" maxlength="32" placeholder="Ex. tt1375666" bind:value={imdbId} disabled={searching || applying} /></label>
            <label class="field-group flex-1"><span>TVDb</span><input aria-label="Identifiant TVDb" type="text" inputmode="numeric" maxlength="32" bind:value={tvdbId} disabled={searching || applying} /></label>
          </div>
        </details>

        <div class="options-row">
          <label class="checkbox-label">
            <input
              type="checkbox"
              bind:checked={replaceImages}
              disabled={searching || applying}
            />
            Remplacer également l’affiche et l’arrière-plan existants par ceux du fournisseur
          </label>
        </div>
      </form>

      {#if error}
        <p class="status-msg error" role="alert">{error}</p>
      {/if}
      {#if successMsg}
        <p class="status-msg success" role="status">{successMsg}</p>
      {/if}

      <!-- Results List -->
      <div class="results-section">
        {#if searching}
          <div class="loading-state">
            <div class="spinner"></div>
            <p>Interrogation des fournisseurs en direct…</p>
          </div>
        {:else if providersList.length === 0}
          <div class="no-provider-notice" role="status">
            <div class="notice-icon">
              <Icon name="sparkles" size={28} />
            </div>
            <div class="notice-content">
              <h4>Aucun fournisseur de métadonnées actif</h4>
              <p>
                Activez un fournisseur de métadonnées pour rechercher ce média ou utiliser ses identifiants NFO.
              </p>
              <p>
                Vérifiez l’activation et l’accès à cette bibliothèque dans les paramètres de <strong>TMDb</strong> ou <strong>IMDb</strong>.
              </p>
              <a href={resolve('admin/libraries/#extensions-section')} class="btn primary-btn notice-link">
                <Icon name="search" size={15} />
                <span>Gérer les extensions</span>
              </a>
            </div>
          </div>
        {:else if results.length === 0}
          <div class="empty-state">
            <Icon name="search" size={24} />
            <p>
              Aucun résultat trouvé via {providersList.length === 1 ? providersList[0].name : `${providersList.length} fournisseurs`}.
            </p>
            <p class="empty-hint">
              Vérifiez l'orthographe du titre, essayez sans renseigner l'année, ou assurez-vous que votre clé d'API est bien configurée dans les réglages de l'extension.
            </p>
          </div>
        {:else}
          <div class="results-grid">
            {#each results as item (item.provider + ':' + item.id)}
              <article class="result-card">
                <div class="result-poster">
                  {#if item.posterUrl}
                    <img src={providerImagePreview(item.provider, work.id, item.posterUrl)} alt={`Affiche de ${item.title}`} loading="lazy" />
                  {:else}
                    <div class="poster-placeholder">
                      <Icon name="image" size={32} />
                    </div>
                  {/if}
                </div>

                <div class="result-info">
                  <div class="result-header-row">
                    <h4 class="result-title">{item.title}</h4>
                    <span class="provider-pill">{item.providerName || item.provider.toUpperCase()}</span>
                  </div>

                  <div class="result-meta">
                    {#if item.year}<span class="meta-tag">{item.year}</span>{/if}
                    {#if item.originalTitle && item.originalTitle !== item.title}
                      <span class="meta-orig">({item.originalTitle})</span>
                    {/if}
                  </div>

                  {#if item.overview}
                    <p class="result-overview">{item.overview}</p>
                  {/if}

                  <div class="result-actions">
                    <button
                      type="button"
                      class="btn select-btn"
                      disabled={applying}
                      onclick={() => applyResult(item)}
                    >
                      {#if applying && applyingId === item.id}
                        <span class="mini-spinner"></span>
                        <span>Application…</span>
                      {:else}
                        <span>Sélectionner</span>
                      {/if}
                    </button>
                  </div>
                </div>
              </article>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .identify-ids { margin: 4px 0 0; }
  .identify-ids summary {
    display: inline-flex;
    align-items: center;
    min-height: 44px;
    cursor: pointer;
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }
  .identify-ids summary:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
    border-radius: var(--r-xs);
  }
  .identify-ids p { color: var(--mu); font-size: 13px; margin: 4px 0 12px; line-height: 1.5; }
  .modal-overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgb(6 8 14 / 0.55);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
  }
  .modal-backdrop-click {
    position: absolute;
    inset: 0;
    border: none;
    background: transparent;
    cursor: default;
  }
  .modal-card {
    position: relative;
    z-index: 1001;
    width: min(900px, 100%);
    max-height: calc(100dvh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--bg-3);
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    box-shadow: var(--shadow);
    color: var(--tx);
    overflow: hidden;
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 24px 28px 8px;
    flex: none;
  }
  .modal-title-group {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
    color: var(--mu);
  }
  .modal-title-group h3 {
    margin: 0;
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
    color: var(--tx);
    overflow-wrap: anywhere;
  }
  .close-modal-btn {
    display: grid;
    place-items: center;
    flex: none;
    width: 44px;
    height: 44px;
    padding: 0;
    border: none;
    border-radius: 14px;
    background: var(--pn);
    color: var(--tx);
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease);
  }
  .close-modal-btn:hover:not(:disabled) {
    background: var(--pn-2);
  }
  .modal-body {
    padding: 16px 28px 28px;
    overflow-y: auto;
    overscroll-behavior: contain;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  /* Search bar */
  .identify-search-bar {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .input-row {
    display: flex;
    gap: 12px;
    align-items: flex-end;
    flex-wrap: wrap;
  }
  .field-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .field-group span {
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }
  .flex-1 { flex: 1; min-width: 120px; }
  .flex-2 { flex: 2; min-width: 200px; }
  .spacer-label { user-select: none; }
  input, select {
    box-sizing: border-box;
    width: 100%;
    height: 46px;
    min-height: 46px;
    padding: 0 14px;
    background: var(--pn);
    color: var(--tx);
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    font: inherit;
    font-size: 14px;
    box-shadow: none;
    transition: border-color var(--t-fast) var(--ease), background-color var(--t-fast) var(--ease);
  }
  select option {
    background: var(--bg-3);
    color: var(--tx);
  }
  input:focus, select:focus {
    outline: none;
    border-color: var(--ac);
    background: var(--bg-3);
    box-shadow: none;
  }
  .search-submit-btn {
    height: 46px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 0 20px;
  }
  .options-row {
    padding-top: 2px;
  }
  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    font-size: 14px;
    font-weight: 600;
    color: var(--tx);
    cursor: pointer;
  }
  .checkbox-label input {
    flex: none;
    width: 18px;
    height: 18px;
    min-height: 18px;
    padding: 0;
    accent-color: var(--ac);
    background: none;
  }

  /* Status messages */
  .status-msg {
    padding: 12px 16px;
    border-radius: var(--r-sm);
    font-size: 14px;
    font-weight: 600;
    margin: 0;
  }
  .status-msg.error {
    background: var(--err-soft);
    color: var(--err);
  }
  .status-msg.success {
    background: var(--ok-soft);
    color: var(--ok);
  }

  /* Results Grid */
  .results-section {
    min-height: 240px;
  }
  .loading-state, .empty-state {
    text-align: center;
    padding: 40px 0;
    color: var(--mu);
  }
  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--line-2);
    border-top-color: var(--ac);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    margin: 0 auto 12px;
  }
  .mini-spinner {
    width: 14px;
    height: 14px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    display: inline-block;
    opacity: 0.8;
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .results-grid {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .result-card {
    display: flex;
    gap: 16px;
    padding: 12px;
    background: var(--pn);
    border-radius: var(--r-md);
    transition: background-color var(--t-fast) var(--ease);
  }
  .result-card:hover {
    background: var(--pn-2);
  }
  .result-poster {
    width: 84px;
    min-width: 84px;
    align-self: flex-start;
    aspect-ratio: 2 / 3;
    background: var(--pn-2);
    border-radius: var(--r-sm);
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .result-poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .poster-placeholder {
    color: var(--mu);
  }
  .result-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .result-header-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }
  .result-title {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    color: var(--tx);
    overflow-wrap: anywhere;
  }
  .provider-pill {
    flex: none;
    font-size: 12px;
    font-weight: 700;
    padding: 3px 10px;
    border-radius: var(--r-pill);
    background: var(--pn-2);
    color: var(--mu);
  }
  .result-meta {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    font-size: 13px;
  }
  .meta-tag {
    color: var(--mu);
    font-variant-numeric: tabular-nums;
  }
  .meta-orig {
    color: var(--mu);
    font-style: italic;
  }
  .result-overview {
    font-size: 13px;
    color: var(--mu);
    line-height: 1.5;
    margin: 2px 0 6px;
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .result-actions {
    margin-top: auto;
    display: flex;
    justify-content: flex-end;
  }
  .btn {
    min-height: 44px;
    border-radius: var(--r-md);
    padding: 0 18px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    border: 1px solid transparent;
    transition: background-color var(--t-fast) var(--ease), color var(--t-fast) var(--ease), opacity var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  .btn:active:not(:disabled) {
    transform: scale(0.98);
  }
  .primary-btn {
    background: var(--btn);
    color: var(--btn-tx);
  }
  .primary-btn:hover:not(:disabled) {
    opacity: 0.9;
  }
  .select-btn {
    background: var(--pn-2);
    color: var(--tx);
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .select-btn:hover:not(:disabled) {
    background: var(--btn);
    color: var(--btn-tx);
  }

  .no-provider-notice {
    display: flex;
    align-items: flex-start;
    gap: 16px;
    padding: 20px;
    background: var(--pn);
    border-radius: var(--r-md);
    margin: 12px 0;
  }
  .notice-icon {
    color: var(--mu);
    flex-shrink: 0;
    margin-top: 2px;
  }
  .notice-content {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .notice-content h4 {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    color: var(--tx);
  }
  .notice-content p {
    margin: 0;
    font-size: 13px;
    color: var(--mu);
    line-height: 1.5;
  }
  .notice-link {
    align-self: flex-start;
    margin-top: 6px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    text-decoration: none;
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 32px 16px;
    gap: 8px;
    color: var(--mu);
    text-align: center;
  }
  .empty-state p { margin: 0; color: var(--tx); font-weight: 600; }
  .empty-state .empty-hint {
    font-size: 13px;
    max-width: 440px;
    color: var(--mu);
    font-weight: 500;
  }

  @media (max-width: 639px) {
    .modal-overlay { padding: 0; align-items: stretch; }
    .modal-card {
      width: 100%;
      max-height: 100dvh;
      height: 100dvh;
      border: 0;
      border-radius: 0;
    }
    .modal-header { padding: 20px 16px 8px; }
    .modal-body { padding: 12px 16px calc(24px + env(safe-area-inset-bottom, 0px)); }
    .search-btn-group, .search-submit-btn { width: 100%; justify-content: center; }
    .search-btn-group .spacer-label { display: none; }
    .result-poster { width: 64px; min-width: 64px; }
  }
</style>

<script lang="ts">
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

  $effect(() => {
    searchTitle = work.title || '';
    searchYear = work.year ? Number(work.year) : undefined;
  });

  let results = $state<IdentifyResult[]>([]);
  let providersList = $state<{ id: string; name: string }[]>([]);
  let searching = $state(false);
  let applying = $state(false);
  let applyingId = $state<string | null>(null);
  let error = $state('');
  let successMsg = $state('');

  async function performSearch() {
    if (!searchTitle.trim()) {
      error = 'Veuillez saisir un titre à rechercher.';
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
          title: searchTitle.trim(),
          year: searchYear ? Number(searchYear) : undefined,
          provider: searchProvider === 'all' ? null : searchProvider
        }),
        signal: AbortSignal.timeout(30000)
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
          .map((f) => `${f.providerName ?? f.provider} : ${f.error}`)
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
              required
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
                Contrairement à Jellyfin qui intègre des clés de métadonnées mutualisées, Aether isole strictement ses connecteurs dans des extensions sécurisées (TMDb, OMDb, IMDb).
              </p>
              <p>
                Pour identifier ce média et importer ses affiches et résumés, installez et activez le plugin <strong>IMDb (sans clé d'API requise)</strong> ou <strong>TMDb</strong> dans la section Administration.
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
                    <img src={item.posterUrl} alt={`Affiche de ${item.title}`} loading="lazy" />
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
  .modal-overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: rgba(0, 0, 0, 0.8);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
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
    max-height: calc(100dvh - 32px);
    display: flex;
    flex-direction: column;
    background: var(--color-glass-raised);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow-lg);
    color: var(--color-text-bright);
    overflow: hidden;
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 22px;
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-line);
  }
  .modal-title-group {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--color-accent);
  }
  .modal-title-group h3 {
    margin: 0;
    font-size: 17px;
    font-weight: 650;
    color: var(--color-text-bright);
  }
  .close-modal-btn {
    background: transparent;
    border: none;
    color: var(--color-muted);
    cursor: pointer;
    padding: 6px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
  }
  .close-modal-btn:hover:not(:disabled) {
    background: var(--color-raised);
    color: var(--color-text-bright);
  }
  .modal-body {
    padding: 22px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  /* Search bar */
  .identify-search-bar {
    background: var(--color-surface);
    padding: 16px;
    border-radius: var(--radius-md);
    border: 1px solid var(--color-line);
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
    gap: 6px;
  }
  .field-group span {
    font-size: 12px;
    font-weight: 600;
    color: var(--color-muted);
  }
  .flex-1 { flex: 1; min-width: 120px; }
  .flex-2 { flex: 2; min-width: 200px; }
  .spacer-label { user-select: none; }
  input, select {
    background: var(--color-raised);
    color: var(--color-text-bright);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 8px 12px;
    font-size: 14px;
    height: 40px;
  }
  input:focus, select:focus {
    outline: none;
    border-color: var(--color-accent);
  }
  .search-submit-btn {
    height: 40px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 0 18px;
  }
  .options-row {
    padding-top: 4px;
  }
  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--color-muted);
    cursor: pointer;
  }
  .checkbox-label input {
    height: 16px;
    width: 16px;
  }

  /* Status messages */
  .status-msg {
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: 14px;
    margin: 0;
  }
  .status-msg.error {
    background: var(--color-danger-soft);
    border: 1px solid rgba(239, 68, 68, 0.35);
    color: var(--color-danger);
  }
  .status-msg.success {
    background: var(--color-success-soft);
    border: 1px solid rgba(16, 185, 129, 0.35);
    color: var(--color-success);
  }

  /* Results Grid */
  .results-section {
    min-height: 240px;
  }
  .loading-state, .empty-state {
    text-align: center;
    padding: 40px 0;
    color: var(--color-muted);
  }
  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--color-line-strong);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    margin: 0 auto 12px;
  }
  .mini-spinner {
    width: 14px;
    height: 14px;
    border: 2px solid rgba(255, 255, 255, 0.3);
    border-top-color: #ffffff;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    display: inline-block;
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .results-grid {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .result-card {
    display: flex;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    overflow: hidden;
    transition: border-color var(--transition-fast), transform var(--transition-fast);
  }
  .result-card:hover {
    border-color: var(--color-accent);
    transform: translateY(-1px);
  }
  .result-poster {
    width: 90px;
    min-width: 90px;
    aspect-ratio: 2 / 3;
    background: var(--color-raised);
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
    color: var(--color-muted);
  }
  .result-info {
    padding: 12px 16px;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .result-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .result-title {
    margin: 0;
    font-size: 16px;
    font-weight: 650;
    color: var(--color-text-bright);
  }
  .provider-pill {
    font-size: 11px;
    font-weight: 700;
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border: 1px solid rgba(229, 9, 20, 0.3);
  }
  .result-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .meta-tag {
    color: var(--color-muted);
  }
  .meta-orig {
    color: var(--color-muted);
    font-style: italic;
  }
  .result-overview {
    font-size: 13px;
    color: var(--color-muted);
    line-height: 1.45;
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
    border-radius: var(--radius-sm);
    padding: 7px 15px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    border: 1px solid transparent;
  }
  .primary-btn {
    background: var(--color-play);
    color: var(--color-on-play);
  }
  .primary-btn:hover:not(:disabled) {
    background: var(--color-play-hover);
  }
  .select-btn {
    background: var(--color-raised);
    color: var(--color-text-bright);
    border-color: var(--color-line-strong);
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .select-btn:hover:not(:disabled) {
    background: var(--color-play);
    color: var(--color-on-play);
    border-color: var(--color-play);
  }

  .no-provider-notice {
    display: flex;
    align-items: flex-start;
    gap: 16px;
    padding: 20px;
    background: rgba(249, 115, 22, 0.08);
    border: 1px solid rgba(249, 115, 22, 0.25);
    border-radius: var(--radius-md);
    margin: 12px 0;
  }
  .notice-icon {
    color: var(--color-accent);
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
    font-size: 15px;
    font-weight: 600;
    color: var(--color-text-bright);
  }
  .notice-content p {
    margin: 0;
    font-size: 13px;
    color: var(--color-muted);
    line-height: 1.45;
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
    color: var(--color-muted);
    text-align: center;
  }
  .empty-hint {
    font-size: 12.5px;
    max-width: 440px;
    opacity: 0.85;
  }
</style>

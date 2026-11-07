<script lang="ts">
  import { page } from '$app/state';
  import { resolve } from '$app/paths';
  import { onDestroy } from 'svelte';
  import Navbar from '../../lib/ui/Navbar.svelte';
  import Poster from '../../lib/catalog/Poster.svelte';
  import { creditPeople, initials } from '../../lib/catalog/credits';
  import type { Work } from '../../lib/catalog/api';
  import { getSessionGeneration, session } from '../../lib/auth/session';
  import { t } from '../../lib/i18n';

  const name = $derived((page.url.searchParams.get('name') ?? '').trim());
  const sourceId = $derived(page.url.searchParams.get('work') ?? '');
  const creditKey = $derived(page.url.searchParams.get('credit') ?? '');
  let source = $state<Work | null>(null);
  let items = $state<Work[]>([]);
  let hasMore = $state(false);
  let offset = $state(0);
  let loading = $state(false);
  let error = $state('');
  let photoFailed = $state(false);
  let controller: AbortController | undefined;
  const photo = $derived.by(() => {
    const matches = creditPeople(source ?? {}).filter(person => person.name.toLocaleLowerCase() === name.toLocaleLowerCase());
    const credit = creditKey ? matches.find(person => person.key === creditKey) : matches.length === 1 ? matches[0] : undefined;
    return credit?.photoUrl ?? '';
  });
  const returnHref = $derived(source?.kind === 'series'
    ? resolve(`series/?work=${encodeURIComponent(sourceId)}` as 'series/')
    : source?.kind === 'film'
      ? resolve(`films/?work=${encodeURIComponent(sourceId)}` as 'films/')
      : resolve('/'));

  async function load(personName: string, workId: string, append = false) {
    controller?.abort();
    const current = new AbortController(); controller = current;
    const generation = getSessionGeneration();
    loading = true; error = '';
    const nextOffset = append ? offset : 0;
    try {
      const search = new URLSearchParams({ person: personName, offset: String(nextOffset), limit: '24', count: 'false' });
      const response = await fetch(`/api/v1/catalog?${search}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.any([current.signal, AbortSignal.timeout(15000)]) });
      if (!response.ok) throw new Error($t('unable_to_load_the_works_try'));
      const result: { items: Work[]; hasMore?: boolean; offset: number; limit: number } = await response.json();
      if (!Array.isArray(result.items)) throw new Error($t('unable_to_load_the_works_try'));
      if (current.signal.aborted || generation !== getSessionGeneration()) return;
      items = append ? [...items, ...result.items.filter(item => !items.some(existing => existing.id === item.id))] : result.items;
      offset = result.offset + result.limit;
      hasMore = result.hasMore === true;
      if (!append && workId) {
        try {
          const details = await fetch(`/api/v1/catalog/${encodeURIComponent(workId)}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.any([current.signal, AbortSignal.timeout(10000)]) });
          if (details.ok) {
            const work: Work = await details.json();
            if (!current.signal.aborted && generation === getSessionGeneration()) source = work;
          }
        } catch { /* The list remains useful if the source portrait is unavailable. */ }
      }
    } catch (cause) {
      if (!current.signal.aborted && generation === getSessionGeneration()) error = cause instanceof Error && cause.name !== 'TimeoutError' ? cause.message : $t('the_server_is_taking_too_long');
    } finally {
      if (!current.signal.aborted && generation === getSessionGeneration()) loading = false;
    }
  }

  $effect(() => {
    const selectedName = name;
    const workId = sourceId;
    // Clear and reload when the authenticated account changes.
    void $session;
    source = null; items = []; offset = 0; hasMore = false; photoFailed = false; error = '';
    if (selectedName) void load(selectedName, workId);
    return () => controller?.abort();
  });
  onDestroy(() => controller?.abort());
</script>

<svelte:head><title>{name ? $t('catalog_cast_and_crew', { name }) : $t('cast_and_crew')}</title></svelte:head>
<Navbar />
<main class="person-view">
  <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- returnHref is resolved above. -->
  <a class="back" href={returnHref}>{$t('back_to_the_page')}</a>
  {#if name}
    <header class="person-header">
      <div class="portrait">
        <span aria-hidden="true">{initials(name)}</span>
        {#if photo && !photoFailed}<img src={photo} alt="" width="240" height="240" onerror={() => photoFailed = true} />{/if}
      </div>
      <div><p class="eyebrow">{$t('cast_crew')}</p><h1>{name}</h1><p class="subtitle">{$t('works_in_your_library')}</p></div>
    </header>
    <section aria-labelledby="works-title">
      <h2 id="works-title">{$t('films_and_series')}</h2>
      <p class="explanation">{$t('credits_with_the_same_name_are')}</p>
      {#if error}<p class="status error" role="alert">{error} <button onclick={() => void load(name, sourceId, items.length > 0)}>{$t('request_retry')}</button></p>{/if}
      {#if items.length}
        <div class="works-grid">
          {#each items as work (work.id)}
            <Poster {work} href={work.kind === 'series' ? resolve(`series/?work=${encodeURIComponent(work.id)}` as 'series/') : resolve(`films/?work=${encodeURIComponent(work.id)}` as 'films/')} />
          {/each}
        </div>
      {:else if !loading && !error}<p class="status">{$t('no_accessible_work_for_this_person')}</p>{/if}
      {#if loading}<p class="status" role="status">{$t('loading_works')}</p>{/if}
      {#if hasMore && !loading}<button class="more" onclick={() => void load(name, sourceId, true)}>{$t('catalog_show_more')}</button>{/if}
    </section>
  {:else}<p class="status">{$t('no_person_selected')}</p>{/if}
</main>

<style>
  .person-view { max-width: var(--maxw); margin: 0 auto; padding: 24px var(--gutter) 72px; }
  .back { display: inline-flex; padding: 10px 0; color: var(--mu); font-size: 14px; }
  .back:hover { color: var(--tx); }
  .person-header { display: flex; align-items: center; gap: 28px; margin: 30px 0 42px; }
  .portrait { position: relative; display: grid; place-items: center; flex: 0 0 156px; width: 156px; height: 156px; overflow: hidden; border-radius: 50%; background: var(--pn-2); color: var(--tx); font-size: 44px; font-weight: 800; }
  .portrait img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; object-position: center 20%; }
  .eyebrow { margin: 0 0 8px; color: var(--ac); font-size: 12px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
  h1 { margin: 0; color: var(--tx); font-size: clamp(28px, 4vw, 48px); letter-spacing: -.03em; overflow-wrap: anywhere; }
  h2 { margin: 0 0 6px; color: var(--tx); font-size: 24px; }
  .subtitle, .explanation, .status { color: var(--mu); line-height: 1.6; }
  .subtitle { margin: 8px 0 0; }
  .explanation { margin: 0 0 22px; font-size: 13px; }
  .works-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 24px 16px; }
  button { min-height: 44px; padding: 10px 18px; border: 1px solid var(--line); border-radius: var(--r-md); background: var(--pn); color: var(--tx); font: inherit; cursor: pointer; }
  button:hover { background: var(--pn-2); }
  button:focus-visible, a:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .more { margin-top: 28px; }
  .error { color: var(--err); }
  @media (max-width: 600px) { .person-header { align-items: center; gap: 18px; margin: 18px 0 32px; } .portrait { flex-basis: 88px; width: 88px; height: 88px; font-size: 26px; } .subtitle { font-size: 13px; } .works-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px 12px; } }
</style>

<script lang="ts">
  import { onDestroy } from 'svelte';
  import { t } from '../i18n';
  let { assetId, hasChapters }: { assetId: string; hasChapters?: boolean } = $props();
  type Chapter = { index: number; start: number; title: string; imageUrl: string; image?: string; state?: string };
  let chapters = $state<Chapter[]>([]);
  let available = $state(false);
  let busy = $state(false);
  let error = $state('');
  let opened = $state(false);
  let listController: AbortController | undefined;
  let imagesController: AbortController | undefined;
  function clearImages() { imagesController?.abort(); }
  function clearAll() { listController?.abort(); clearImages(); }
  onDestroy(clearAll);

  $effect(() => {
    const id = assetId;
    const knownEmpty = hasChapters === false;
    clearAll();
    chapters = [];
    available = false;
    error = '';
    opened = false;
    if (knownEmpty || !id) return;
    const current = new AbortController();
    listController = current;
    void (async () => {
      try {
        const response = await fetch(`/api/v1/assets/${encodeURIComponent(id)}/chapter-images`, { signal: current.signal, cache: 'no-store' });
        if (!response.ok) return;
        const data = await response.json() as { chapters?: Chapter[] };
        if (current.signal.aborted) return;
        const list = Array.isArray(data.chapters) ? data.chapters : [];
        if (list.length > 0) {
          chapters = list;
          available = true;
        }
      } catch {
        // Ignore network/abort errors during availability check.
      }
    })();
    return () => current.abort();
  });

  async function loadImages() {
    clearImages();
    const current = new AbortController(); imagesController = current;
    busy = true; error = '';
    try {
      if (!chapters.length) {
        const response = await fetch(`/api/v1/assets/${encodeURIComponent(assetId)}/chapter-images`, { signal: current.signal, cache: 'no-store' });
        if (response.status === 404) { error = $t('chapter_images_are_disabled_or_unavailable'); return; }
        if (!response.ok) throw new Error($t('chapter_images_are_unavailable'));
        chapters = (await response.json()).chapters;
      }
      for (const chapter of chapters) {
        if (current.signal.aborted) return;
        if (chapter.image) continue;
        for (let attempt = 0; attempt < 30; attempt++) {
          const image = await fetch(chapter.imageUrl, { method: 'HEAD', signal: current.signal, cache: 'no-store' });
          if (image.status === 200 && ['image/jpeg', 'image/webp'].includes((image.headers.get('content-type') ?? '').split(';')[0].trim())) {
            chapter.image = chapter.imageUrl; chapter.state = ''; break;
          }
          if (image.status !== 202) { chapter.state = $t('unavailable_try_again_after_playback'); break; }
          chapter.state = $t('generating');
          await new Promise<void>((resolve, reject) => {
            const cancelled = () => { clearTimeout(timer); reject(new DOMException('Aborted', 'AbortError')); };
            const timer = setTimeout(() => { current.signal.removeEventListener('abort', cancelled); resolve(); }, 1000);
            current.signal.addEventListener('abort', cancelled, { once: true });
          });
        }
      }
      // Mutations of the local array entries become reactive by replacing the array.
      chapters = [...chapters];
    } catch (cause) {
      if (!current.signal.aborted) error = cause instanceof Error ? cause.message : $t('chapter_images_are_unavailable');
    } finally { if (imagesController === current) busy = false; }
  }

  function refreshImages() {
    chapters = chapters.map(c => ({ ...c, image: undefined, state: undefined }));
    void loadImages();
  }
</script>

{#if available && chapters.length > 0}
<details ontoggle={event => { opened = event.currentTarget.open; if (opened) void loadImages(); else clearImages(); }}>
  <summary>{$t('chapter_images')}</summary>
  {#if opened}
    <p>{$t('images_extracted_on_demand_by_the')}</p>
    {#if busy}<p role="status">{$t('loading_and_generating_previews')}</p>{/if}
    {#if error}<p role="status">{error}</p>{/if}
    <div class="chapter-images">
      {#each chapters as chapter (chapter.index)}
        <figure>
          {#if chapter.image}<img src={chapter.image} alt={$t('catalog_preview_of', { title: chapter.title })} />{:else}<p>{chapter.state ?? $t('sched_queued')}</p>{/if}
          <figcaption>{chapter.title} · {Math.floor(chapter.start / 60)}:{String(Math.floor(chapter.start % 60)).padStart(2, '0')}</figcaption>
        </figure>
      {/each}
    </div>
    <button type="button" disabled={busy} onclick={refreshImages}>{$t('refresh_images')}</button>
  {/if}
</details>
{/if}

<style>
  details { margin-block: 16px; }
  summary { display: flex; align-items: center; min-height: 44px; cursor: pointer; font-size: 15px; font-weight: 700; color: var(--tx); }
  summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; border-radius: var(--r-xs); }
  p { margin: 8px 0 12px; font-size: 13px; line-height: 1.5; color: var(--mu); }
  .chapter-images { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 220px), 1fr)); gap: 16px; }
  figure { margin: 0; min-width: 0; }
  figure > p { display: grid; place-items: center; aspect-ratio: 16 / 9; margin: 0; padding: 12px; border-radius: var(--r-md); background: var(--pn); text-align: center; }
  img { display: block; width: 100%; height: auto; aspect-ratio: auto 16 / 9; object-fit: cover; border-radius: var(--r-md); background: var(--pn); }
  figcaption { margin-block: 8px 4px; font-size: 13px; font-weight: 600; color: var(--mu); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; margin-top: 16px; padding: 0 16px; border: 1px solid transparent; border-radius: var(--r-md); background: var(--pn); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-2); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  @media (max-width: 639px) { .chapter-images { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; } }
</style>

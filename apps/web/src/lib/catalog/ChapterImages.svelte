<script lang="ts">
  import { onDestroy } from 'svelte';
  let { assetId }: { assetId: string } = $props();
  type Chapter = { index: number; start: number; title: string; imageUrl: string; image?: string; state?: string };
  let chapters = $state<Chapter[]>([]);
  let busy = $state(false);
  let error = $state('');
  let opened = $state(false);
  let controller: AbortController | undefined;
  function clear() { controller?.abort(); }
  onDestroy(clear);
  async function load() {
    clear();
    const current = new AbortController(); controller = current;
    busy = true; error = ''; chapters = [];
    try {
      const response = await fetch(`/api/v1/assets/${encodeURIComponent(assetId)}/chapter-images`, { signal: current.signal, cache: 'no-store' });
      if (response.status === 404) { error = 'Les images de chapitres sont désactivées ou indisponibles pour cette copie.'; return; }
      if (!response.ok) throw new Error('Les images de chapitres sont indisponibles.');
      chapters = (await response.json()).chapters;
      for (const chapter of chapters) {
        if (current.signal.aborted) return;
        for (let attempt = 0; attempt < 30; attempt++) {
          const image = await fetch(chapter.imageUrl, { method: 'HEAD', signal: current.signal, cache: 'no-store' });
          if (image.status === 200 && ['image/jpeg', 'image/webp'].includes((image.headers.get('content-type') ?? '').split(';')[0].trim())) {
            chapter.image = chapter.imageUrl; chapter.state = ''; break;
          }
          if (image.status !== 202) { chapter.state = 'Indisponible ; réessayez après la lecture.'; break; }
          chapter.state = 'Génération en cours…';
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
      if (!current.signal.aborted) error = cause instanceof Error ? cause.message : 'Les images de chapitres sont indisponibles.';
    } finally { if (controller === current) busy = false; }
  }
</script>

<details ontoggle={event => { opened = event.currentTarget.open; if (opened) void load(); else clear(); }}>
  <summary>Images des chapitres</summary>
  {#if opened}
    <p>Images extraites à la demande par le serveur qui possède cette copie. La lecture reste prioritaire.</p>
    {#if busy}<p role="status">Chargement et génération des aperçus…</p>{/if}
    {#if error}<p role="status">{error}</p>{/if}
    {#if !busy && !error && !chapters.length}<p>Ce média ne contient aucun chapitre identifié.</p>{/if}
    <div class="chapter-images">
      {#each chapters as chapter (chapter.index)}
        <figure>
          {#if chapter.image}<img src={chapter.image} alt={`Aperçu de ${chapter.title}`} />{:else}<p>{chapter.state ?? 'En attente…'}</p>{/if}
          <figcaption>{chapter.title} · {Math.floor(chapter.start / 60)}:{String(Math.floor(chapter.start % 60)).padStart(2, '0')}</figcaption>
        </figure>
      {/each}
    </div>
    <button type="button" disabled={busy} onclick={load}>Actualiser les images</button>
  {/if}
</details>

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

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
          if (image.status === 200 && image.headers.get('content-type')?.startsWith('image/jpeg')) {
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
  .chapter-images { display: flex; flex-wrap: wrap; gap: 16px; }
  figure { margin: 0; width: min(320px, 100%); }
  img { display: block; width: 100%; border-radius: var(--radius-md); }
  figcaption { margin-block: 6px 12px; color: var(--color-muted); }
</style>

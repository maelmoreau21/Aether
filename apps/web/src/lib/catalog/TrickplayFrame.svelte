<script lang="ts">
  import { t } from '../i18n';
  import { computePreviewFrame, readyTrickplay, type TrickplayManifest } from '../player/trickplay';
  import { formatClock } from './segments';

  // The preview thumbnail of one instant of a file (the same sheets the player shows when hovering
  // its bar), so a bound can be set on the picture rather than on a number.
  let { assetId, time, duration, label }: { assetId: string; time: number | null; duration: number; label: string } = $props();

  let manifest = $state<TrickplayManifest | null>(null);
  let unavailable = $state(false), failed = $state(false);

  $effect(() => {
    const id = assetId;
    manifest = null; unavailable = false; failed = false;
    const controller = new AbortController();
    let retries = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = async () => {
      try {
        const response = await fetch('/api/v1/assets/' + encodeURIComponent(id) + '/trickplay', { credentials: 'same-origin', signal: controller.signal });
        const value = await response.json().catch(() => null);
        if (controller.signal.aborted) return;
        const ready = readyTrickplay(response.ok, value, id);
        if (ready) { manifest = ready; return; }
        // Still being prepared: ask again a few times, then say there is no picture.
        if (response.status === 202 && value?.status === 'generating' && retries++ < 8) timer = setTimeout(() => void check(), 4000);
        else unavailable = true;
      } catch { if (!controller.signal.aborted) unavailable = true; }
    };
    void check();
    return () => { controller.abort(); if (timer) clearTimeout(timer); };
  });

  const frame = $derived(failed ? null : computePreviewFrame(time, duration, manifest));
</script>

<figure class="frame">
  <div class="picture" style:aspect-ratio={frame ? `${frame.width} / ${frame.height}` : '16 / 9'}>
    {#if frame}
      <img src={frame.url} alt="" draggable="false"
        style:width={`${(frame.fullWidth / frame.width) * 100}%`}
        style:height={`${(frame.fullHeight / frame.height) * 100}%`}
        style:left={`${(-frame.x / frame.width) * 100}%`}
        style:top={`${(-frame.y / frame.height) * 100}%`}
        onerror={() => (failed = true)} />
    {:else}
      <span class="none">{unavailable || failed ? $t('segment_preview_unavailable') : time === null ? '—' : $t('segment_preview_loading')}</span>
    {/if}
  </div>
  <figcaption><strong>{label}</strong> {formatClock(time, true)}</figcaption>
</figure>

<style>
  .frame { display: flex; flex-direction: column; gap: 4px; width: min(220px, 100%); margin: 0; }
  .picture { position: relative; overflow: hidden; border-radius: var(--r-sm); background: var(--pn-3); }
  .picture img { position: absolute; display: block; max-width: none; max-height: none; }
  .none { position: absolute; inset: 0; display: grid; place-items: center; padding: 6px; font-size: 12px; text-align: center; color: var(--mu); }
  figcaption { font-size: 12px; color: var(--mu); font-variant-numeric: tabular-nums; }
  figcaption strong { color: var(--tx); }
</style>

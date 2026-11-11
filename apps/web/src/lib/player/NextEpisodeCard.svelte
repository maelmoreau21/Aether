<script lang="ts">
  import Icon from '../ui/Icon.svelte';
  import type { Media } from './types';
  import { type EventKind } from '../statistics/collector';
  import { t } from '../i18n';

  let { visible, nextImage, nextImageFailed = $bindable(), nextLabel, nextTitle, autoplayNext, onNext, postCreditsScene, active, seek, wake, onDismiss }: { visible: boolean; nextImage: string; nextImageFailed: string; nextLabel: string; nextTitle: string | undefined; autoplayNext: boolean; onNext: (() => void) | undefined; postCreditsScene: boolean; active: Media | undefined; seek: (value: number, kind?: EventKind) => void; wake: () => void; onDismiss: () => void } = $props();
</script>

<aside class="next-card" class:raised={visible} aria-label={$t('next_episode')}>
  <div class="next-thumb">
    {#if nextImage && nextImageFailed !== nextImage}
      <img src={nextImage} alt="" width="640" height="360" decoding="async" onerror={() => nextImageFailed = nextImage} />
    {:else}
      <Icon name="tv" size={28} />
    {/if}
  </div>
  <div class="next-info">
    <span class="next-kicker">{$t('up_next_label')}{nextLabel ? ` · ${nextLabel}` : ''}</span>
    <strong title={nextTitle}>{nextTitle}</strong>
    {#if autoplayNext}<small>{$t('autoplay_at_the_end')}</small>{/if}
    <div class="next-actions">
      <button type="button" class="next-play" onclick={() => onNext?.()}><Icon name="play" size={16} /><span>{$t('play_now')}</span></button>
      {#if postCreditsScene && active?.credits}
        {@const creditsEnd = active.credits.end}
        <button type="button" class="next-secondary" onclick={() => { seek(creditsEnd, 'skip_credits'); wake(); }}>{$t('skip_credits')}</button>
      {/if}
    </div>
  </div>
  <button type="button" class="next-dismiss" aria-label={$t('hide_the_next_episode')} title={$t('hide')} onclick={onDismiss}><Icon name="close" size={16} /></button>
</aside>

<style>

  /* Carte « À suivre » : panneau sombre du lecteur, en bas à droite, au-dessus des commandes quand elles sont visibles */
  .next-card {
    position: absolute;
    z-index: 6;
    right: max(var(--gutter, 40px), env(safe-area-inset-right));
    bottom: max(28px, env(safe-area-inset-bottom));
    display: flex;
    gap: 14px;
    align-items: stretch;
    width: min(440px, calc(100% - 32px));
    padding: 12px;
    border-radius: 24px;
    background: rgb(17 21 31 / 0.92);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
    color: #f4f6fb;
    font-family: var(--font);
    animation: next-in var(--t-slow) var(--ease);
    transition: bottom var(--t-slow) var(--ease);
  }

  /* Hauteur des commandes (PlayerControls) : barre 28 px + rangée 60 px + marges. */
  .next-card.raised { bottom: calc(max(20px, env(safe-area-inset-bottom)) + 108px); }

  @media (min-width: 1920px) {
  .next-card.raised { bottom: calc(max(32px, env(safe-area-inset-bottom)) + 124px); }
}

  .next-thumb {
    flex: none;
    display: grid;
    place-items: center;
    width: 160px;
    aspect-ratio: 16 / 9;
    overflow: hidden;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.08);
    color: #a2a9ba;
  }

  .next-thumb img { width: 100%; height: 100%; object-fit: cover; }

  .next-info { display: flex; flex-direction: column; gap: 4px; min-width: 0; padding: 2px 28px 2px 0; }

  .next-kicker { font-size: 11.5px; font-weight: 800; letter-spacing: 0.06em; text-transform: uppercase; color: rgb(244 246 251 / 0.65); }

  .next-info strong { font-size: 16px; font-weight: 800; line-height: 1.25; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }

  .next-info small { font-size: 12px; color: rgb(244 246 251 / 0.6); }

  .next-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: auto; padding-top: 6px; }

  .next-play,
  .next-secondary {
    display: inline-flex; align-items: center; gap: 8px;
    min-height: 40px; padding: 0 16px; border: 0; border-radius: 999px;
    font: inherit; font-size: 13.5px; font-weight: 800; cursor: pointer;
  }

  .next-play { background: rgb(248 249 252 / 0.95); color: #0b0e18; }

  .next-play:hover { background: #fff; }

  .next-play :global(svg) { fill: currentColor; }

  .next-secondary { background: rgb(255 255 255 / 0.12); color: #f4f6fb; }

  .next-secondary:hover { background: rgb(255 255 255 / 0.2); }

  .next-dismiss {
    position: absolute; top: 8px; right: 8px;
    display: grid; place-items: center;
    width: 32px; height: 32px; min-height: 32px; padding: 0;
    border: 0; border-radius: 50%;
    background: transparent; color: rgb(244 246 251 / 0.7); cursor: pointer;
  }

  .next-dismiss:hover { background: rgb(255 255 255 / 0.12); color: #fff; }

  .next-card button:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }

  @media (max-width: 768px) {
  .next-card { right: 12px; left: 12px; width: auto; padding: 10px; gap: 10px; border-radius: 20px; }
  .next-card.raised { bottom: calc(max(20px, env(safe-area-inset-bottom)) + 96px); }
  .next-thumb { width: 112px; }
  .next-info strong { font-size: 14px; }
}

  @media (prefers-reduced-motion: reduce) {
  .next-card { animation: none; transition: none; }
}
</style>

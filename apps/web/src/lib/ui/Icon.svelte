<script lang="ts">
  export type IconName = 'play' | 'pause' | 'volume' | 'arrow' | 'check' | 'globe' | 'captions' | 'film' | 'chevron' | 'alert' | 'back' | 'fullscreen' | 'minimize' | 'muted' | 'settings';
  let { name, size = 20 }: { name: IconName; size?: number } = $props();
  const paths: Record<IconName, string> = {
    back: 'M20 12H4m6-6-6 6 6 6', fullscreen: 'M8 3H3v5m13-5h5v5M3 16v5h5m8 0h5v-5', minimize: 'M3 8h5V3m8 0v5h5M8 21v-5H3m18 0h-5v5', muted: 'm11 4-6 5H2v6h3l6 5V4Zm5 5 6 6m0-6-6 6', settings: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM12 2v3m0 14v3M2 12h3m14 0h3M5 5l2 2m10 10 2 2M5 19l2-2M17 7l2-2',
    play: 'm9 5 11 7-11 7V5Z', pause: 'M8 5v14M16 5v14',
    volume: 'm11 4-6 5H2v6h3l6 5V4ZM15 8a6 6 0 0 1 0 8M18 5a10 10 0 0 1 0 14',
    arrow: 'M4 12h16m-6-6 6 6-6 6', check: 'm5 12 4 4L19 6',
    globe: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM3 12h18M12 3c5 5 5 13 0 18-5-5-5-13 0-18Z',
    captions: 'M4 5h16v14H4V5Zm6 5a2 2 0 1 0 0 4m7-4a2 2 0 1 0 0 4',
    film: 'M3 4h18v16H3V4Zm4 0v16M17 4v16M3 9h4m10 0h4M3 15h4m10 0h4',
    chevron: 'm9 5 7 7-7 7', alert: 'M12 3 2 21h20L12 3Zm0 6v5m0 3v1'
  };
</script>
<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>

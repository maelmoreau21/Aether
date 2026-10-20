<script module lang="ts">
  export type IconName =
    | 'play'
    | 'pause'
    | 'volume'
    | 'arrow'
    | 'check'
    | 'globe'
    | 'captions'
    | 'film'
    | 'chevron'
    | 'alert'
    | 'back'
    | 'fullscreen'
    | 'minimize'
    | 'muted'
    | 'settings'
    | 'search'
    | 'profile'
    | 'info'
    | 'star'
    | 'clock'
    | 'eye'
    | 'eye-off'
    | 'filter'
    | 'sort'
    | 'sparkles'
    | 'tv'
    | 'refresh'
    | 'chevron-left'
    | 'chevron-right'
    | 'server'
    | 'users'
    | 'shield'
    | 'sun'
    | 'moon';

  const paths: Record<IconName, string> = {
    sun: 'M12 3v2m0 14v2M5.64 5.64l1.42 1.42m9.88 9.88 1.42 1.42M3 12h2m14 0h2M5.64 18.36l1.42-1.42m9.88-9.88 1.42-1.42M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
    moon: 'M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79Z',
    search: 'M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z',
    profile: 'M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0ZM4 21v-2a8 8 0 0 1 16 0v2',
    back: 'M20 12H4m6-6-6 6 6 6',
    fullscreen: 'M8 3H3v5m13-5h5v5M3 16v5h5m8 0h5v-5',
    minimize: 'M3 8h5V3m8 0v5h5M8 21v-5H3m18 0h-5v5',
    muted: 'm11 4-6 5H2v6h3l6 5V4Zm5 5 6 6m0-6-6 6',
    settings: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM12 2v3m0 14v3M2 12h3m14 0h3M5 5l2 2m10 10 2 2M5 19l2-2M17 7l2-2',
    play: 'm9 5 11 7-11 7V5Z',
    pause: 'M8 5v14M16 5v14',
    volume: 'm11 4-6 5H2v6h3l6 5V4ZM15 8a6 6 0 0 1 0 8M18 5a10 10 0 0 1 0 14',
    arrow: 'M4 12h16m-6-6 6 6-6 6',
    check: 'm5 12 4 4L19 6',
    globe: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM3 12h18M12 3c5 5 5 13 0 18-5-5-5-13 0-18Z',
    captions: 'M4 5h16v14H4V5Zm6 5a2 2 0 1 0 0 4m7-4a2 2 0 1 0 0 4',
    film: 'M3 4h18v16H3V4Zm4 0v16M17 4v16M3 9h4m10 0h4M3 15h4m10 0h4',
    chevron: 'm9 5 7 7-7 7',
    alert: 'M12 3 2 21h20L12 3Zm0 6v5m0 3v1',
    info: 'M12 16v-4m0-4h.01M22 12c0 5.523-4.477 10-10 10S2 17.523 2 12 6.477 2 12 2s10 4.477 10 10Z',
    star: 'M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2Z',
    clock: 'M12 6v6l4 2m6-2a10 10 0 1 1-20 0 10 10 0 0 1 20 0Z',
    eye: 'M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Zm10 3a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z',
    'eye-off': 'M9.88 9.88a3 3 0 1 0 4.24 4.24M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61M2 2l20 20',
    filter: 'M3 4h18M6 9h12M9 14h6M11 19h2',
    sort: 'm3 8 4-4 4 4M7 4v16m14-4-4 4-4-4m4 4V4',
    sparkles: 'm12 3 1.9 5.7a1 1 0 0 0 .6.6L20.2 11.2a.5.5 0 0 1 0 .9l-5.7 1.9a1 1 0 0 0-.6.6L12 20.3a.5.5 0 0 1-.9 0l-1.9-5.7a1 1 0 0 0-.6-.6L2.9 12.1a.5.5 0 0 1 0-.9l5.7-1.9a1 1 0 0 0 .6-.6L11.1 3a.5.5 0 0 1 .9 0Z',
    tv: 'M20 7H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2Zm-4-4-4 4-4-4',
    refresh: 'M3 12a9 9 0 0 1 15-6.7L21 8M21 3v5h-5M21 12a9 9 0 0 1-15 6.7L3 16M3 21v-5h5',
    'chevron-left': 'm15 18-6-6 6-6',
    'chevron-right': 'm9 18 6-6-6-6',
    server: 'M2 4h20v6H2V4Zm0 10h20v6H2v-6ZM6 7h.01M6 17h.01',
    users: 'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M9 7a4 4 0 1 0 0-8 4 4 0 0 0 0 8Zm13 14v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75',
    shield: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z'
  };
</script>
<script lang="ts">
  let { name, size = 20 }: { name: IconName; size?: number } = $props();
</script>
<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>

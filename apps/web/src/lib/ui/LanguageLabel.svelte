<script lang="ts">
  import { languageLabel } from '../player/track-labels';
  let { language }: {language:string} = $props();
  const label=$derived(languageLabel(language));
  const flags:Record<string,string[]>={
    français:['#002395','#fff','#ED2939'],italien:['#009246','#fff','#CE2B37'],
    espagnol:['#AA151B','#F1BF00','#AA151B'],néerlandais:['#AE1C28','#fff','#21468B'],
    allemand:['#111','#DD0000','#FFCE00'],russe:['#fff','#0039A6','#D52B1E'],
    hindi:['#FF9933','#fff','#138808']
  };
  const colors=$derived(flags[label]);
</script>
<span class="language-label">
  {#if colors}
    <svg viewBox="0 0 18 12" aria-hidden="true">
      {#if label==='français' || label==='italien'}
        <path fill={colors[0]} d="M0 0h6v12H0z"/><path fill={colors[1]} d="M6 0h6v12H6z"/><path fill={colors[2]} d="M12 0h6v12h-6z"/>
      {:else if label==='espagnol'}
        <path fill={colors[0]} d="M0 0h18v12H0z"/><path fill={colors[1]} d="M0 3h18v6H0z"/>
      {:else}
        <path fill={colors[0]} d="M0 0h18v4H0z"/><path fill={colors[1]} d="M0 4h18v4H0z"/><path fill={colors[2]} d="M0 8h18v4H0z"/>
        {#if label==='hindi'}<circle cx="9" cy="6" r="1.4" fill="none" stroke="#000080" stroke-width=".6"/>{/if}
      {/if}
    </svg>
  {:else if label==='anglais'}
    <svg viewBox="0 0 18 12" aria-hidden="true"><path fill="#012169" d="M0 0h18v12H0z"/><path d="m0 0 18 12m0-12L0 12" stroke="#fff" stroke-width="3"/><path d="m0 0 18 12m0-12L0 12" stroke="#C8102E" stroke-width="1"/><path d="M9 0v12M0 6h18" stroke="#fff" stroke-width="4"/><path d="M9 0v12M0 6h18" stroke="#C8102E" stroke-width="2"/></svg>
  {:else if label==='chinois'}
    <svg viewBox="0 0 18 12" aria-hidden="true"><path fill="#DE2910" d="M0 0h18v12H0z"/><path fill="#FFDE00" d="m4 1 1 2h2l-1.7 1.3.7 2L4 5 2 6.3l.7-2L1 3h2z"/></svg>
  {:else if label==='japonais'}
    <svg viewBox="0 0 18 12" aria-hidden="true"><path fill="#fff" d="M0 0h18v12H0z"/><circle cx="9" cy="6" r="3" fill="#BC002D"/></svg>
  {/if}
  {label}
</span>
<style>.language-label {display:inline-flex;align-items:center;gap:6px;} svg {width:18px;height:12px;flex-shrink:0;box-shadow:0 0 0 1px var(--border-subtle);border-radius:1px;overflow:hidden;}</style>

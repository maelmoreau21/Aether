<script lang="ts">
  import { languageLabel } from '../player/track-labels';
  let { language, flagOnly = false }: {language:string;flagOnly?:boolean} = $props();
  const label=$derived(languageLabel(language));
  // Flags follow the language code, never its displayed name: the name changes with the interface language.
  const aliases:Record<string,string>={fra:'fr',fre:'fr',eng:'en',spa:'es',ger:'de',deu:'de',ita:'it',jpn:'ja',zho:'zh',chi:'zh',nld:'nl',dut:'nl',hin:'hi',rus:'ru'};
  const code=$derived.by(()=>{const base=(language??'').toLowerCase().replaceAll('_','-').split('-')[0];return aliases[base]??base;});
  const flags:Record<string,string[]>={
    fr:['#002395','#fff','#ED2939'],it:['#009246','#fff','#CE2B37'],
    es:['#AA151B','#F1BF00','#AA151B'],nl:['#AE1C28','#fff','#21468B'],
    de:['#111','#DD0000','#FFCE00'],ru:['#fff','#0039A6','#D52B1E'],
    hi:['#FF9933','#fff','#138808']
  };
  const colors=$derived(flags[code]);
</script>
<span class="language-label" class:flag-only={flagOnly}>
  {#if colors}
    <svg viewBox="0 0 18 12" aria-hidden="true">
      {#if code==='fr' || code==='it'}
        <path fill={colors[0]} d="M0 0h6v12H0z"/><path fill={colors[1]} d="M6 0h6v12H6z"/><path fill={colors[2]} d="M12 0h6v12h-6z"/>
      {:else if code==='es'}
        <path fill={colors[0]} d="M0 0h18v12H0z"/><path fill={colors[1]} d="M0 3h18v6H0z"/>
      {:else}
        <path fill={colors[0]} d="M0 0h18v4H0z"/><path fill={colors[1]} d="M0 4h18v4H0z"/><path fill={colors[2]} d="M0 8h18v4H0z"/>
        {#if code==='hi'}<circle cx="9" cy="6" r="1.4" fill="none" stroke="#000080" stroke-width=".6"/>{/if}
      {/if}
    </svg>
  {:else if code==='en'}
    <svg viewBox="0 0 18 12" aria-hidden="true"><path fill="#012169" d="M0 0h18v12H0z"/><path d="m0 0 18 12m0-12L0 12" stroke="#fff" stroke-width="3"/><path d="m0 0 18 12m0-12L0 12" stroke="#C8102E" stroke-width="1"/><path d="M9 0v12M0 6h18" stroke="#fff" stroke-width="4"/><path d="M9 0v12M0 6h18" stroke="#C8102E" stroke-width="2"/></svg>
  {:else if code==='zh'}
    <svg viewBox="0 0 18 12" aria-hidden="true"><path fill="#DE2910" d="M0 0h18v12H0z"/><path fill="#FFDE00" d="m4 1 1 2h2l-1.7 1.3.7 2L4 5 2 6.3l.7-2L1 3h2z"/></svg>
  {:else if code==='ja'}
    <svg viewBox="0 0 18 12" aria-hidden="true"><path fill="#fff" d="M0 0h18v12H0z"/><circle cx="9" cy="6" r="3" fill="#BC002D"/></svg>
  {/if}
  {#if !flagOnly}{label}{/if}
</span>
<style>
  .language-label { display: inline-flex; align-items: center; gap: 6px; }
  .flag-only svg { width: 24px; height: 16px; }
  svg { width: 18px; height: 12px; flex-shrink: 0; box-shadow: 0 0 0 1px var(--line-2); border-radius: 2px; overflow: hidden; }
</style>

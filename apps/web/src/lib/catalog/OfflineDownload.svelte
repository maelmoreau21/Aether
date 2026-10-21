<script lang="ts">
  let { url, title, mime, options = [] }: { url:string; title:string; mime:string; options?:{url:string;label:string;mime:string}[] } = $props();
  let choosing = $state(false);
  function filename(sourceMime:string) {
    const extension:Record<string,string> = { 'video/mp4':'mp4','video/x-matroska':'mkv','video/webm':'webm','audio/ogg':'ogg','audio/opus':'opus','audio/mpeg':'mp3','audio/mp4':'m4a','audio/flac':'flac','audio/wav':'wav','audio/x-wav':'wav' };
    const name = Array.from(title,character=>character.charCodeAt(0)<32||character.charCodeAt(0)===127||'<>:"/\\|?*'.includes(character)?'_':character).join('').replace(/[. ]+$/g,'').slice(0,180) || 'media';
    const suffix = extension[sourceMime.split(';')[0].trim().toLowerCase()];
    return suffix && !name.toLowerCase().endsWith('.'+suffix) ? `${name}.${suffix}` : name;
  }
</script>
{#snippet icon()}<svg width="21" height="21" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5" /></svg>{/snippet}
<div class="download-action">
  {#if options.length > 1}
    <button type="button" class="download-icon" aria-label="Télécharger" title="Télécharger le fichier" aria-expanded={choosing} onclick={() => choosing = !choosing}>{@render icon()}</button>
  {:else}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- Protected same-origin content URL; download is a file, not a SvelteKit route. -->
    <a class="download-icon" href={url} download={filename(mime)} aria-label="Télécharger" title="Télécharger le fichier">{@render icon()}</a>
  {/if}
  {#if choosing}<div class="download-choices"><strong>Version à télécharger</strong>{#each options as source(source.url)}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- Protected same-origin content URL; native file download. -->
    <a href={source.url} download={filename(source.mime)}>{source.label}</a>
  {/each}<button type="button" onclick={() => choosing = false}>Fermer</button></div>{/if}
</div>
<style>
.download-action { display:flex;align-items:center;gap:10px;flex-wrap:wrap;position:relative;margin-block:12px; }
button,a { color:var(--color-text);background:var(--color-surface);border:1px solid var(--color-line);border-radius:var(--radius-sm);cursor:pointer;font:inherit;text-decoration:none; }
.download-icon { width:44px;height:44px;display:grid;place-items:center;padding:0; }
.download-icon:hover { color:var(--color-accent);border-color:var(--color-accent); }
button:focus-visible,a:focus-visible { outline:2px solid var(--color-accent);outline-offset:3px; }
.download-choices button,.download-choices a { padding:10px; }
.download-choices { display:grid;gap:8px;padding:14px;background:var(--color-surface);border:1px solid var(--color-line);border-radius:var(--radius-md); }
</style>

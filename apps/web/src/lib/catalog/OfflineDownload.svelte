<script lang="ts">
  import { session } from '../auth/session';
  import { downloadUrl } from '../download-url';
  let { url, title, mime, options = [] }: { url:string; title:string; mime:string; options?:{url:string;label:string;mime:string}[] } = $props();
  let choosing = $state(false);
  let allowed = $state(false);
  $effect(() => {
    const actor = $session?.user.id, controller = new AbortController(); allowed = false; choosing = false;
    if (actor) fetch('/api/v1/me/media-policy', { signal: controller.signal, cache: 'no-store' })
      .then(async response => response.ok ? response.json() : null)
      .then(policy => { if (!controller.signal.aborted) allowed = policy?.allowMediaDownload === true; })
      .catch(() => {});
    return () => controller.abort();
  });
  function filename(sourceMime:string) {
    const extension:Record<string,string> = { 'video/mp4':'mp4','video/x-matroska':'mkv','video/webm':'webm','audio/ogg':'ogg','audio/opus':'opus','audio/mpeg':'mp3','audio/mp4':'m4a','audio/flac':'flac','audio/wav':'wav','audio/x-wav':'wav' };
    const name = Array.from(title,character=>character.charCodeAt(0)<32||character.charCodeAt(0)===127||'<>:"/\\|?*'.includes(character)?'_':character).join('').replace(/[. ]+$/g,'').slice(0,180) || 'media';
    const suffix = extension[sourceMime.split(';')[0].trim().toLowerCase()];
    return suffix && !name.toLowerCase().endsWith('.'+suffix) ? `${name}.${suffix}` : name;
  }
</script>
{#snippet icon()}<svg width="21" height="21" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5" /></svg>{/snippet}
{#if allowed && downloadUrl(url,mime)}<div class="download-action">
  {#if options.length > 1}
    <button type="button" class="download-icon" aria-label="Télécharger" title="Télécharger le fichier" aria-expanded={choosing} onclick={() => choosing = !choosing}>{@render icon()}</button>
  {:else}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- Protected same-origin content URL; download is a file, not a SvelteKit route. -->
    <a class="download-icon" href={downloadUrl(url,mime)!} download={filename(mime)} aria-label="Télécharger" title="Télécharger le fichier">{@render icon()}</a>
  {/if}
  {#if choosing}<div class="download-choices"><strong>Version à télécharger</strong>{#each options as source(source.url)}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- Protected same-origin content URL; native file download. -->
    {#if downloadUrl(source.url,source.mime)}<a href={downloadUrl(source.url,source.mime)!} download={filename(source.mime)}>{source.label}</a>{/if}
  {/each}<button type="button" onclick={() => choosing = false}>Fermer</button></div>{/if}
</div>{/if}
<style>
  .download-action {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    position: relative;
    z-index: 40;
  }
  button, a {
    color: var(--tx);
    background: transparent;
    border: 1px solid transparent;
    cursor: pointer;
    font: inherit;
    text-decoration: none;
    transition: background-color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  .download-icon {
    width: 44px;
    height: 44px;
    min-height: 44px;
    box-sizing: border-box;
    display: grid;
    place-items: center;
    padding: 0;
    border-radius: 14px;
    background: var(--pn);
  }
  .download-icon:hover {
    background: var(--pn-2);
  }
  .download-icon:active {
    transform: scale(0.98);
  }
  .download-icon[aria-expanded="true"] {
    background: var(--pn-3);
  }
  button:focus-visible, a:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }
  .download-choices {
    position: absolute;
    top: calc(100% + 8px);
    left: 0;
    min-width: 240px;
    max-width: min(320px, calc(100vw - 32px));
    z-index: 300;
    display: grid;
    gap: 2px;
    padding: 6px;
    background: var(--bg-3);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    box-shadow: var(--shadow);
  }
  .download-choices strong {
    padding: 8px 12px 4px;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--mu);
  }
  .download-choices button, .download-choices a {
    display: flex;
    align-items: center;
    min-height: 42px;
    padding: 0 12px;
    border-radius: 10px;
    font-size: 14px;
    font-weight: 600;
    text-align: left;
    overflow-wrap: anywhere;
  }
  .download-choices button {
    margin-top: 4px;
    border-top-color: var(--line);
    border-radius: 0 0 10px 10px;
    color: var(--mu);
  }
  .download-choices button:hover, .download-choices a:hover {
    background: var(--pn);
    color: var(--tx);
  }
  .download-choices button:focus-visible, .download-choices a:focus-visible {
    outline-offset: -2px;
  }
  @media (max-width: 720px) {
    .download-choices { left: auto; right: 0; }
  }
</style>

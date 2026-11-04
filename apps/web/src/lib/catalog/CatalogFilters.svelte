<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { resolve } from '$app/paths';
</script>
<details><summary>Filtrer le catalogue</summary><form method="GET" action={page.url.pathname} onsubmit={event=>{event.preventDefault();const params=new URLSearchParams();for(const [key,value] of new FormData(event.currentTarget)){if(typeof value==='string'&&value.trim())params.set(key,value.trim());}void goto(resolve(`${page.url.pathname.replace(/^\/app\/?/, '')}?${params}` as `films/?${string}`));}}>
{#if page.url.searchParams.get('library')}<input type="hidden" name="library" value={page.url.searchParams.get('library')??''} />{/if}
{#if page.url.searchParams.get('q')}<input type="hidden" name="q" value={page.url.searchParams.get('q')??''} />{/if}
<label>Genre<input name="genre" value={page.url.searchParams.get('genre')??''} maxlength="100" placeholder="Animation" /></label>
<label>Année<input type="number" name="year" value={page.url.searchParams.get('year')??''} min="1800" max="2200" /></label>
<label>Qualité minimum<select name="quality" value={page.url.searchParams.get('quality')??''}><option value="">Toutes</option><option value="720">720p</option><option value="1080">1080p</option><option value="2160">2160p</option></select></label>
<label>Codec<select name="codec" value={page.url.searchParams.get('codec')??''}><option value="">Tous</option><option value="av1">AV1</option><option value="hevc">HEVC</option><option value="h264">H.264</option><option value="opus">Opus</option></select></label>
<label>Langue de piste<input name="language" value={page.url.searchParams.get('language')??''} maxlength="35" placeholder="fra, eng, fr…" /></label><button>Appliquer</button><a href={resolve(page.url.pathname.replace(/^\/app\/?/, '') as 'films/' | 'series/' | 'search/' | '/')}>Effacer</a></form></details>
<style>
  details { margin: 20px 0; }
  summary { display: inline-flex; align-items: center; min-height: 44px; padding: 0 16px; border-radius: var(--r-pill); background: var(--pn); color: var(--tx); font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease; }
  summary:hover { background: var(--pn-2); }
  details[open] summary { background: var(--ac-soft); color: var(--ac); }
  summary:focus-visible, a:focus-visible, button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  form { display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: 14px; align-items: end; margin-top: 12px; padding: 20px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  input, select { width: 100%; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font: inherit; font-size: 14px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input:hover, select:hover { background-color: var(--pn-2); }
  input:focus, select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  button, a { display: inline-flex; align-items: center; justify-content: center; height: 46px; padding: 0 18px; border: 0; border-radius: var(--r-md); font-size: 14px; font-weight: 700; }
  button { background: var(--btn); color: var(--btn-tx); cursor: pointer; transition: opacity var(--t-fast) ease; }
  button:hover { opacity: 0.9; }
  a { background: transparent; color: var(--mu); transition: background-color var(--t-fast) ease, color var(--t-fast) ease; }
  a:hover { background: var(--pn); color: var(--tx); }
</style>

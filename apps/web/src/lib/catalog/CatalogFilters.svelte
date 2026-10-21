<script lang="ts">
  import { page } from '$app/state';
  import { resolve } from '$app/paths';
</script>
<details><summary>Filtrer le catalogue</summary><form method="GET" action={page.url.pathname} onsubmit={event=>{event.preventDefault();const params=new URLSearchParams();for(const [key,value] of new FormData(event.currentTarget)){if(typeof value==='string'&&value.trim())params.set(key,value.trim());}window.location.assign(page.url.pathname+'?'+params);}}>
{#if page.url.searchParams.get('library')}<input type="hidden" name="library" value={page.url.searchParams.get('library')??''} />{/if}
{#if page.url.searchParams.get('q')}<input type="hidden" name="q" value={page.url.searchParams.get('q')??''} />{/if}
<label>Genre<input name="genre" value={page.url.searchParams.get('genre')??''} maxlength="100" placeholder="Animation" /></label>
<label>Année<input type="number" name="year" value={page.url.searchParams.get('year')??''} min="1800" max="2200" /></label>
<label>Qualité minimum<select name="quality" value={page.url.searchParams.get('quality')??''}><option value="">Toutes</option><option value="720">720p</option><option value="1080">1080p</option><option value="2160">2160p</option></select></label>
<label>Codec<select name="codec" value={page.url.searchParams.get('codec')??''}><option value="">Tous</option><option value="av1">AV1</option><option value="hevc">HEVC</option><option value="h264">H.264</option><option value="opus">Opus</option></select></label>
<label>Langue de piste<input name="language" value={page.url.searchParams.get('language')??''} maxlength="35" placeholder="fra, eng, fr…" /></label><button>Appliquer</button><a href={resolve(page.url.pathname.replace(/^\/app\/?/, '') as 'films/' | 'series/' | 'search/' | '/')}>Effacer</a></form></details>
<style>details { margin:20px 5%; } summary { cursor:pointer; } form { display:flex;gap:12px;flex-wrap:wrap;align-items:end;padding:16px 0; } label { display:grid;gap:6px; } input,select,button { padding:10px;background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius:8px;max-width:180px; } a { color:var(--color-accent);padding:10px; } </style>

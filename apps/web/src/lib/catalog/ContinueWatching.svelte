<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { history, loadHistory } from './watch-state';
  let error = $state('');
  onMount(() => { void loadHistory(true).catch(e => error = e.message); });
  $effect(() => { if ($session) void loadHistory().catch(e => error = e.message); });
  const continuing = $derived($history.filter(item => !item.watched && item.position > 0));
</script>
<section aria-label="Continuer à regarder">
  <h2>Continuer à regarder</h2>
  {#if error}<p role="alert">{error}</p><button onclick={() => { error=''; void loadHistory(true).catch(e=>error=e.message); }}>Réessayer</button>
  {:else if !continuing.length}<p>Vos séances en cours apparaîtront ici.</p>
  {:else}<div class="continue-list">{#each continuing as item (item.workId)}
    <a href={resolve(`${item.kind === 'episode' ? 'series/' : 'films/'}?work=${encodeURIComponent(item.seriesId ?? item.workId)}`)}>
      <strong>{item.title}</strong><span>{Math.floor(item.position / 60)} / {Math.ceil(item.duration / 60)} min</span>
      <progress value={item.position} max={item.duration} aria-label={`Progression de ${item.title}`}></progress>
    </a>
  {/each}</div>{/if}
  {#if $history.length}<details><summary>Historique récent</summary><ul>{#each $history.slice(0, 20) as item (item.workId)}
    <li><a href={resolve(`${item.kind === 'episode' ? 'series/' : 'films/'}?work=${encodeURIComponent(item.seriesId ?? item.workId)}`)}>{item.title}</a> · {item.watched ? 'Vu' : 'En cours'}</li>
  {/each}</ul></details>{/if}
</section>
<style>
  section { margin:32px 0; } h2 { font-size:22px; } p { color:#a9a1b5; font-size:14px; }
  .continue-list { display:flex; gap:16px; overflow:auto; padding:10px 0; } a { flex:0 0 230px; display:grid; gap:12px; padding:18px; background:#ffffff08; border:1px solid #ffffff15; border-radius:8px; color:inherit; text-decoration:none; }
  span { font-size:12px; color:#bcb3cb; } progress { width:100%; height:5px; accent-color:#b69ae3; }
  details { margin-top:20px; font-size:14px; } summary { cursor:pointer; padding:12px 0; } li { margin:10px 0; color:#bcb3cb; } li a { display:inline; padding:0; background:none; border:0; color:inherit; }
</style>

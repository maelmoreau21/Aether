<script lang="ts">
  import {playlistRequest,type Playlist} from './playlists';
  let {id,kind='music'}:{id:string;kind?:'music'|'work'}=$props();
  let lists=$state<Playlist[]>([]);let notice=$state('');let busy=$state(false);
  async function load(){try{lists=(await playlistRequest<{lists:Playlist[]}>('/api/v1/lists')).lists.filter(list=>list.owned);}catch(error){notice=(error as Error).message;}}
  async function add(list:Playlist){busy=true;notice='';try{await playlistRequest(`/api/v1/${kind==='music'?'playlists':'lists'}/${encodeURIComponent(list.id)}/${kind==='music'?'music':'items'}/${encodeURIComponent(id)}`,'PUT');notice=`Ajouté à « ${list.name} ».`;}catch(error){notice=(error as Error).message;}finally{busy=false;}}
</script>
<details ontoggle={event=>{if(event.currentTarget.open)void load();}}><summary>Ajouter à une playlist</summary><div>{#each lists as list(list.id)}<button disabled={busy} onclick={()=>add(list)}>{list.name}</button>{:else}<p>Créez une playlist dans « Mes playlists ».</p>{/each}{#if notice}<p role="status">{notice}</p>{/if}</div></details>
<style>details{position:relative;font-size:13px}summary{cursor:pointer;padding:9px}details>div{position:absolute;right:0;top:100%;min-width:220px;max-height:240px;overflow:auto;background:var(--color-surface);border:1px solid var(--color-line);padding:8px;z-index:8;border-radius:var(--radius-md);box-shadow:0 10px 30px #0004}button{display:block;width:100%;padding:10px;text-align:left;background:transparent;color:var(--color-text);border:0;cursor:pointer}p{padding:8px}</style>

<script lang="ts">
  import {playlistRequest,type Playlist} from './playlists';
  import { t } from '../i18n';
  import { dismissibleDetails } from '../ui/dismissible-details';
  let {id,kind='music',inline=false}:{id:string;kind?:'music'|'work';inline?:boolean}=$props();
  let lists=$state<Playlist[]>([]);let notice=$state('');let busy=$state(false);
  async function load(){try{lists=(await playlistRequest<{lists:Playlist[]}>('/api/v1/lists')).lists.filter(list=>list.owned);}catch(error){notice=(error as Error).message;}}
  async function add(list:Playlist){busy=true;notice='';try{await playlistRequest(`/api/v1/${kind==='music'?'playlists':'lists'}/${encodeURIComponent(list.id)}/${kind==='music'?'music':'items'}/${encodeURIComponent(id)}`,'PUT');notice=$t('added_to_playlist',{name:list.name});}catch(error){notice=(error as Error).message;}finally{busy=false;}}
</script>
<details use:dismissibleDetails ontoggle={event=>{if(event.currentTarget.open)void load();}}><summary>{$t('add_to_playlist')}</summary><div class:inline>{#each lists as list(list.id)}<button disabled={busy} onclick={()=>add(list)}>{list.name}</button>{:else}<p>{$t('create_playlist_hint')}</p>{/each}{#if notice}<p role="status">{notice}</p>{/if}</div></details>
<style>
details{position:relative;font-size:14px}summary{display:flex;align-items:center;cursor:pointer;padding:0 12px;min-height:42px;border-radius:10px;font-weight:600;color:var(--tx);transition:background-color var(--t-fast) var(--ease)}summary:hover{background:var(--pn)}summary:focus-visible{outline:2px solid var(--ac);outline-offset:-2px}details>div{position:absolute;right:0;top:calc(100% + 6px);min-width:240px;max-height:260px;overflow:auto;background:var(--bg-3);border:1px solid var(--line);padding:6px;z-index:8;border-radius:var(--r-md);box-shadow:var(--shadow)}details>div.inline{position:static;min-width:0;max-height:none;box-shadow:none;border:0;border-radius:0;background:transparent;padding:0 0 0 12px}button{display:flex;align-items:center;width:100%;min-height:42px;padding:0 12px;text-align:left;background:transparent;color:var(--tx);border:0;border-radius:10px;font:inherit;font-size:14px;font-weight:600;cursor:pointer;transition:background-color var(--t-fast) var(--ease)}button:hover:not(:disabled){background:var(--pn)}button:focus-visible{outline:2px solid var(--ac);outline-offset:-2px}button:disabled{opacity:.45;cursor:not-allowed}p{margin:0;padding:8px 12px;font-size:13px;color:var(--mu)}
</style>

<script lang="ts">
  import {onMount} from 'svelte';
  import {adminRequest} from './request';
  let allowPublic=$state(false),allowSharing=$state(false),busy=$state(true),error=$state(''),notice=$state('');
  onMount(()=>{void adminRequest<{allowPublic:boolean;allowSharing:boolean}>('/api/v1/admin/playlists/policy').then(policy=>{allowPublic=policy.allowPublic;allowSharing=policy.allowSharing;}).catch(cause=>error=cause.message).finally(()=>busy=false);});
  async function save(event:SubmitEvent){event.preventDefault();busy=true;error='';notice='';try{await adminRequest('/api/v1/admin/playlists/policy',{method:'PUT',body:JSON.stringify({allowPublic,allowSharing})});notice='Autorisations des playlists enregistrées.';}catch(cause){error=(cause as Error).message;}finally{busy=false;}}
</script>
<section aria-labelledby="playlist-policy-heading"><h2 id="playlist-policy-heading">Partage des playlists</h2><p>Les playlists restent réservées aux utilisateurs connectés. Les permissions des bibliothèques continuent de s’appliquer à chaque média.</p><form onsubmit={save}><fieldset disabled={busy}><label><input type="checkbox" bind:checked={allowSharing} /> Autoriser le partage avec des personnes</label><label><input type="checkbox" bind:checked={allowPublic} /> Autoriser les playlists publiques sur le serveur</label><p>Désactiver une option rend les playlists concernées inaccessibles aux autres utilisateurs, sans supprimer leur contenu.</p><button>Enregistrer les autorisations des playlists</button></fieldset></form>{#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}</section>
<style>
section { margin-top: 24px; padding: 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); font-family: var(--font); color: var(--tx); }
  h2 { margin: 0 0 8px; font-size: 17px; font-weight: 700; letter-spacing: 0; }
  p { max-width: 720px; margin: 0; color: var(--mu); font-size: 13px; line-height: 1.5; }
  form { margin-top: 16px; }
  fieldset { display: grid; gap: 0; min-width: 0; margin: 0; padding: 0; border: 0; }
  fieldset:disabled { opacity: 0.7; }
  label { display: flex; align-items: center; gap: 12px; min-height: 60px; border-bottom: 1px solid var(--line); font-size: 14px; font-weight: 700; cursor: pointer; }
  input { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  fieldset p { margin-top: 12px; }
  button {
    justify-self: start;
    height: 48px;
    margin-top: 16px;
    padding: 0 22px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--btn);
    color: var(--btn-tx);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: opacity var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  button:hover:not(:disabled) { opacity: 0.9; }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  p[role='alert'], p[role='status'] { max-width: none; margin-top: 16px; padding: 12px 16px; border-radius: var(--r-md); color: var(--tx); font-size: 14px; }
  p[role='alert'] { background: var(--err-soft); }
  p[role='status'] { background: var(--ok-soft); }
  @media (max-width: 640px) {
    section { padding: 16px; border-radius: var(--r-md); }
    button { width: 100%; justify-self: stretch; }
  }
  @media (prefers-reduced-motion: reduce) {
    button { transition: none; }
  }
</style>

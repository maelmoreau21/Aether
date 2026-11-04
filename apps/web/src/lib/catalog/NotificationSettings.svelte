<script lang="ts">
  import { session } from '../auth/session';
  type Config = { destination:string; smtpHost:string; smtpPort:number; sender:string; recipient:string; username:string; hasSecret?:boolean };
  type Channel = { id:string; name:string; recipientId:string; kind:string; enabled:boolean; config:Config };
  type History = { id:string; eventId:string; channelId:string; status:string; attempt:number; errorCode?:string; occurredAt:number };
  const emptyConfig = ():Config => ({destination:'',smtpHost:'',smtpPort:465,sender:'',recipient:'',username:''});
  const admin=$derived($session?.user.role==='administrator');
  let enabled=$state(false),scans=$state(false),tasks=$state(false),content=$state(false),loaded=$state(false),busy=$state(false),error=$state(''),notice=$state('');
  let channels=$state<Channel[]>([]),history=$state<History[]>([]),users=$state<{id:string;role:string;name?:string}[]>([]);
  let editId=$state(''),name=$state(''),recipientId=$state(''),kind=$state('webhook'),channelEnabled=$state(false),config=$state<Config>(emptyConfig()),secret=$state('');
  async function request<T>(path:string,method='GET',body?:unknown,signal?:AbortSignal):Promise<T> {
    const response=await fetch(path,{method,cache:'no-store',credentials:'same-origin',headers:method==='GET'?{}:{'content-type':'application/json','x-csrf-token':$session?.csrfToken??''},body:body===undefined?undefined:JSON.stringify(body),signal:signal?AbortSignal.any([signal,AbortSignal.timeout(12000)]):AbortSignal.timeout(12000)});
    if(!response.ok) throw new Error(response.status===403?'Action non autorisée.':response.status===400?'Vérifiez les champs, le secret et la destination HTTPS publique.':'Notifications indisponibles.');
    return response.status===204?undefined as T:await response.json() as T;
  }
  $effect(()=>{
    const id=$session?.user.id,controller=new AbortController();loaded=false;
    if(!id)return;
    void request<{enabled:boolean;scans:boolean;tasks:boolean;content?:boolean}>('/api/v1/account/notifications','GET',undefined,controller.signal).then(value=>{
      if(controller.signal.aborted)return;enabled=value.enabled;scans=value.scans;tasks=value.tasks;content=value.content??false;loaded=true;
    }).catch(()=>{if(!controller.signal.aborted)error='Préférences de notification indisponibles.';});
    if(admin)void loadAdmin(controller.signal);
    return ()=>controller.abort();
  });
  async function loadAdmin(signal?:AbortSignal) {
    try {
      const [data,log]=await Promise.all([request<{items:Channel[];users:{id:string;role:string;name?:string}[]}>('/api/v1/admin/notifications/channels','GET',undefined,signal),request<{items:History[]}>('/api/v1/admin/notifications/history','GET',undefined,signal)]);
      if(signal?.aborted)return;channels=data.items;users=data.users;history=log.items;
      if(!recipientId)recipientId=$session?.user.id??'';
    }catch{if(!signal?.aborted)error='Configuration ou historique de notification indisponible.';}
  }
  async function savePreferences(event:SubmitEvent){
    event.preventDefault();busy=true;error='';notice='';
    try{await request('/api/v1/account/notifications','PUT',{enabled,scans,tasks:admin&&tasks,content});notice='Préférences enregistrées.';}catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}
  }
  function edit(channel?:Channel){editId=channel?.id??'';name=channel?.name??'';recipientId=channel?.recipientId??$session?.user.id??'';kind=channel?.kind??'webhook';channelEnabled=channel?.enabled??false;config=channel?{...channel.config}:emptyConfig();secret='';}
  async function saveChannel(event:SubmitEvent){
    event.preventDefault();busy=true;error='';notice='';
    const { hasSecret:_hasSecret,...fields }=config;void _hasSecret;
    try{
      await request(`/api/v1/admin/notifications/channels${editId?`/${encodeURIComponent(editId)}`:''}`,editId?'PUT':'POST',{name,recipientId,kind,enabled:channelEnabled,config:{...fields,...(secret?{secret}:{})}});
      edit();await loadAdmin();notice='Destination enregistrée. Aucun message de test envoyé.';
    }catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}
  }
  async function remove(id:string){busy=true;error='';notice='';try{await request(`/api/v1/admin/notifications/channels/${encodeURIComponent(id)}`,'DELETE');if(editId===id)edit();await loadAdmin();notice='Destination supprimée et file associée retirée.';}catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}}
</script>

<section class="notification-settings" aria-labelledby="notification-heading">
  <h2 id="notification-heading">Notifications</h2>
  <p>Les notifications externes sont facultatives. Un administrateur configure vos destinations. Les scans et nouveaux contenus respectent vos droits de bibliothèque actuels ; les nouveaux contenus respectent aussi votre contrôle parental.</p>
  <form onsubmit={savePreferences}><fieldset disabled={busy||!loaded}>
    <legend>Mes préférences</legend>
    <label class="check"><input type="checkbox" bind:checked={enabled} /> Recevoir les notifications sur mes destinations configurées</label>
    <label class="check"><input type="checkbox" bind:checked={scans} /> Scans terminés de mes bibliothèques autorisées</label>
    <label class="check"><input type="checkbox" bind:checked={content} /> Nouveaux films, séries et épisodes autorisés après activation</label>
    {#if admin}<label class="check"><input type="checkbox" bind:checked={tasks} /> Tâches terminées ou en échec (administration)</label>{/if}
    <button>Enregistrer mes préférences</button>
  </fieldset></form>
  {#if admin}
    <details><summary>Destinations et historique administrateur</summary>
      <p>Adaptateurs email SMTP TLS, ntfy, Gotify et webhook. Un envoi exige une destination activée et le consentement du compte destinataire. Aucun envoi automatique de test. Les événements Node indisponible et erreur fournisseur ne sont pas encore disponibles.</p>
      <p>File persistante, cinq tentatives maximum avec délais croissants, un départ par destination par minute. Historique conservé 30 jours. Une reprise après coupure peut dupliquer un message ; les webhooks portent un identifiant stable.</p>
      <button type="button" disabled={busy} onclick={()=>edit()}>Nouvelle destination</button>
      {#each channels as channel (channel.id)}<div class="channel"><span>{channel.name} · {channel.kind} · {channel.enabled?'Activée':'Désactivée'}</span><button type="button" disabled={busy} onclick={()=>edit(channel)}>Modifier</button><button type="button" disabled={busy} onclick={()=>remove(channel.id)}>Supprimer</button></div>{/each}
      <form onsubmit={saveChannel}><fieldset disabled={busy}><legend>{editId?'Modifier la destination':'Nouvelle destination'}</legend>
        <label>Nom<input bind:value={name} required maxlength="100" /></label>
        <label>Compte destinataire<select bind:value={recipientId} required>{#each users as user (user.id)}<option value={user.id}>{user.name??user.id} · {user.role}</option>{/each}</select></label>
        <label>Adaptateur<select bind:value={kind}><option value="email">Email SMTP TLS</option><option value="ntfy">ntfy</option><option value="gotify">Gotify</option><option value="webhook">Webhook signé</option></select></label>
        {#if kind==='email'}
          <label>Serveur SMTP<input bind:value={config.smtpHost} placeholder="smtp.example.org" required /></label>
          <p>Port 465, TLS avec certificat vérifié. Les relais privés et le SMTP en clair ne sont pas disponibles.</p>
          <label>Adresse expéditeur<input type="email" bind:value={config.sender} required /></label>
          <label>Adresse destinataire<input type="email" bind:value={config.recipient} required /></label>
          <label>Identifiant SMTP (facultatif)<input bind:value={config.username} autocomplete="off" /></label>
        {:else}
          <label>{kind==='ntfy'?'URL HTTPS du sujet ntfy':kind==='gotify'?'URL HTTPS de base Gotify':'URL HTTPS du récepteur'}<input type="url" bind:value={config.destination} required placeholder="https://notifications.example.org" /></label>
          <p>Destination publique HTTPS sur le port 443, sans identifiant ni secret dans l’URL, paramètre de requête ou redirection.</p>
        {/if}
        <label>{kind==='webhook'?'Secret HMAC partagé (32 caractères minimum)':kind==='email'?'Mot de passe SMTP':'Jeton d’accès'}<input type="password" bind:value={secret} autocomplete="new-password" placeholder={config.hasSecret?'Secret conservé si destination identique':''} minlength={kind==='webhook'?32:undefined} /></label>
        <p>Un secret vide conserve le secret existant uniquement si l’adaptateur et la destination restent identiques. Changer de destination exige de saisir à nouveau son secret.</p>
        <label class="check"><input type="checkbox" bind:checked={channelEnabled} /> Activer les futurs envois vers cette destination</label>
        <button>Enregistrer la destination</button>
      </fieldset></form>
      <button type="button" disabled={busy} onclick={()=>loadAdmin()}>Actualiser l’historique</button>
      <ul>{#each history as item (item.id)}<li>{new Date(item.occurredAt*1000).toLocaleString()} · {item.status} · tentative {item.attempt}{item.errorCode?` · ${item.errorCode}`:''}</li>{:else}<li>Aucune notification enregistrée.</li>{/each}</ul>
    </details>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}
</section>
<style>
  .notification-settings { display: grid; gap: 12px; min-width: 0; color: var(--tx); overflow-wrap: anywhere; }
  h2 { margin: 0 0 6px; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  p, li { margin: 0; color: var(--mu); font-size: 14px; line-height: 1.6; }
  form { margin: 0; }
  fieldset { display: grid; gap: 14px; min-width: 0; margin: 8px 0 0; padding: 20px 0 0; border: 0; border-top: 1px solid var(--line); }
  fieldset:disabled { opacity: 0.7; }
  legend { float: left; width: 100%; margin-bottom: 2px; padding: 0; font-size: 17px; font-weight: 700; color: var(--tx); }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; }
  .check { display: flex; align-items: center; gap: 12px; min-height: 44px; font-weight: 600; }
  select, input:not([type='checkbox']) { width: 100%; max-width: 520px; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  select { padding-right: 40px; }
  select:hover, input:not([type='checkbox']):hover { background-color: var(--pn-2); }
  select:focus, input:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  button { justify-self: start; display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 44px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible, input:focus-visible, select:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  [role='alert'] { margin: 0; padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
  fieldset > button:last-child { height: 48px; padding: 0 22px; background: var(--btn); color: var(--btn-tx); }
  fieldset > button:last-child:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  details { margin-top: 8px; border-top: 1px solid var(--line); }
  details[open] { display: grid; gap: 12px; }
  summary { display: flex; align-items: center; min-height: 52px; padding-top: 12px; font-size: 15px; font-weight: 700; cursor: pointer; border-radius: var(--r-xs); }
  summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .channel { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; min-height: 56px; padding: 6px 8px 6px 16px; border-radius: var(--r-md); background: var(--pn); }
  .channel span { flex: 1; min-width: 180px; font-size: 14px; font-weight: 600; color: var(--tx); }
  .channel button { background: transparent; color: var(--mu); }
  .channel button:hover:not(:disabled) { background: var(--pn-2); color: var(--tx); }
  .channel button:last-child { color: var(--err); }
  .channel button:last-child:hover:not(:disabled) { background: var(--err-soft); color: var(--err); }
  ul { display: grid; margin: 0; padding: 0; list-style: none; border-radius: var(--r-md); background: var(--pn); overflow: hidden; }
  li { padding: 12px 16px; border-top: 1px solid var(--line); font-variant-numeric: tabular-nums; }
  li:first-child { border-top: 0; }
  [role='status'] { margin: 0; padding: 12px 14px; border-radius: var(--r-sm); background: var(--ok-soft); color: var(--ok); font-weight: 600; }
</style>

<script lang="ts">
  import { session } from '../auth/session';
  type Config = { destination:string; smtpHost:string; smtpPort:number; sender:string; recipient:string; username:string; hasSecret?:boolean };
  type Channel = { id:string; name:string; recipientId:string; kind:string; enabled:boolean; config:Config };
  type History = { id:string; eventId:string; channelId:string; status:string; attempt:number; errorCode?:string; occurredAt:number };
  const emptyConfig = ():Config => ({destination:'',smtpHost:'',smtpPort:465,sender:'',recipient:'',username:''});
  const admin=$derived($session?.user.role==='administrator');
  let enabled=$state(false),scans=$state(false),tasks=$state(false),loaded=$state(false),busy=$state(false),error=$state(''),notice=$state('');
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
    void request<{enabled:boolean;scans:boolean;tasks:boolean}>('/api/v1/account/notifications','GET',undefined,controller.signal).then(value=>{
      if(controller.signal.aborted)return;enabled=value.enabled;scans=value.scans;tasks=value.tasks;loaded=true;
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
    try{await request('/api/v1/account/notifications','PUT',{enabled,scans,tasks:admin&&tasks});notice='Préférences enregistrées.';}catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}
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
  <p>Les notifications externes sont facultatives. Un administrateur configure vos destinations. Seuls les scans des bibliothèques auxquelles vous avez encore accès peuvent vous être signalés.</p>
  <form onsubmit={savePreferences}><fieldset disabled={busy||!loaded}>
    <legend>Mes préférences</legend>
    <label class="check"><input type="checkbox" bind:checked={enabled} /> Recevoir les notifications sur mes destinations configurées</label>
    <label class="check"><input type="checkbox" bind:checked={scans} /> Scans terminés de mes bibliothèques autorisées</label>
    {#if admin}<label class="check"><input type="checkbox" bind:checked={tasks} /> Tâches terminées ou en échec (administration)</label>{/if}
    <button>Enregistrer mes préférences</button>
  </fieldset></form>
  {#if admin}
    <details><summary>Destinations et historique administrateur</summary>
      <p>Adaptateurs email SMTP TLS, ntfy, Gotify et webhook. Un envoi exige une destination activée et le consentement du compte destinataire. Aucun envoi automatique de test. Les événements Node indisponible, erreur fournisseur et nouveau contenu ne sont pas encore disponibles.</p>
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
  .notification-settings { display:grid;gap:12px; } fieldset { display:grid;gap:12px;border:1px solid var(--color-line);border-radius:var(--radius-md);padding:16px;margin-block:12px; } label { display:grid;gap:6px; } .check { display:flex;align-items:center;gap:10px; } p,li { font-size:13px;line-height:1.5; } .channel { display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin-block:12px; } button,input,select { padding:10px;background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius:var(--radius-md); } summary { cursor:pointer;padding-block:12px; }
</style>

<script lang="ts">
  import { session } from '../auth/session';
  import { features, loadFeatures } from '../applications/api';
  import { t, fmt } from '../i18n';
  type Config = { destination:string; smtpHost:string; smtpPort:number; sender:string; recipient:string; username:string; hasSecret?:boolean };
  type Channel = { id:string; name:string; recipientId:string; kind:string; enabled:boolean; config:Config };
  type History = { id:string; eventId:string; channelId:string; status:string; attempt:number; errorCode?:string; occurredAt:number };
  const emptyConfig = ():Config => ({destination:'',smtpHost:'',smtpPort:465,sender:'',recipient:'',username:''});
  const admin=$derived($session?.user.role==='administrator');
  let enabled=$state(false),scans=$state(false),tasks=$state(false),content=$state(false),loaded=$state(false),busy=$state(false),error=$state(''),notice=$state('');
  let requests=$state(false),reports=$state(false);
  let channels=$state<Channel[]>([]),history=$state<History[]>([]),users=$state<{id:string;role:string;name?:string}[]>([]);
  let editId=$state(''),name=$state(''),recipientId=$state(''),kind=$state('webhook'),channelEnabled=$state(false),config=$state<Config>(emptyConfig()),secret=$state('');
  async function request<T>(path:string,method='GET',body?:unknown,signal?:AbortSignal):Promise<T> {
    const response=await fetch(path,{method,cache:'no-store',credentials:'same-origin',headers:method==='GET'?{}:{'content-type':'application/json','x-csrf-token':$session?.csrfToken??''},body:body===undefined?undefined:JSON.stringify(body),signal:signal?AbortSignal.any([signal,AbortSignal.timeout(12000)]):AbortSignal.timeout(12000)});
    if(!response.ok) throw new Error(response.status===403?$t('action_not_allowed'):response.status===400?$t('check_the_fields_the_secret_and'):$t('notifications_unavailable'));
    return response.status===204?undefined as T:await response.json() as T;
  }
  $effect(()=>{
    const id=$session?.user.id,controller=new AbortController();loaded=false;
    if(!id)return;
    void loadFeatures().catch(()=>{});
    void request<{enabled:boolean;scans:boolean;tasks:boolean;content?:boolean;requests?:boolean;reports?:boolean}>('/api/v1/account/notifications','GET',undefined,controller.signal).then(value=>{
      if(controller.signal.aborted)return;enabled=value.enabled;scans=value.scans;tasks=value.tasks;content=value.content??false;requests=value.requests??false;reports=value.reports??false;loaded=true;
    }).catch(()=>{if(!controller.signal.aborted)error=$t('notification_preferences_unavailable');});
    if(admin)void loadAdmin(controller.signal);
    return ()=>controller.abort();
  });
  async function loadAdmin(signal?:AbortSignal) {
    try {
      const [data,log]=await Promise.all([request<{items:Channel[];users:{id:string;role:string;name?:string}[]}>('/api/v1/admin/notifications/channels','GET',undefined,signal),request<{items:History[]}>('/api/v1/admin/notifications/history','GET',undefined,signal)]);
      if(signal?.aborted)return;channels=data.items;users=data.users;history=log.items;
      if(!recipientId)recipientId=$session?.user.id??'';
    }catch{if(!signal?.aborted)error=$t('notification_configuration_or_history');}
  }
  async function savePreferences(event:SubmitEvent){
    event.preventDefault();busy=true;error='';notice='';
    try{await request('/api/v1/account/notifications','PUT',{enabled,scans,tasks:admin&&tasks,content,requests,reports});notice=$t('preferences_saved');}catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}
  }
  function edit(channel?:Channel){editId=channel?.id??'';name=channel?.name??'';recipientId=channel?.recipientId??$session?.user.id??'';kind=channel?.kind??'webhook';channelEnabled=channel?.enabled??false;config=channel?{...channel.config}:emptyConfig();secret='';}
  async function saveChannel(event:SubmitEvent){
    event.preventDefault();busy=true;error='';notice='';
    const { hasSecret:_hasSecret,...fields }=config;void _hasSecret;
    try{
      await request(`/api/v1/admin/notifications/channels${editId?`/${encodeURIComponent(editId)}`:''}`,editId?'PUT':'POST',{name,recipientId,kind,enabled:channelEnabled,config:{...fields,...(secret?{secret}:{})}});
      edit();await loadAdmin();notice=$t('destination_saved_no_test_message_sent');
    }catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}
  }
  async function remove(id:string){busy=true;error='';notice='';try{await request(`/api/v1/admin/notifications/channels/${encodeURIComponent(id)}`,'DELETE');if(editId===id)edit();await loadAdmin();notice=$t('destination_deleted_and_its_queue_removed');}catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}}
</script>

<section class="notification-settings" aria-labelledby="notification-heading">
  <h2 id="notification-heading">{$t('notifications')}</h2>
  <p>{$t('external_notifications_are_optional_an')}</p>
  <form onsubmit={savePreferences}><fieldset disabled={busy||!loaded}>
    <legend>{$t('my_preferences')}</legend>
    <label class="check"><input type="checkbox" bind:checked={enabled} /> {$t('receive_notifications_on_my_configured')}</label>
    <label class="check"><input type="checkbox" bind:checked={scans} /> {$t('completed_scans_of_my_authorised_libraries')}</label>
    <label class="check"><input type="checkbox" bind:checked={content} /> {$t('new_films_series_and_episodes_allowed')}</label>
    {#if $features.requests}<label class="check"><input type="checkbox" bind:checked={requests} /> {$t('follow_up_on_my_content_requests')}</label>{/if}
    {#if $features.reports}<label class="check"><input type="checkbox" bind:checked={reports} /> {$t('replies_and_status_changes_on_my')}</label>{/if}
    {#if admin}<label class="check"><input type="checkbox" bind:checked={tasks} /> {$t('completed_or_failed_tasks_administration')}</label>{/if}
    <button>{$t('save_my_preferences')}</button>
  </fieldset></form>
  {#if admin}
    <details><summary>{$t('destinations_and_administrator_history')}</summary>
      <p>{$t('smtp_tls_email_ntfy_gotify_and')}</p>
      <p>{$t('persistent_queue_five_attempts_at_most')}</p>
      <button type="button" disabled={busy} onclick={()=>edit()}>{$t('new_destination')}</button>
      {#each channels as channel (channel.id)}<div class="channel"><span>{channel.name} · {channel.kind} · {channel.enabled?'Activée':'Désactivée'}</span><button type="button" disabled={busy} onclick={()=>edit(channel)}>{$t('edit')}</button><button type="button" disabled={busy} onclick={()=>remove(channel.id)}>{$t('delete')}</button></div>{/each}
      <form onsubmit={saveChannel}><fieldset disabled={busy}><legend>{editId?$t('edit_the_destination'):$t('new_destination')}</legend>
        <label>{$t('name')}<input bind:value={name} required maxlength="100" /></label>
        <label>{$t('recipient_account')}<select bind:value={recipientId} required>{#each users as user (user.id)}<option value={user.id}>{user.name??user.id} · {user.role}</option>{/each}</select></label>
        <label>{$t('adapter')}<select bind:value={kind}><option value="email">{$t('smtp_tls_email')}</option><option value="ntfy">{$t('ntfy')}</option><option value="gotify">{$t('gotify')}</option><option value="webhook">{$t('signed_webhook')}</option></select></label>
        {#if kind==='email'}
          <label>{$t('smtp_server')}<input bind:value={config.smtpHost} placeholder={$t('smtp_example_org')} required /></label>
          <p>{$t('port_465_tls_with_a_verified')}</p>
          <label>{$t('sender_address')}<input type="email" bind:value={config.sender} required /></label>
          <label>{$t('recipient_address')}<input type="email" bind:value={config.recipient} required /></label>
          <label>{$t('smtp_username_optional')}<input bind:value={config.username} autocomplete="off" /></label>
        {:else}
          <label>{kind==='ntfy'?$t('ntfy_topic_https_url'):kind==='gotify'?$t('gotify_base_https_url'):$t('receiver_https_url')}<input type="url" bind:value={config.destination} required placeholder="https://notifications.example.org" /></label>
          <p>{$t('public_https_destination_on_port_443')}</p>
        {/if}
        <label>{kind==='webhook'?$t('shared_hmac_secret_32_characters_minimum'):kind==='email'?$t('smtp_password'):$t('access_token')}<input type="password" bind:value={secret} autocomplete="new-password" placeholder={config.hasSecret?$t('secret_kept_if_the_destination_is'):''} minlength={kind==='webhook'?32:undefined} /></label>
        <p>{$t('an_empty_secret_keeps_the_existing')}</p>
        <label class="check"><input type="checkbox" bind:checked={channelEnabled} /> {$t('enable_future_sends_to_this_destination')}</label>
        <button>{$t('save_the_destination')}</button>
      </fieldset></form>
      <button type="button" disabled={busy} onclick={()=>loadAdmin()}>{$t('refresh_history')}</button>
      <ul>{#each history as item (item.id)}<li>{$fmt.dateTime(item.occurredAt)} · {item.status} · {$t('notification_attempt', { attempt: item.attempt })}{item.errorCode?` · ${item.errorCode}`:''}</li>{:else}<li>{$t('no_notifications_recorded')}</li>{/each}</ul>
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

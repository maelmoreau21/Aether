<script lang="ts">
  type Field={key:string;label:string;description?:string;fieldType:string;defaultValue?:string;required?:boolean;secret?:boolean;options?:{label:string;value:string}[]};
  let {fields,values=$bindable(),busy,onsave}:{fields:Field[];values:Record<string,string|number|boolean>;busy:boolean;onsave:()=>void}=$props();
</script>
<form class="plugin-settings" onsubmit={event=>{event.preventDefault();onsave();}}>

  <div class="fields">{#each fields as field (field.key)}
    {#if field.fieldType==='boolean'}
      <label class="toggle-row"><span><strong>{field.label}</strong>{#if field.description}<small>{field.description}</small>{/if}</span><input type="checkbox" role="switch" checked={values[field.key]===true||values[field.key]==='true'} disabled={busy} onchange={event=>values[field.key]=event.currentTarget.checked} /></label>
    {:else}<label class="field"><span>{field.label}{#if field.required}<span aria-hidden="true"> *</span>{/if}</span>
      {#if field.fieldType==='select'}<select aria-label={field.label} required={field.required} disabled={busy} bind:value={values[field.key]}>{#each field.options??[] as option (option.value)}<option value={option.value}>{option.label}</option>{/each}</select>
      {:else}<input aria-label={field.label} type={field.secret||field.fieldType==='password'?'password':field.fieldType==='number'?'number':field.key==='base_url'?'url':'text'} min={field.fieldType==='number'?(field.key==='pool_size'?'1':'0'):undefined} max={field.key==='pool_size'?'24':field.key==='min_hours_between_switches'?'720':undefined} step={field.fieldType==='number'?'1':undefined} required={field.required} autocomplete={field.secret?'new-password':'off'} disabled={busy} bind:value={values[field.key]} />{/if}
      {#if field.description}<small>{field.description}</small>{/if}</label>
    {/if}
  {/each}</div>
  <div class="save-row"><button disabled={busy}>Enregistrer les paramètres</button></div>
</form>
<style>
.plugin-settings{margin:0;max-width:760px;}.fields{display:grid;gap:24px;}.field{display:grid;gap:9px;font-size:14px;}.field>span{font-weight:600;}small{display:block;color:var(--text-secondary);font-size:12px;line-height:1.5;font-weight:400;}.field input,.field select{box-sizing:border-box;max-width:520px;width:100%;min-height:44px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--bg-base);color:var(--text-primary);padding:10px 12px;font:inherit;}.toggle-row{display:flex;align-items:center;justify-content:space-between;gap:24px;padding:16px 0;border-bottom:1px solid var(--border-subtle);font-size:14px;cursor:pointer;}.toggle-row small{margin-top:6px;max-width:540px;}.toggle-row input{appearance:none;flex:0 0 42px;width:42px;height:24px;border-radius:20px;background:var(--text-muted,#777);position:relative;cursor:pointer;}.toggle-row input::after{content:'';position:absolute;top:3px;left:3px;width:18px;height:18px;border-radius:50%;background:white;transition:transform .15s;}.toggle-row input:checked{background:var(--color-accent);}.toggle-row input:checked::after{transform:translateX(18px);}input:focus-visible,select:focus-visible{outline:2px solid var(--color-accent);outline-offset:3px;}.save-row{margin-top:28px;}button{min-height:44px;padding:10px 20px;border:0;border-radius:var(--radius-sm);background:var(--color-accent);color:white;font:inherit;cursor:pointer;}button:disabled{opacity:.5;}
</style>

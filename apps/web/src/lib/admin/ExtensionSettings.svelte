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
  .plugin-settings { max-width: 760px; margin: 0; color: var(--tx); font-family: var(--font); }
  .fields { display: grid; gap: 20px; }
  .field { display: grid; gap: 8px; font-size: 14px; }
  .field > span { font-weight: 700; color: var(--tx); }
  small { display: block; color: var(--mu); font-size: 13px; font-weight: 500; line-height: 1.5; }
  .field input, .field select {
    box-sizing: border-box;
    width: 100%;
    max-width: 520px;
    height: 46px;
    padding: 0 14px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background-color: var(--pn);
    color: var(--tx);
    font: inherit;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }
  .field select { padding-right: 40px; }
  .field input:hover, .field select:hover { background-color: var(--pn-2); }
  .field input:focus, .field select:focus { border-color: var(--ac); background-color: var(--bg-3); }
  .field input:disabled, .field select:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Réglage à interrupteur */
  .toggle-row { display: flex; align-items: center; justify-content: space-between; gap: 24px; min-height: 44px; padding: 14px 0; border-bottom: 1px solid var(--line); font-size: 14px; cursor: pointer; }
  .toggle-row strong { font-weight: 700; color: var(--tx); }
  .toggle-row small { max-width: 540px; margin-top: 4px; }
  .toggle-row input { appearance: none; -webkit-appearance: none; position: relative; flex: 0 0 46px; width: 46px; height: 28px; margin: 0; border-radius: var(--r-pill); background: var(--pn-3); cursor: pointer; transition: background-color var(--t-med) ease; }
  .toggle-row input::after { content: ''; position: absolute; top: 3px; left: 3px; width: 22px; height: 22px; border-radius: 50%; background: var(--tx); transition: transform var(--t-med) var(--ease), background-color var(--t-med) ease; }
  .toggle-row input:checked { background: var(--ac); }
  .toggle-row input:checked::after { transform: translateX(18px); background: var(--on-media); }
  .toggle-row input:disabled { opacity: 0.45; cursor: not-allowed; }

  input:focus-visible, select:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  input:focus, select:focus { outline: none; }
  input:focus-visible, select:focus-visible, button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  .save-row { display: flex; justify-content: flex-end; margin-top: 24px; }
  button { height: 48px; padding: 0 22px; border: 0; border-radius: var(--r-md); background: var(--btn); color: var(--btn-tx); font: inherit; font-weight: 700; box-shadow: none; cursor: pointer; transition: opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { opacity: 0.9; }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }

  @media (max-width: 640px) {
    .field input, .field select { font-size: 16px; }
    .save-row button { width: 100%; }
  }

  @media (prefers-reduced-motion: reduce) {
    .field input, .field select, .toggle-row input, .toggle-row input::after, button { transition: none; }
  }
</style>

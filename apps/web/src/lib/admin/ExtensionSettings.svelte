<script lang="ts">
  import { t } from '../i18n';
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
  <div class="save-row"><button disabled={busy}>{$t('save_extension_settings')}</button></div>
</form>
<style>
  .plugin-settings { max-width: 760px; margin: 0; color: var(--color-text); font-family: var(--font-sans); }
  .fields { display: grid; }
  .field { display: grid; gap: 8px; padding: 14px 0; border-top: 1px solid var(--color-line-subtle); font-size: 14px; }
  .fields > :first-child { border-top: 0; }
  .field > span { font-weight: 600; color: var(--color-text); }
  small { display: block; color: var(--color-muted); font-size: 12px; font-weight: 400; line-height: 1.5; }
  .field input, .field select { box-sizing: border-box; width: 100%; max-width: 520px; min-height: 42px; padding: 8px 12px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; }
  .field input:disabled, .field select:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Réglage à interrupteur */
  .toggle-row { display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 14px 0; border-top: 1px solid var(--color-line-subtle); font-size: 14px; cursor: pointer; }
  .fields > :first-child.toggle-row { border-top: 0; }
  .toggle-row strong { font-weight: 600; color: var(--color-text); }
  .toggle-row small { max-width: 540px; margin-top: 4px; }
  .toggle-row input { appearance: none; -webkit-appearance: none; position: relative; flex: 0 0 42px; width: 42px; height: 24px; margin: 0; border-radius: var(--radius-pill); background: var(--color-line-strong); cursor: pointer; transition: background-color var(--transition-fast); }
  .toggle-row input::after { content: ''; position: absolute; top: 3px; left: 3px; width: 18px; height: 18px; border-radius: 50%; background: var(--color-text-bright); transition: transform var(--transition-fast); }
  .toggle-row input:checked { background: var(--color-accent); }
  .toggle-row input:checked::after { transform: translateX(18px); }
  .toggle-row input:disabled { opacity: 0.45; cursor: not-allowed; }

  input:focus-visible, select:focus-visible, button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }

  .save-row { display: flex; justify-content: flex-end; margin-top: 20px; }
  button { min-height: 42px; padding: 8px 20px; border: 0; border-radius: var(--radius-sm); background: var(--color-accent-solid); color: var(--color-on-accent); font: inherit; font-weight: 600; cursor: pointer; transition: background-color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-accent-hover); opacity: 0.9; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }

  @media (max-width: 640px) {
    .field input, .field select { font-size: 16px; }
    .save-row button { width: 100%; }
  }

  @media (prefers-reduced-motion: reduce) {
    .toggle-row input, .toggle-row input::after, button { transition: none; }
  }
</style>

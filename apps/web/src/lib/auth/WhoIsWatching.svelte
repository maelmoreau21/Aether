<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { session } from './session';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';
  import {
    type AccountProfile, type AccountProfiles,
    PROFILES_AFTER_LOGIN, createProfile, deleteProfile, isValidPin, loadProfiles, profileInitial, setProfilePin, switchProfile
  } from './profiles';

  let account = $state<AccountProfiles | null>(null);
  let error = $state('');
  let busy = $state(false);
  let managing = $state(false);
  // Profile waiting for its PIN (switch), or whose PIN is being set (manage).
  let pinFor = $state<AccountProfile | null>(null);
  let pinMode = $state<'switch' | 'set'>('switch');
  let pin = $state('');
  let newName = $state('');
  let newKid = $state(false);
  let afterLogin = $state(false);
  const csrf = $derived($session?.csrfToken ?? '');

  async function refresh() {
    try {
      account = await loadProfiles();
      // A single-profile account has nobody to choose: go straight to the catalogue.
      if (afterLogin && account.profiles.length < 2) void goto(resolve('/'), { replaceState: true });
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('account_profiles_unavailable'); }
  }
  onMount(() => {
    try { afterLogin = sessionStorage.getItem(PROFILES_AFTER_LOGIN) === '1'; sessionStorage.removeItem(PROFILES_AFTER_LOGIN); } catch { afterLogin = false; }
    void refresh();
  });

  async function run(action: () => Promise<void>) {
    if (busy) return;
    busy = true; error = '';
    try { await action(); } catch (cause) { error = cause instanceof Error ? cause.message : $t('account_profiles_unavailable'); }
    finally { busy = false; }
  }

  function choose(profile: AccountProfile) {
    if (managing) return;
    if (profile.isCurrent) { void goto(resolve('/')); return; }
    if (profile.hasPin) { pinFor = profile; pinMode = 'switch'; pin = ''; return; }
    void enter(profile);
  }

  // A full reload drops every cache filled by the previous profile (catalogue, progress, preferences).
  const enter = (profile: AccountProfile, code?: string) => run(async () => {
    await switchProfile(csrf, profile.id, code);
    window.location.assign(resolve('/'));
  });

  function submitPin(event: SubmitEvent) {
    event.preventDefault();
    const target = pinFor;
    if (!target || !isValidPin(pin)) { error = $t('profiles_pin_format'); return; }
    const code = pin;
    if (pinMode === 'switch') { void enter(target, code); return; }
    void run(async () => { await setProfilePin(csrf, target.id, code); pinFor = null; pin = ''; await refresh(); });
  }

  const removePin = (profile: AccountProfile) => run(async () => { await setProfilePin(csrf, profile.id, null); await refresh(); });
  const remove = (profile: AccountProfile) => {
    if (!confirm($t('profiles_delete_confirm', { name: profile.name }))) return;
    void run(async () => { await deleteProfile(csrf, profile.id); await refresh(); });
  };
  function add(event: SubmitEvent) {
    event.preventDefault();
    const name = newName.trim();
    if (!name) return;
    void run(async () => { await createProfile(csrf, name, newKid); newName = ''; newKid = false; await refresh(); });
  }
</script>

<main class="who">
  <h1>{managing ? $t('profiles_manage') : $t('profiles_who_is_watching')}</h1>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if account}
    <ul class="grid">
      {#each account.profiles as profile (profile.id)}
        <li class="tile" class:current={profile.isCurrent}>
          <button type="button" class="pick" disabled={busy || managing} onclick={() => choose(profile)} aria-label={$t('profiles_open', { name: profile.name })}>
            <span class="avatar" class:kid={profile.restricted} aria-hidden="true">{profileInitial(profile.name)}</span>
            <span class="name">{profile.name}</span>
            <span class="badges">
              {#if profile.hasPin}<span class="badge"><Icon name="shield" size={12} />{$t('profiles_locked')}</span>{/if}
              {#if profile.restricted}<span class="badge">{$t('profiles_kid')}</span>{/if}
            </span>
          </button>
          {#if managing}
            <div class="manage">
              <button type="button" disabled={busy} onclick={() => { pinFor = profile; pinMode = 'set'; pin = ''; }}>
                <Icon name="edit" size={13} />{profile.hasPin ? $t('profiles_change_pin') : $t('profiles_add_pin')}
              </button>
              {#if profile.hasPin}<button type="button" disabled={busy} onclick={() => removePin(profile)}>{$t('profiles_remove_pin')}</button>{/if}
              {#if !profile.isOwner}
                <button type="button" class="danger" disabled={busy} onclick={() => remove(profile)} aria-label={$t('profiles_delete', { name: profile.name })}><Icon name="trash" size={13} /></button>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>

    {#if pinFor}
      <form class="pin" onsubmit={submitPin}>
        <label for="profile-pin">{pinMode === 'switch' ? $t('profiles_enter_pin', { name: pinFor.name }) : $t('profiles_new_pin', { name: pinFor.name })}</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="profile-pin" type="password" inputmode="numeric" autocomplete="off" pattern="[0-9]*" minlength="4" maxlength="6" bind:value={pin} autofocus />
        <div class="row">
          <button type="submit" class="primary" disabled={busy}>{$t('profiles_confirm')}</button>
          <button type="button" onclick={() => { pinFor = null; pin = ''; }}>{$t('profiles_cancel')}</button>
        </div>
      </form>
    {/if}

    {#if managing && account.profiles.length - 1 < account.maxProfiles}
      <form class="add" onsubmit={add}>
        <label for="profile-name">{$t('profiles_new')}</label>
        <input id="profile-name" maxlength="40" bind:value={newName} placeholder={$t('profiles_name')} />
        <label class="check"><input type="checkbox" bind:checked={newKid} />{$t('profiles_kid_option')}</label>
        <button type="submit" class="primary" disabled={busy || !newName.trim()}>{$t('profiles_create')}</button>
      </form>
      <p class="hint">{$t('profiles_owner_pin_hint')}</p>
    {/if}

    <div class="row footer">
      {#if account.canManage}
        <button type="button" onclick={() => { managing = !managing; pinFor = null; }}>{managing ? $t('profiles_done') : $t('profiles_manage')}</button>
      {/if}
      {#if !afterLogin}<a href={resolve('/')}>{$t('profiles_back')}</a>{/if}
    </div>
  {:else if !error}
    <p class="loading" role="status">{$t('loading')}</p>
  {/if}
</main>

<style>
  .who { min-height: 100dvh; box-sizing: border-box; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 28px; padding: 48px 16px; background: var(--color-canvas, var(--bg)); color: var(--tx); }
  h1 { margin: 0; font-size: clamp(26px, 4vw, 40px); font-weight: 800; letter-spacing: -0.02em; text-align: center; }
  .grid { display: flex; flex-wrap: wrap; justify-content: center; gap: 24px; margin: 0; padding: 0; list-style: none; }
  .tile { display: flex; flex-direction: column; align-items: center; gap: 10px; width: 140px; }
  .pick { display: flex; flex-direction: column; align-items: center; gap: 10px; width: 100%; padding: 0; border: 0; background: none; color: inherit; font: inherit; cursor: pointer; }
  .pick:disabled { cursor: default; }
  .avatar { display: grid; place-items: center; width: 120px; height: 120px; border-radius: var(--r-lg, 18px); border: 3px solid transparent; background: var(--pn-2); color: var(--tx); font-size: 48px; font-weight: 800; transition: border-color .15s ease, transform .15s ease; }
  .avatar.kid { background: color-mix(in srgb, var(--ac) 35%, var(--pn-2)); }
  .current .avatar { border-color: var(--line-2, var(--line)); }
  .pick:not(:disabled):hover .avatar, .pick:focus-visible .avatar { border-color: var(--tx); transform: scale(1.04); }
  .pick:focus-visible { outline: none; }
  .name { font-weight: 700; text-align: center; overflow-wrap: anywhere; }
  .badges { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; min-height: 20px; }
  .badge { display: inline-flex; align-items: center; gap: 4px; padding: 2px 8px; border-radius: 999px; background: var(--pn); color: var(--mu); font-size: 12px; font-weight: 600; }
  .manage { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; }
  button:not(.pick), a { display: inline-flex; align-items: center; gap: 6px; min-height: 36px; padding: 0 14px; border: 1px solid var(--line); border-radius: var(--r-sm, 10px); background: var(--pn); color: var(--tx); font: inherit; font-weight: 700; text-decoration: none; cursor: pointer; }
  .manage button { min-height: 30px; padding: 0 10px; font-size: 13px; }
  button.primary { border-color: transparent; background: var(--ac); color: var(--on-ac, #fff); }
  button.danger { color: var(--danger, #e5484d); }
  button:disabled { opacity: .5; cursor: not-allowed; }
  button:focus-visible, a:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
  form { display: flex; flex-wrap: wrap; align-items: center; justify-content: center; gap: 10px 12px; width: min(100%, 520px); padding: 16px; border: 1px solid var(--line); border-radius: var(--r-lg, 18px); background: var(--bg-2, var(--pn)); }
  form > label:first-child { width: 100%; text-align: center; font-weight: 700; }
  input:not([type='checkbox']) { height: 40px; padding: 0 12px; border: 1px solid var(--line); border-radius: var(--r-sm, 10px); background: var(--pn); color: var(--tx); font: inherit; }
  .pin input { width: 10ch; text-align: center; letter-spacing: .4em; font-size: 20px; }
  .add input:not([type='checkbox']) { flex: 1 1 180px; }
  .check { display: inline-flex; align-items: center; gap: 6px; }
  .row { display: flex; flex-wrap: wrap; justify-content: center; gap: 10px; }
  .error { margin: 0; color: var(--danger, #e5484d); font-weight: 600; text-align: center; }
  .hint, .loading { margin: 0; max-width: 52ch; color: var(--mu); font-size: 14px; text-align: center; }
  @media (max-width: 480px) { .tile { width: 108px; } .avatar { width: 96px; height: 96px; font-size: 38px; } }
  @media (prefers-reduced-motion: reduce) { .avatar { transition: none; } }
</style>

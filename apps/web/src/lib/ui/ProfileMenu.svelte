<script lang="ts">
  import { resolve } from '$app/paths';
  import Icon from './Icon.svelte';
  import { goto } from '$app/navigation';
  import { session, logout } from '../auth/session';

  let panel: HTMLDivElement;
  let expanded = $state(false);
  let busy = $state(false);
  let error = $state('');
  let administration = $state(false);
  async function signOut() {
    if (!$session || busy) return;
    busy = true; error = '';
    try { await logout($session); panel.hidePopover(); await goto(resolve('login/'), { replaceState:true }); }
    catch (failure) { error = failure instanceof Error ? failure.message : 'Déconnexion impossible. Réessayez.'; }
    finally { busy = false; }
  }

  function toggled(event: ToggleEvent) {
    expanded = event.newState === 'open';
    if (expanded) panel.querySelector<HTMLAnchorElement>('a[href]')?.focus();
  }

  function focusLeft(event: FocusEvent) {
    if (event.relatedTarget instanceof Node && !panel.parentElement?.contains(event.relatedTarget)) panel.hidePopover();
  }
</script>

<div class="profile-access">
  <button class="profile-trigger" popovertarget="profile-menu" aria-label="Menu profil" aria-expanded={expanded} aria-controls="profile-menu">
    <span class="avatar"><Icon name="profile" size={18} /></span>
    <span class="profile-caption">{$session?.user.login ?? 'Espace personnel'}</span>
    <span class="profile-chevron" class:expanded><Icon name="chevron" size={12} /></span>
  </button>
  <div id="profile-menu" class="profile-popover" popover="auto" bind:this={panel} ontoggle={toggled} onfocusout={focusLeft}>
    <div class="profile-heading"><span class="avatar"><Icon name="profile" size={18} /></span><div><strong>{$session?.user.login}</strong><p>{$session?.user.role === 'administrator' ? 'Administrateur' : 'Utilisateur'}</p></div></div>
    <nav aria-label="Menu profil">
      <button class="profile-entry" disabled aria-describedby="profile-planned"><Icon name="profile" size={17} /><span>Mon profil</span><small>Bientôt</small></button>
      <span id="profile-planned" class="sr-only">Avatar, identité, mot de passe et préférences personnelles de langue, audio et sous-titres : bientôt.</span>
      <a class="profile-entry" href={resolve('settings/')} onclick={() => panel.hidePopover()}><Icon name="settings" size={17} /><span>Paramètres</span><Icon name="chevron" size={13} /></a>
      {#if $session?.user.role === 'administrator'}<button class="profile-entry" onclick={() => administration = !administration} aria-expanded={administration}><Icon name="settings" size={17} /><span>Administration</span></button>{/if}
      {#if administration && $session?.user.role === 'administrator'}<p class="menu-notice">L’administration du serveur sera disponible prochainement.</p>{/if}
      <button class="profile-entry" disabled={busy} onclick={signOut}><span>{busy ? 'Déconnexion…' : 'Se déconnecter'}</span></button>
      {#if error}<p class="menu-notice error" role="alert">{error}</p>{/if}
    </nav>
  </div>
</div>

<style>
  .profile-access { margin-left:auto; flex-shrink:0; }
  .profile-trigger { display:flex; align-items:center; gap:10px; min-height:44px; border:1px solid transparent; border-radius:24px; padding:4px 9px 4px 4px; background:transparent; color:var(--color-text); }
  .profile-trigger:hover,.profile-trigger[aria-expanded='true'] { background:var(--color-surface); border-color:#b4a1f833; }
  .profile-trigger:active { background:var(--color-accent-soft); }
  .avatar { display:inline-flex; align-items:center; justify-content:center; width:34px; height:34px; border:1px solid #b4a1f84d; border-radius:50%; background:var(--color-accent-soft); color:var(--color-accent); flex-shrink:0; }
  .profile-caption { font-size:11px; color:#d1cadf; white-space:nowrap; }
  .profile-chevron { display:inline-flex; color:var(--color-muted); transform:rotate(90deg); transition:transform .18s; }
  .profile-chevron.expanded { transform:rotate(-90deg); }
  .profile-popover { position:fixed; inset:76px var(--page-gutter) auto auto; margin:0; width:264px; max-width:calc(100vw - 32px); padding:8px; border:1px solid #b4a1f830; border-radius:14px; color:var(--color-text); background:#15141e; box-shadow:0 18px 50px #0009; }
  .profile-popover:popover-open { animation:reveal .14s ease-out; }
  .profile-heading { display:flex; gap:12px; align-items:center; padding:12px 10px 18px; margin-bottom:6px; border-bottom:1px solid var(--color-line); }
  strong { font-size:13px; font-weight:550; }p { margin:4px 0 0; color:var(--color-muted); font-size:10px; }
  nav { display:grid; gap:4px; }
  .profile-entry { display:flex; align-items:center; gap:12px; width:100%; min-height:46px; padding:12px; border:0; border-radius:8px; background:transparent; color:var(--color-text); font-size:12px; text-align:left; }
  .profile-entry>span { flex:1; }
  .profile-entry:disabled { opacity:1; color:var(--color-muted); cursor:default; }
  small { padding:3px 6px; border:1px solid var(--color-line); border-radius:4px; font-size:9px; color:var(--color-muted); }
  .profile-entry:hover:not(:disabled),.profile-entry:focus-visible { background:var(--color-accent-soft); color:var(--color-accent-hover); }
  .menu-notice { padding:4px 12px; line-height:1.7; }.error { color:var(--color-danger); }.profile-heading>div { min-width:0; overflow-wrap:anywhere; }.profile-caption { max-width:150px; overflow:hidden; text-overflow:ellipsis; }
  a.profile-entry:active { background:#34294b; }
  a.profile-entry:focus-visible { outline-offset:-2px; }
  @keyframes reveal { from { opacity:0; transform:translateY(-4px); } to { opacity:1; transform:translateY(0); } }
  @media(max-width:1100px) { .profile-caption { display:none; } }
  @media(max-width:700px) { .profile-popover { top:64px; right:20px; }.profile-trigger { padding-right:6px; } }
</style>

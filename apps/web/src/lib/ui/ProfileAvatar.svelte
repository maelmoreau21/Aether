<script lang="ts">
  import { session } from '../auth/session';
  import { avatarRevision } from '../auth/avatar';
  import Icon from './Icon.svelte';
  let { size = 34 }: { size?: number } = $props();
  let failed = $state(false);
  const source = $derived(`/api/v1/account/avatar?account=${encodeURIComponent($session?.user.id ?? '')}&v=${$avatarRevision}`);
  $effect(() => { if (source) failed = false; });
</script>
<span class="profile-image" style:width={`${size}px`} style:height={`${size}px`} aria-hidden="true">
  {#if $session && !failed}<img src={source} alt="" width={size} height={size} onerror={() => failed = true} />{:else}<Icon name="profile" size={Math.round(size * .52)} />{/if}
</span>
<style>
  .profile-image { display: inline-grid; place-items: center; border-radius: 50%; overflow: hidden; flex-shrink: 0; background: var(--color-accent-soft); color: var(--accent-primary); }
  img { display: block; width: 100%; height: 100%; object-fit: cover; }
</style>

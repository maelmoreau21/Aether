<script lang="ts">
  // The account's other devices that are playing: drive them from here, or
  // take their title over at the same position ("Resume here").
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';
  import { remote, type RemoteDevice } from './remote.svelte';
  import { remoteControlEnabled } from '../config/presentation';

  let open = $state(false);
  const playing = $derived(remote.devices.filter(device => device.playing));

  function clock(seconds: number): string {
    const s = Math.max(0, Math.floor(seconds));
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
    return h ? `${h}:${String(m).padStart(2, '0')}:${String(r).padStart(2, '0')}` : `${m}:${String(r).padStart(2, '0')}`;
  }
  function seekBy(device: RemoteDevice, delta: number) {
    const at = device.playing?.position ?? 0;
    const end = device.playing?.duration ?? Infinity;
    remote.send(device.deviceId, 'seek', Math.min(end, Math.max(0, at + delta)));
  }
  function playHere(workId: string, position?: number) {
    remote.pendingOpen = { workId, position };
    if (remote.catalogs === 0) void goto(resolve('/'));
  }
  function resumeHere(device: RemoteDevice) {
    const now = device.playing;
    if (!now) return;
    remote.send(device.deviceId, 'stop');
    open = false;
    playHere(now.workId, now.position);
  }

  // Off on this server: no socket at all (the Core would refuse it).
  $effect(() => {
    if (!$remoteControlEnabled) return;
    remote.start();
    // "Open" sent by another device: play it here.
    const unsubscribe = remote.onCommand(command => {
      if (command.action === 'open' && command.workId) playHere(command.workId, command.position);
    });
    return () => { unsubscribe(); remote.stop(); };
  });
</script>

{#if playing.length}
  <div class="remote">
    <button type="button" class="toggle" aria-expanded={open} aria-controls="remote-panel" title={$t('remote_devices')} onclick={() => open = !open}>
      <Icon name="tv" size={18} /><span>{playing[0].name}{playing.length > 1 ? ` +${playing.length - 1}` : ''}</span>
    </button>
    {#if open}
      <section id="remote-panel" class="panel" aria-label={$t('remote_devices')}>
        {#each playing as device (device.deviceId)}
          {@const now = device.playing!}
          <article>
            <p class="device">{device.name}</p>
            <p class="title">{now.title ?? $t('remote_idle')}</p>
            <p class="time">{clock(now.position)}{now.duration ? ` / ${clock(now.duration)}` : ''}</p>
            <div class="actions">
              <button type="button" aria-label={$t('remote_back')} title={$t('remote_back')} onclick={() => seekBy(device, -10)}><Icon name="jump-backward" size={18} /></button>
              <button type="button" aria-label={$t('remote_play_pause')} title={$t('remote_play_pause')} onclick={() => remote.send(device.deviceId, now.paused ? 'play' : 'pause')}><Icon name={now.paused ? 'play' : 'pause'} size={18} /></button>
              <button type="button" aria-label={$t('remote_forward')} title={$t('remote_forward')} onclick={() => seekBy(device, 30)}><Icon name="jump-forward" size={18} /></button>
              <button type="button" aria-label={$t('remote_stop')} title={$t('remote_stop')} onclick={() => remote.send(device.deviceId, 'stop')}><Icon name="close" size={18} /></button>
              <button type="button" class="here" onclick={() => resumeHere(device)}>{$t('remote_resume_here')}</button>
            </div>
          </article>
        {/each}
      </section>
    {/if}
  </div>
{/if}

<style>
  .remote { position: fixed; right: 16px; bottom: calc(var(--music-player-height, 0px) + 16px + env(safe-area-inset-bottom, 0px)); z-index: 119; display: grid; justify-items: end; gap: 8px; max-width: calc(100vw - 32px); }
  .toggle { display: inline-flex; align-items: center; gap: 8px; min-height: 40px; max-width: 100%; padding: 0 14px; border: 1px solid var(--line); border-radius: 999px; background: var(--glass); backdrop-filter: blur(18px); -webkit-backdrop-filter: blur(18px); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; }
  .toggle span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .panel { display: grid; gap: 8px; width: min(340px, calc(100vw - 32px)); padding: 10px; border: 1px solid var(--line); border-radius: 18px; background: var(--glass); backdrop-filter: blur(18px); -webkit-backdrop-filter: blur(18px); color: var(--tx); }
  article { display: grid; gap: 2px; padding: 8px; }
  p { margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .device, .time { color: var(--mu); font-size: 13px; }
  .title { font-weight: 700; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 6px; }
  .actions button { display: inline-grid; place-items: center; min-width: 40px; min-height: 40px; border: 0; border-radius: 12px; background: rgb(127 127 127 / 0.14); color: inherit; font: inherit; cursor: pointer; }
  .actions button:hover { background: rgb(127 127 127 / 0.24); }
  .actions .here { padding: 0 12px; margin-left: auto; font-size: 14px; font-weight: 700; background: var(--ac); color: var(--btn-tx, #fff); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
</style>

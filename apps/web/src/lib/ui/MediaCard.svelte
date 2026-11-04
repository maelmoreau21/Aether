<script lang="ts">
  import Icon from './Icon.svelte';
  let { container, active, disabled, onclick }: { container: string; active: boolean; disabled: boolean; onclick: () => void } = $props();
</script>
<button class="media-card" class:active {disabled} {onclick} aria-label="Sélectionner la version {container}" aria-pressed={active}>
  <span class="art" class:alternate={container === 'MP4'}><span class="edition">{container}</span><span class="number">{container === 'MKV' ? '01' : '02'}</span><span class="selected">{#if active}<Icon name="check" size={15} />{:else}<Icon name="arrow" size={15} />{/if}</span></span>
  <span class="details"><span><strong>Mire de démonstration</strong><small>{container} · AV1 10 bits · 12 secondes</small></span><span class="state">{active ? 'Sélectionné' : 'Choisir'}</span></span>
</button>
<style>
  .media-card { display: block; width: 100%; text-align: left; border: 0; padding: 0; border-radius: var(--r-md); color: var(--tx); background: transparent; cursor: pointer; transition: transform var(--t-slow) var(--ease); }
  .media-card:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .art { display: block; position: relative; height: 174px; overflow: hidden; border-radius: var(--r-md); background: linear-gradient(0deg, var(--media-shade), transparent 80%), url('/images/cinematic-horizon.png') center 48% / cover; box-shadow: inset 0 0 0 1px var(--line); transition: box-shadow var(--t-med) ease; }
  .art.alternate { background-position: center 77%; filter: saturate(.65); }
  .active .art { box-shadow: inset 0 0 0 2px var(--ac); }
  .media-card:hover:not(:disabled) .art { box-shadow: inset 0 0 0 2px var(--line-2); }
  .active:hover:not(:disabled) .art { box-shadow: inset 0 0 0 2px var(--ac); }
  .media-card:hover:not(:disabled) { transform: translateY(-3px); }
  .media-card:disabled { opacity: 0.45; cursor: default; }
  .edition { position: absolute; top: 16px; left: 16px; padding: 4px 8px; background: var(--media-shade); border-radius: var(--r-xs); color: var(--on-media); font-size: 11px; font-weight: 700; letter-spacing: .07em; }
  .number { position: absolute; left: 20px; bottom: 7px; font-size: 68px; font-weight: 300; letter-spacing: -.06em; color: var(--on-media); opacity: 0.8; }
  .selected { position: absolute; bottom: 18px; right: 18px; border-radius: 50%; display: grid; place-items: center; width: 32px; height: 32px; background: var(--media-shade); color: var(--on-media); }
  .active .selected { background: var(--ac); color: var(--on-ac); }
  .details { display: flex; justify-content: space-between; gap: 10px; padding: 14px 2px; align-items: center; }
  strong { display: block; font-size: 15px; font-weight: 700; }
  small { display: block; color: var(--mu); font-size: 13px; margin-top: 4px; }
  .state { color: var(--mu); font-size: 13px; font-weight: 600; }
  .active .state { color: var(--ac); }
  @media (prefers-reduced-motion: reduce) { .media-card:hover:not(:disabled) { transform: none; } }
  @media (max-width: 550px) { .art { height: 150px; } .state { display: none; } }
</style>

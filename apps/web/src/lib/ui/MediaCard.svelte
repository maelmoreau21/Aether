<script lang="ts">
  import Icon from './Icon.svelte';
  let { container, active, disabled, onclick }: { container: string; active: boolean; disabled: boolean; onclick: () => void } = $props();
</script>
<button class="media-card" class:active {disabled} {onclick} aria-label="Sélectionner la version {container}" aria-pressed={active}>
  <span class="art" class:alternate={container === 'MP4'}><span class="edition">{container}</span><span class="number">{container === 'MKV' ? '01' : '02'}</span><span class="selected">{#if active}<Icon name="check" size={15} />{:else}<Icon name="arrow" size={15} />{/if}</span></span>
  <span class="details"><span><strong>Mire de démonstration</strong><small>{container} · AV1 10 bits · 12 secondes</small></span><span class="state">{active ? 'Sélectionné' : 'Choisir'}</span></span>
</button>
<style>
  .media-card { display: block; width: 100%; text-align: left; border: 0; padding: 0; border-radius: var(--radius-md); color: var(--color-text); background: transparent; }
  .art { display: block; position: relative; height: 174px; overflow: hidden; border-radius: var(--radius-md); background: linear-gradient(0deg,#080710a8,transparent 80%), url('/images/cinematic-horizon.png') center 48% / cover; box-shadow: inset 0 0 0 1px #ffffff0c; transition: box-shadow .2s; }
  .art.alternate { background-position: center 77%; filter: saturate(.65); }
  .active .art { box-shadow: inset 0 0 0 2px var(--color-accent); }
  .media-card:hover:not(:disabled) .art { box-shadow: inset 0 0 0 2px #d6c9ff; }
  .media-card:hover:not(:disabled) { transform: translateY(-3px); }
  .edition { position: absolute; top: 16px; left: 16px; padding: 4px 7px; background: #111019a6; backdrop-filter: blur(10px); border-radius: 4px; font-size: 9px; font-weight: 600; letter-spacing: .16em; }
  .number { position: absolute; left: 20px; bottom: 7px; font-size: 68px; font-weight: 250; letter-spacing: -.06em; color: #f8f5efc9; }
  .selected { position: absolute; bottom: 18px; right: 18px; border-radius: 50%; display: grid; place-items: center; width: 30px; height: 30px; background: #ffffff16; color: white; }
  .active .selected { background: var(--color-accent); color: #161020; }
  .details { display: flex; justify-content: space-between; gap: 10px; padding: 16px 2px; align-items: center; }
  strong { display: block; font-size: 13px; font-weight: 550; } small { display: block; color: var(--color-subtle); font-size: 11px; margin-top: 6px; }
  .state { color: var(--color-accent); font-size: 10px; }
  @media(max-width:550px) { .art { height: 150px; } .state { display: none; } }
</style>

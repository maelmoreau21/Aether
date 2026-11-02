<script lang="ts">
  import { preferenceState, savePreferences, DEFAULT_HOME_SECTIONS, type HomeSection } from '../preferences';
  const names: Record<HomeSection, string> = { hero: 'À la une', continue: 'Reprendre la lecture', films: 'Films récents', series: 'Séries récentes', rated: 'Les mieux notés' };
  const sections = $derived($preferenceState.homeSections ?? DEFAULT_HOME_SECTIONS);
  let saveError = $state('');
  async function save(patch: Parameters<typeof savePreferences>[0]) {
    saveError = await savePreferences(patch) ? '' : 'Enregistrement impossible. Modifiez à nouveau le réglage pour réessayer.';
  }
  function move(index: number, direction: number) {
    const next = [...sections];
    [next[index], next[index + direction]] = [next[index + direction], next[index]];
    void save({ homeSections: next });
  }
  function toggle(section: HomeSection) {
    void save({ homeSections: sections.includes(section) ? sections.filter(item => item !== section) : [...sections, section] });
  }
</script>
<section class="customization" aria-labelledby="customization-title">
  <h2 id="customization-title">Personnaliser l’affichage</h2>
  <p>Ces choix suivent votre compte sur vos appareils. Une page déjà ouverte les recharge quand vous y revenez.</p>
  <div class="choices">
    <label>Couleur d’accent
      <select value={$preferenceState.accentColor ?? 'orange'} onchange={event => void save({ accentColor: event.currentTarget.value as 'orange' | 'blue' | 'violet' | 'green' })}>
        <option value="orange">Orange</option><option value="blue">Bleu</option><option value="violet">Violet</option><option value="green">Vert</option>
      </select>
    </label>
    <label>Taille des affiches
      <select value={$preferenceState.cardSize ?? 'medium'} onchange={event => void save({ cardSize: event.currentTarget.value as 'small' | 'medium' | 'large' })}>
        <option value="small">Petite</option><option value="medium">Moyenne</option><option value="large">Grande</option>
      </select>
    </label>
    <label>Espacement des cartes
      <select value={$preferenceState.cardDensity ?? 'comfortable'} onchange={event => void save({ cardDensity: event.currentTarget.value as 'comfortable' | 'compact' })}>
        <option value="comfortable">Confortable</option><option value="compact">Compact</option>
      </select>
    </label>
  </div>
  <h3>Sections de l’accueil</h3>
  <ol>
    {#each sections as section, index (section)}
      <li><span>{names[section]}</span>
        <button type="button" aria-label={`Monter ${names[section]}`} disabled={index === 0} onclick={() => move(index, -1)}>↑</button>
        <button type="button" aria-label={`Descendre ${names[section]}`} disabled={index === sections.length - 1} onclick={() => move(index, 1)}>↓</button>
        <button type="button" onclick={() => toggle(section)}>Masquer</button>
      </li>
    {/each}
  </ol>
  {#each DEFAULT_HOME_SECTIONS.filter(section => !sections.includes(section)) as section (section)}
    <button class="restore" type="button" onclick={() => toggle(section)}>Afficher : {names[section]}</button>
  {/each}
  <button type="button" onclick={() => void save({ homeSections: [...DEFAULT_HOME_SECTIONS], cardSize: 'medium', cardDensity: 'comfortable', accentColor: 'orange' })}>Rétablir l’affichage par défaut</button>
  {#if saveError}<p role="alert">{saveError}</p>{/if}
</section>
<style>
  .customization { padding:24px; margin-bottom:24px; border:1px solid var(--color-line); border-radius:var(--radius-md); background:var(--color-surface); }
  h2 { margin-top:0; } p { color:var(--color-muted); line-height:1.5; }
  .choices { display:flex; flex-wrap:wrap; gap:16px; } label { display:grid; gap:8px; }
  select,button { color:var(--color-text); background:var(--color-raised); border:1px solid var(--color-line-strong); padding:8px 10px; border-radius:8px; }
  button { cursor:pointer; } button:disabled { opacity:.4; cursor:default; } ol { padding:0; list-style:none; }
  li { display:flex; align-items:center; gap:8px; margin:10px 0; flex-wrap:wrap; } li span { flex:1; min-width:120px; } .restore { margin:0 8px 12px 0; }
</style>

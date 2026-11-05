<script lang="ts">
  // Yearly recap as full-screen story slides: auto-advance, tap/keys, hold to pause.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { getSessionGeneration } from '../auth/session';
  import { statisticsRequest, type Rank, type Wrapped, type WrappedSettings } from './api';
  import { st, statLocale } from './i18n';
  import { browserTimeZone, growthInfo, monthName, shortDuration, weekdayName, wrappedSlides, wrappedSummary, type SlideId } from './insights';
  import { imagePreviewUrl } from '../catalog/image-url';

  let { userId = '' }: { userId?: string } = $props();
  const SLIDE_MS = 8000;
  const zone = browserTimeZone();
  let data = $state<Wrapped | null>(null), hidden = $state<WrappedSettings | null>(null);
  let loading = $state(true), error = $state(''), copied = $state('');
  let year = $state<number | null>(null), kind = $state<Wrapped['kind']>('all');
  let index = $state(0), elapsed = $state(0), paused = $state(false), held = $state(false);
  let failed = $state<string[]>([]);
  let stage: HTMLElement;
  let request = 0;

  const slides = $derived<SlideId[]>(data ? wrappedSlides(data) : []);
  const current = $derived<SlideId>(slides[index] ?? 'intro');
  const terminal = $derived(current === 'summary' || current === 'empty');
  const number = $derived(new Intl.NumberFormat($statLocale, { maximumFractionDigits: 0 }));
  const decimal = $derived(new Intl.NumberFormat($statLocale, { maximumFractionDigits: 1 }));
  const backdrop = $derived(data ? leadImage(current, data) : '');
  const monthMax = $derived(Math.max(1, ...(data?.monthly ?? []).map(m => m.seconds)));
  const hourMax = $derived(Math.max(1, ...(data?.hours ?? [])));
  const genreMax = $derived(Math.max(1, ...(data?.genres ?? []).map(g => g.seconds)));

  function leadImage(slide: SlideId, w: Wrapped): string {
    const pick = slide === 'films' ? w.films[0]?.image : slide === 'series' ? w.series[0]?.image : slide === 'binge' ? w.binge?.image : slide === 'music' ? w.albums[0]?.image ?? w.artists[0]?.image : slide === 'moments' ? w.last?.image : '';
    return pick && !failed.includes(pick) ? imagePreviewUrl(pick, 640) : '';
  }
  function fail(url: string) { if (!failed.includes(url)) failed = [...failed, url]; }
  function longDate(value: string | number) {
    const date = typeof value === 'number' ? new Date(value * 1000) : new Date(`${value}T12:00:00`);
    return date.toLocaleDateString($statLocale, { day: 'numeric', month: 'long', year: typeof value === 'number' ? 'numeric' : undefined });
  }
  function hourLabel(hour: number) { return $st('hour_label', { hour }); }
  function kindLabel(value: string) { return $st(value === 'music' ? 'kind_music' : value === 'episode' ? 'kind_episode' : value === 'series' ? 'kind_series' : 'kind_film'); }

  async function load() {
    const ticket = ++request, generation = getSessionGeneration();
    loading = true; error = '';
    const query = new URLSearchParams({ tz: zone, kind });
    if (year) query.set('year', String(year));
    const path = userId ? `/api/v1/admin/statistics/wrapped/${encodeURIComponent(userId)}` : '/api/v1/account/statistics/wrapped';
    try {
      const result: Wrapped = await (await statisticsRequest(`${path}?${query}`)).json();
      if (ticket !== request || generation !== getSessionGeneration()) return;
      if (result.visible) { data = result; hidden = null; year = result.year; } else { data = null; hidden = result.settings; }
      index = 0; elapsed = 0; paused = false;
    } catch (cause) {
      if (ticket === request) { data = null; error = cause instanceof Error ? cause.message : $st('w_error'); }
    } finally { if (ticket === request) loading = false; }
  }
  function go(target: number) {
    if (!slides.length) return;
    index = Math.max(0, Math.min(slides.length - 1, target)); elapsed = 0;
  }
  function next() { if (index < slides.length - 1) go(index + 1); }
  function previous() { go(index - 1); }
  function close() {
    if (history.length > 1) history.back();
    else void goto(resolve(userId ? 'admin/libraries/#statistics-section' : 'settings/#statistics'));
  }
  async function copy() {
    if (!data) return;
    const text = wrappedSummary(data, {
      title: $st('s_title'), hours: $st('s_hours'), plays: $st('s_plays'), titles: $st('s_titles'), topFilm: $st('s_film'), topSeries: $st('s_series'),
      topArtist: $st('s_artist'), topGenre: $st('s_genre'), streak: $st('s_streak'), footer: $st('s_footer')
    }, $statLocale);
    try { await navigator.clipboard.writeText(text); copied = $st('w_copied'); }
    catch { copied = $st('w_copy_failed'); }
    setTimeout(() => copied = '', 2500);
  }

  // Tap the left third to go back, elsewhere to advance; holding pauses.
  let downAt = 0, holdTimer: ReturnType<typeof setTimeout> | undefined;
  function interactive(target: EventTarget | null) { return target instanceof Element && Boolean(target.closest('button, a, select, input, label')); }
  function pointerDown(event: PointerEvent) {
    if (interactive(event.target) || event.button > 0) return;
    downAt = performance.now();
    holdTimer = setTimeout(() => { held = true; }, 220);
  }
  function pointerUp(event: PointerEvent) {
    if (!downAt) return;
    clearTimeout(holdTimer);
    const wasHeld = held; held = false; downAt = 0;
    if (wasHeld) return;
    const rect = stage.getBoundingClientRect();
    if (event.clientX - rect.left < rect.width / 3) previous(); else next();
  }
  function pointerCancel() { clearTimeout(holdTimer); held = false; downAt = 0; }
  function key(event: KeyboardEvent) {
    if (event.target instanceof HTMLSelectElement || event.target instanceof HTMLInputElement) return;
    if (event.key === 'ArrowRight' || event.key === 'PageDown') { event.preventDefault(); next(); }
    else if (event.key === 'ArrowLeft' || event.key === 'PageUp') { event.preventDefault(); previous(); }
    else if (event.key === ' ' && !(event.target instanceof HTMLButtonElement)) { event.preventDefault(); paused = !paused; }
    else if (event.key === 'Home') go(0);
    else if (event.key === 'End') go(slides.length - 1);
    else if (event.key === 'Escape') close();
  }
  onMount(() => {
    void load();
    let frame = 0, last = performance.now();
    const tick = (now: number) => {
      const delta = Math.min(250, now - last); last = now;
      if (data && !paused && !held && !terminal && document.visibilityState === 'visible') {
        elapsed += delta;
        if (elapsed >= SLIDE_MS) next();
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    window.addEventListener('keydown', key);
    return () => { cancelAnimationFrame(frame); window.removeEventListener('keydown', key); clearTimeout(holdTimer); request++; };
  });
</script>

{#snippet art(item: { image: string; title: string }, size: 'large' | 'small' = 'small', round = false)}
  <span class={`art ${size}`} class:round>
    {#if item.image && !failed.includes(item.image)}
      <img src={imagePreviewUrl(item.image, size === 'large' ? 640 : 320)} alt="" loading="lazy" decoding="async" onerror={() => fail(item.image)} />
    {:else}<span class="fallback" aria-hidden="true">{item.title.trim().charAt(0).toUpperCase() || '·'}</span>{/if}
  </span>
{/snippet}
{#snippet ranking(items: Rank[], detail: (item: Rank) => string, round = false)}
  <ol class="ranking">
    {#each items as item, position (item.id)}
      <li class="rise" style:--i={position + 2}>
        <span class="position">{position + 1}</span>
        {@render art(item, 'small', round)}
        <span class="label"><strong>{item.title}</strong>{#if item.subtitle}<small>{item.subtitle}</small>{/if}<small>{detail(item)}</small></span>
      </li>
    {/each}
  </ol>
{/snippet}

<div class="wrapped" role="dialog" aria-modal="true" aria-label={data ? $st('w_title', { year: data.year }) : $st('w_loading')}>
  <header class="top">
    {#if data && slides.length > 1}
      <div class="progress" aria-hidden="true">
        {#each slides as slide, position (slide)}
          <span class="segment"><span class="fill" style:--p={position < index ? 1 : position === index ? (terminal ? 1 : elapsed / SLIDE_MS) : 0}></span></span>
        {/each}
      </div>
    {/if}
    <div class="toolbar">
      <div class="who">
        <strong>{data ? $st('w_title', { year: data.year }) : $st('w_loading')}</strong>
        {#if data && userId}<small>{$st('w_for', { name: data.user.name })}</small>{/if}
      </div>
      <div class="controls">
        {#if data}
          <label><span class="h-sr">{$st('w_year')}</span><select bind:value={year} onchange={() => void load()}>{#each data.availableYears as option (option)}<option value={option}>{option}</option>{/each}</select></label>
          <label><span class="h-sr">{$st('w_kind')}</span><select bind:value={kind} onchange={() => { year = data?.year ?? year; void load(); }}><option value="all">{$st('w_all')}</option><option value="film">{$st('films')}</option><option value="episode">{$st('kind_series')}</option><option value="music">{$st('music')}</option></select></label>
          {#if !terminal}<button class="icon" onclick={() => paused = !paused} aria-label={paused ? $st('w_play') : $st('w_pause')} aria-pressed={paused}>
            {#if paused}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14l11-7z" /></svg>{:else}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14M16 5v14" /></svg>{/if}
          </button>{/if}
        {/if}
        <button class="icon" onclick={close} aria-label={$st('w_close')}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg></button>
      </div>
    </div>
  </header>

  <main class="stage" bind:this={stage} onpointerdown={pointerDown} onpointerup={pointerUp} onpointercancel={pointerCancel} onpointerleave={pointerCancel} class:held>
    {#if loading && !data}
      <div class="slide center"><p class="rise eyebrow">{$st('w_loading')}</p></div>
    {:else if error}
      <div class="slide center"><h1 class="rise">{error}</h1><button class="primary rise" style:--i={1} onclick={() => void load()}>{$st('refresh')}</button></div>
    {:else if hidden}
      <div class="slide center"><h1 class="rise">{$st('w_hidden')}</h1>
        {#if hidden.visible && hidden.periodEnabled}<p class="lead rise" style:--i={1}>{$st('w_hidden_window', { start: `${hidden.startDay} ${monthName(hidden.startMonth, $statLocale)}`, end: `${hidden.endDay} ${monthName(hidden.endMonth, $statLocale)}` })}</p>{/if}
        <button class="primary rise" style:--i={2} onclick={close}>{$st('w_back')}</button></div>
    {:else if data}
      {#if backdrop}<img class="backdrop" src={backdrop} alt="" aria-hidden="true" onerror={() => fail(backdrop)} /><span class="veil" aria-hidden="true"></span>{/if}
      {#key `${data.year}-${data.kind}-${current}`}
        <section class={`slide ${current}`} aria-roledescription="slide" aria-label={$st('w_slide', { index: index + 1, count: slides.length })}>
          {#if current === 'empty'}
            <h1 class="rise">{$st('w_empty_title', { year: data.year })}</h1>
            <p class="lead rise" style:--i={1}>{$st('w_empty_text')}</p>
          {:else if current === 'intro'}
            <p class="eyebrow rise">{$st('w_intro_eyebrow', { year: data.year })}</p>
            <h1 class="rise" style:--i={1}>{$st('w_intro_title', { name: data.user.name })}</h1>
            <p class="year rise" style:--i={2} aria-hidden="true">{data.year}</p>
            <p class="hint rise" style:--i={3}>{$st('w_intro_hint')}</p>
          {:else if current === 'time'}
            <p class="eyebrow rise">{$st('w_time_title')}</p>
            <p class="giant pop" style:--i={1}>{number.format(data.totals.hours)}</p>
            <p class="lead rise" style:--i={2}>{$st('w_time_unit')}</p>
            <p class="rise" style:--i={3}>{$st('w_time_days', { days: decimal.format(data.totals.days) })}</p>
            <p class="muted rise" style:--i={4}>{$st('w_time_detail', { plays: number.format(data.totals.plays), titles: number.format(data.totals.titles), active: number.format(data.totals.activeDays) })}</p>
            <ul class="kinds rise" style:--i={5}>{#each (['film', 'episode', 'music'] as const) as name (name)}{#if data.kinds[name].seconds}<li><strong>{shortDuration(data.kinds[name].seconds)}</strong><small>{name === 'episode' ? $st('kind_series') : kindLabel(name)}</small></li>{/if}{/each}</ul>
          {:else if current === 'monthly'}
            <h2 class="rise">{$st('w_monthly_title')}</h2>
            <div class="months rise" style:--i={1} role="img" aria-label={$st('w_monthly_title')}>
              {#each data.monthly as month (month.month)}
                <span class="month" title={`${monthName(month.month, $statLocale)} · ${shortDuration(month.seconds)}`}><span class="col"><span class="bar" class:peak={month.month === data.peaks.month} style:--h={month.seconds / monthMax} style:--i={month.month}></span></span><small>{monthName(month.month, $statLocale, 'short')}</small></span>
              {/each}
            </div>
            {#if data.peaks.month}<p class="lead rise" style:--i={3}>{$st('w_monthly_peak', { month: monthName(data.peaks.month, $statLocale) })}</p>{/if}
          {:else if current === 'rhythm'}
            <h2 class="rise">{$st('w_rhythm_title')}</h2>
            <div class="duo">
              {#if data.peaks.hour !== null}<div class="rise" style:--i={1}><small class="eyebrow">{$st('w_rhythm_hour')}</small><p class="big">{hourLabel(data.peaks.hour)}</p></div>{/if}
              {#if data.peaks.weekday !== null}<div class="rise" style:--i={2}><small class="eyebrow">{$st('w_rhythm_day')}</small><p class="big cap">{weekdayName(data.peaks.weekday, $statLocale)}</p></div>{/if}
            </div>
            <div class="hours rise" style:--i={3} aria-hidden="true">{#each data.hours as value, hour (hour)}<span class:peak={hour === data.peaks.hour} style:--h={value / hourMax}></span>{/each}</div>
          {:else if current === 'streak'}
            <p class="eyebrow rise">{$st('w_streak_title')}</p>
            <p class="giant pop" style:--i={1}>{data.streak.days}</p>
            <p class="lead rise" style:--i={2}>{$st('w_streak_days', { days: data.streak.days })}</p>
            {#if data.streak.start && data.streak.end}<p class="muted rise" style:--i={3}>{$st('w_streak_range', { start: longDate(data.streak.start), end: longDate(data.streak.end) })}</p>{/if}
          {:else if current === 'films'}
            <h2 class="rise">{$st('w_films_title')}</h2>
            <div class="feature">{@render art(data.films[0], 'large')}{@render ranking(data.films, item => `${shortDuration(item.seconds)} · ${number.format(item.plays)} ${$st('w_plays')}`)}</div>
          {:else if current === 'series'}
            <h2 class="rise">{$st('w_series_title')}</h2>
            <div class="feature">{@render art(data.series[0], 'large')}{@render ranking(data.series, item => `${shortDuration(item.seconds)} · ${$st('episodes_count', { count: item.items })}`)}</div>
          {:else if current === 'binge' && data.binge}
            <p class="eyebrow rise">{$st('w_binge_title')}</p>
            <div class="feature">{@render art(data.binge, 'large')}<div><p class="giant pop" style:--i={1}>{data.binge.episodes}</p><p class="lead rise" style:--i={2}>{$st('w_binge_text', { episodes: data.binge.episodes, title: data.binge.title, date: longDate(data.binge.date) })}</p></div></div>
          {:else if current === 'music'}
            <h2 class="rise">{$st('w_music_title')}</h2>
            <div class="music-grid">
              <div><h3 class="rise" style:--i={1}>{$st('w_artists')}</h3>{@render ranking(data.artists, item => `${shortDuration(item.seconds)} · ${number.format(item.plays)} ${$st('w_plays')}`, true)}</div>
              {#if data.albums.length}<div><h3 class="rise" style:--i={1}>{$st('w_albums')}</h3>{@render ranking(data.albums, item => shortDuration(item.seconds))}</div>{/if}
              {#if data.tracks.length}<div><h3 class="rise" style:--i={1}>{$st('w_tracks')}</h3>{@render ranking(data.tracks, item => `${number.format(item.plays)} ${$st('w_plays')}`)}</div>{/if}
            </div>
          {:else if current === 'genres'}
            <h2 class="rise">{$st('w_genres_title')}</h2>
            <ol class="genres">{#each data.genres as genre, position (genre.id)}<li class="rise" style:--i={position + 1}><span class="name">{genre.title}</span><span class="meter"><span style:--w={genre.seconds / genreMax}></span></span><small>{shortDuration(genre.seconds)}</small></li>{/each}</ol>
          {:else if current === 'people'}
            <h2 class="rise">{$st('w_people_title')}</h2>
            <ol class="people">{#each data.people as person, position (`${person.role}:${person.name}`)}<li class="rise" style:--i={position + 1}><span class="initial" aria-hidden="true">{person.name.charAt(0)}</span><span><strong>{person.name}</strong><small>{person.role === 'director' ? $st('role_director') : $st('role_actor')} · {$st('titles_count', { count: person.titles })}</small></span></li>{/each}</ol>
          {:else if current === 'device'}
            <p class="eyebrow rise">{$st('w_device_title')}</p>
            <p class="big rise" style:--i={1}>{data.topDevice}</p>
            <p class="lead rise" style:--i={2}>{$st('w_device_text')}</p>
            {#if data.devices.length > 1}<ul class="kinds rise" style:--i={3}>{#each data.devices.slice(1, 4) as item (item.id)}<li><strong>{shortDuration(item.seconds)}</strong><small>{item.title}</small></li>{/each}</ul>{/if}
          {:else if current === 'moments' && data.first}
            <h2 class="rise">{$st('w_moments_title')}</h2>
            <div class="moments">
              {#each [[$st('w_first'), data.first], [$st('w_last'), data.last ?? data.first]] as [label, moment], position (position)}
                {#if moment && typeof moment !== 'string'}<div class="rise" style:--i={position + 1}>{@render art(moment, 'small')}<span><small class="eyebrow">{label}</small><strong>{moment.title}</strong><small>{longDate(moment.at)} · {kindLabel(moment.kind)}</small></span></div>{/if}
              {/each}
            </div>
          {:else if current === 'compare' && data.previous}
            <h2 class="rise">{$st('w_compare_title', { year: data.year - 1 })}</h2>
            <dl class="compare">
              {#each [[$st('w_compare_time'), shortDuration(data.totals.seconds), shortDuration(data.previous.seconds), data.previous.secondsGrowth], [$st('w_compare_plays'), number.format(data.totals.plays), number.format(data.previous.plays), data.previous.playsGrowth], [$st('w_compare_titles'), number.format(data.totals.titles), number.format(data.previous.titles), data.previous.titlesGrowth]] as [label, now, before, change], position (position)}
                {@const info = growthInfo(change as number | null, $statLocale)}
                <div class="rise" style:--i={position + 1}><dt>{label}</dt><dd><strong>{now}</strong><small>{data.year - 1} : {before}</small><em class={info.direction}>{info.direction === 'none' ? '—' : info.label}</em></dd></div>
              {/each}
            </dl>
          {:else if current === 'summary'}
            <div class="summary-card pop">
              <p class="eyebrow">{$st('w_summary_title', { year: data.year })}</p>
              <h2>{data.user.name}</h2>
              <dl>
                <div><dt>{$st('w_hours')}</dt><dd>{number.format(data.totals.hours)}</dd></div>
                <div><dt>{$st('w_plays')}</dt><dd>{number.format(data.totals.plays)}</dd></div>
                <div><dt>{$st('w_titles')}</dt><dd>{number.format(data.totals.titles)}</dd></div>
                {#if data.streak.days >= 2}<div><dt>{$st('w_streak_title')}</dt><dd>{data.streak.days}</dd></div>{/if}
              </dl>
              <ul>
                {#if data.films[0]}<li><small>{$st('kind_film')}</small>{data.films[0].title}</li>{/if}
                {#if data.series[0]}<li><small>{$st('kind_series')}</small>{data.series[0].title}</li>{/if}
                {#if data.artists[0]}<li><small>{$st('w_artists')}</small>{data.artists[0].title}</li>{/if}
                {#if data.genres[0]}<li><small>{$st('genres_title')}</small>{data.genres[0].title}</li>{/if}
                {#if data.topDevice}<li><small>{$st('device')}</small>{data.topDevice}</li>{/if}
              </ul>
              <div class="summary-actions">
                <button class="primary" onclick={() => void copy()}>{$st('w_copy')}</button>
                <button onclick={() => go(0)}>{$st('w_restart')}</button>
              </div>
              <p class="copied" role="status">{copied}</p>
            </div>
          {/if}
        </section>
      {/key}
    {/if}
  </main>
  {#if data && slides.length > 1}
    <nav class="nav" aria-label={$st('w_slide', { index: index + 1, count: slides.length })}>
      <button class="icon" onclick={previous} disabled={index === 0} aria-label={$st('w_prev')}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg></button>
      <span aria-live="polite">{index + 1} / {slides.length}</span>
      <button class="icon" onclick={next} disabled={index === slides.length - 1} aria-label={$st('w_next')}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg></button>
    </nav>
  {/if}
</div>

<style>
  .wrapped { position: fixed; inset: 0; z-index: 2000; display: grid; grid-template-rows: auto 1fr auto; background: var(--bg); color: var(--tx); overflow: hidden; user-select: none; -webkit-user-select: none; touch-action: manipulation; }
  .top { position: relative; z-index: 3; padding: max(14px, env(safe-area-inset-top)) var(--gutter) 0; }
  .progress { display: flex; gap: 4px; margin-bottom: 14px; }
  .segment { flex: 1; height: 3px; border-radius: var(--r-pill); background: var(--pn-3); overflow: hidden; }
  .fill { display: block; height: 100%; width: 100%; background: var(--tx); transform-origin: left; transform: scaleX(var(--p)); }
  .toolbar { display: flex; justify-content: space-between; align-items: center; gap: 12px; flex-wrap: wrap; }
  .who { display: grid; gap: 2px; } .who strong { font-weight: 800; letter-spacing: -0.01em; } .who small { color: var(--mu); }
  .controls { display: flex; gap: 8px; align-items: center; }
  .controls select { min-height: 40px; width: auto; padding: 8px 34px 8px 12px; font-size: 13px; }
  button { min-height: 40px; padding: 0 16px; border-radius: var(--r-md); border: 1px solid var(--line-2); background: var(--pn); color: var(--tx); font-weight: 700; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--pn-2); }
  button.icon { width: 40px; padding: 0; display: inline-grid; place-items: center; border-radius: var(--r-pill); }
  button.primary { background: var(--btn); color: var(--btn-tx); border-color: transparent; }
  svg { width: 20px; height: 20px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .stage { position: relative; display: grid; place-items: center; min-height: 0; padding: 12px var(--gutter); cursor: pointer; }
  .backdrop { position: absolute; inset: -40px; width: calc(100% + 80px); height: calc(100% + 80px); object-fit: cover; filter: blur(48px) saturate(1.1) brightness(0.55); opacity: var(--ambient-opacity); pointer-events: none; }
  .veil { position: absolute; inset: 0; background: linear-gradient(0deg, var(--bg) 6%, rgb(var(--bg-rgb) / 0.4) 100%); pointer-events: none; }
  .slide { position: relative; z-index: 1; width: min(1100px, 100%); display: grid; gap: clamp(10px, 2vh, 20px); align-content: center; max-height: 100%; overflow: auto; padding: 8px 0; }
  .slide.center, .slide.intro, .slide.time, .slide.streak, .slide.device, .slide.empty { text-align: center; justify-items: center; }
  h1 { font-size: clamp(2rem, 5.5vw, 4.2rem); line-height: 1.05; letter-spacing: -0.035em; font-weight: 800; margin: 0; max-width: 18ch; }
  h2 { font-size: clamp(1.6rem, 4vw, 3rem); letter-spacing: -0.03em; font-weight: 800; margin: 0 0 8px; }
  h3 { font-size: 13px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--mu); margin: 0 0 10px; }
  .eyebrow { text-transform: uppercase; letter-spacing: 0.12em; font-size: 12px; font-weight: 700; color: var(--mu); margin: 0; }
  .lead { font-size: clamp(1.1rem, 2.4vw, 1.6rem); font-weight: 700; margin: 0; max-width: 30ch; }
  .muted, .hint { color: var(--mu); margin: 0; }
  p { margin: 0; }
  .giant { font-size: clamp(5rem, 20vw, 13rem); font-weight: 800; line-height: 0.9; letter-spacing: -0.06em; font-variant-numeric: tabular-nums; }
  .big { font-size: clamp(2.2rem, 7vw, 5rem); font-weight: 800; letter-spacing: -0.04em; line-height: 1; }
  .cap { text-transform: capitalize; }
  .year { font-size: clamp(5rem, 22vw, 15rem); font-weight: 800; letter-spacing: -0.07em; line-height: 0.85; color: var(--pn-3); }
  .kinds { list-style: none; display: flex; flex-wrap: wrap; justify-content: center; gap: 12px 28px; padding: 0; margin: 8px 0 0; }
  .kinds li { display: grid; gap: 2px; } .kinds strong { font-size: 1.3rem; } .kinds small { color: var(--mu); }
  .months { display: grid; grid-template-columns: repeat(12, minmax(0, 1fr)); gap: clamp(4px, 1vw, 12px); height: min(42vh, 320px); }
  .month { display: grid; grid-template-rows: 1fr auto; gap: 8px; min-width: 0; }
  .col { position: relative; border-radius: var(--r-xs); background: var(--pn); overflow: hidden; }
  .bar { position: absolute; inset: auto 0 0; height: calc(var(--h) * 100%); background: var(--mu-2); border-radius: var(--r-xs); transform-origin: bottom; animation: grow var(--t-slow) var(--ease) both; animation-delay: calc(var(--i) * 50ms + 200ms); }
  .bar.peak { background: var(--ac); }
  .month small { text-align: center; color: var(--mu); font-size: 11px; text-transform: capitalize; overflow: hidden; text-overflow: ellipsis; }
  .duo { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 24px; }
  .hours { display: grid; grid-template-columns: repeat(24, 1fr); gap: 3px; align-items: end; height: 120px; }
  .hours span { height: max(3px, calc(var(--h) * 100%)); background: var(--mu-2); border-radius: 3px; }
  .hours span.peak { background: var(--ac); }
  .feature { display: grid; grid-template-columns: minmax(150px, 280px) 1fr; gap: clamp(18px, 4vw, 48px); align-items: center; }
  .ranking { list-style: none; margin: 0; padding: 0; display: grid; gap: 12px; }
  .ranking li { display: grid; grid-template-columns: 26px auto 1fr; gap: 14px; align-items: center; }
  .position { font-size: 1.2rem; font-weight: 800; color: var(--mu); font-variant-numeric: tabular-nums; }
  .label { display: grid; gap: 2px; min-width: 0; } .label strong { font-size: 1.05rem; overflow-wrap: anywhere; } .label small { color: var(--mu); font-size: 12.5px; }
  .art { position: relative; display: block; overflow: hidden; border-radius: var(--r-xs); background: var(--pn-2); aspect-ratio: 2 / 3; }
  .art.small { width: 48px; } .art.large { width: 100%; border-radius: var(--r-md); box-shadow: var(--shadow); }
  .art.round { border-radius: 50%; aspect-ratio: 1; }
  .music-grid .art.small:not(.round) { aspect-ratio: 1; }
  .art img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .fallback { position: absolute; inset: 0; display: grid; place-items: center; font-weight: 800; font-size: 1.2rem; color: var(--mu); }
  .art.large .fallback { font-size: 4rem; }
  .music-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 28px; }
  .genres { list-style: none; padding: 0; margin: 0; display: grid; gap: 16px; }
  .genres li { display: grid; grid-template-columns: minmax(120px, 220px) 1fr auto; gap: 16px; align-items: center; }
  .genres .name { font-size: clamp(1.1rem, 2.6vw, 1.8rem); font-weight: 800; }
  .meter { height: 10px; border-radius: var(--r-pill); background: var(--pn); overflow: hidden; }
  .meter span { display: block; height: 100%; width: calc(var(--w) * 100%); background: var(--tx); border-radius: var(--r-pill); }
  .genres li:first-child .meter span { background: var(--ac); }
  .genres small { color: var(--mu); font-variant-numeric: tabular-nums; }
  .people { list-style: none; padding: 0; margin: 0; display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 14px; }
  .people li { display: flex; align-items: center; gap: 14px; padding: 14px; border-radius: var(--r-md); background: var(--pn); }
  .people strong { display: block; } .people small { color: var(--mu); }
  .initial { display: grid; place-items: center; width: 48px; height: 48px; border-radius: 50%; background: var(--pn-2); font-weight: 800; font-size: 1.2rem; }
  .moments { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 22px; }
  .moments > div { display: flex; gap: 16px; align-items: center; } .moments .art.small { width: 72px; }
  .moments span { display: grid; gap: 4px; } .moments strong { font-size: 1.2rem; } .moments small:not(.eyebrow) { color: var(--mu); }
  .compare { display: grid; gap: 16px; margin: 0; }
  .compare > div { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 16px; padding: 18px 0; border-bottom: 1px solid var(--line); }
  .compare dt { font-size: 1.1rem; color: var(--mu); }
  .compare dd { margin: 0; display: flex; align-items: baseline; gap: 14px; flex-wrap: wrap; justify-content: flex-end; }
  .compare dd strong { font-size: clamp(1.4rem, 3.5vw, 2.4rem); font-weight: 800; }
  .compare small { color: var(--mu); }
  em { font-style: normal; font-weight: 800; padding: 4px 10px; border-radius: var(--r-pill); background: var(--pn); color: var(--mu); }
  em.up { color: var(--ok); background: var(--ok-soft); } em.down { color: var(--err); background: var(--err-soft); }
  .summary-card { justify-self: center; width: min(520px, 100%); display: grid; gap: 16px; padding: clamp(22px, 4vw, 36px); border-radius: var(--r-xl); background: var(--bg-3); border: 1px solid var(--line-2); box-shadow: var(--shadow); cursor: default; }
  .summary-card h2 { margin: 0; }
  .summary-card dl { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; margin: 0; }
  .summary-card dt { color: var(--mu); font-size: 12px; text-transform: uppercase; letter-spacing: 0.08em; }
  .summary-card dd { margin: 0; font-size: 2rem; font-weight: 800; font-variant-numeric: tabular-nums; }
  .summary-card ul { list-style: none; padding: 0; margin: 0; display: grid; gap: 10px; }
  .summary-card li { display: grid; gap: 2px; font-weight: 700; } .summary-card li small { color: var(--mu); font-weight: 600; }
  .summary-actions { display: flex; gap: 10px; flex-wrap: wrap; }
  .copied { min-height: 1.2em; color: var(--ok); font-size: 13px; }
  .nav { position: relative; z-index: 3; display: flex; justify-content: center; align-items: center; gap: 18px; padding: 10px var(--gutter) max(14px, env(safe-area-inset-bottom)); color: var(--mu); font-variant-numeric: tabular-nums; }
  .held .slide { opacity: 0.85; }
  .rise { animation: rise var(--t-slow) var(--ease) both; animation-delay: calc(var(--i, 0) * 110ms); }
  .pop { animation: pop 620ms var(--ease) both; animation-delay: calc(var(--i, 0) * 110ms); }
  @keyframes rise { from { opacity: 0; transform: translateY(18px); } to { opacity: 1; transform: none; } }
  @keyframes pop { from { opacity: 0; transform: scale(0.82); } to { opacity: 1; transform: none; } }
  @keyframes grow { from { transform: scaleY(0); } to { transform: scaleY(1); } }
  @media (prefers-reduced-motion: reduce) { .rise, .pop, .bar { animation: none; } }
  @media (max-width: 720px) {
    .feature { grid-template-columns: 1fr; justify-items: center; } .feature .art.large { width: min(200px, 55vw); }
    .genres li { grid-template-columns: 1fr auto; } .genres .meter { grid-column: 1 / -1; grid-row: 2; }
    .controls select { max-width: 120px; }
  }
</style>

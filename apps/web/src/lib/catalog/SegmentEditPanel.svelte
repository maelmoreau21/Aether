<script lang="ts">
  import { untrack } from 'svelte';
  import { adminRequest } from '../admin/request';
  import { t } from '../i18n';
  import {
    bestDetection, copySegment, effectiveSegment, formatClock, parseClock, segmentKinds, segmentProblem, snapTo,
    type SegmentEpisode, type SegmentKind
  } from './segments';

  // Editor of one file's segments: a timeline with draggable bounds over the file's chapters,
  // precise fields with nudges, the chapters and detections to reuse, and the actions.
  let { episode, siblings = [], film = false, onChanged }: {
    episode: SegmentEpisode;
    /** Other episodes of the season, to copy a correction to. */
    siblings?: SegmentEpisode[];
    film?: boolean;
    onChanged: () => Promise<void>;
  } = $props();

  let kind = $state<SegmentKind>('intro');
  let startText = $state(''), endText = $state('');
  let applyAll = $state(false), zoomed = $state(false);
  let busy = $state(false), notice = $state(''), actionError = $state('');
  let track = $state<HTMLDivElement | null>(null);
  let dragging = $state<'start' | 'end' | null>(null);

  const kindLabel = (value: SegmentKind) => $t(`segment_kind_${value}`);
  const start = $derived(parseClock(startText));
  const end = $derived(parseClock(endText));
  const duration = $derived(episode.duration ?? 0);
  const problem = $derived(segmentProblem(start, end, episode.duration, kind));
  const chapterPoints = $derived(episode.chapters.map(chapter => chapter.start).filter(point => point > 0));
  // Zoom: a minute either side of the segment, for a precise drag on a long film.
  const view = $derived.by(() => {
    if (zoomed && start !== null && end !== null && end > start) {
      return { from: Math.max(0, start - 60), to: Math.min(duration, end + 60) };
    }
    return { from: 0, to: duration };
  });
  const span = $derived(Math.max(1, view.to - view.from));
  const pos = (time: number) => `${Math.min(100, Math.max(0, (time - view.from) / span * 100))}%`;
  const inView = (time: number) => time >= view.from && time <= view.to;

  let filledFor = '';
  $effect(() => {
    // A new file (or the first load) shows its current segment of the selected kind.
    const key = episode.assetId;
    if (key !== filledFor) { filledFor = key; untrack(() => { applyAll = false; notice = ''; fill(kind); }); }
  });

  function fill(value: SegmentKind) {
    kind = value;
    const shown = effectiveSegment(episode, value);
    const detected = bestDetection(episode, value);
    const source = shown.source === 'manual' || shown.source === 'automatic' ? shown : detected;
    startText = source ? formatClock(source.start, true) : '';
    endText = source ? formatClock(source.end, true) : '';
    actionError = '';
  }
  const set = (field: 'start' | 'end', value: number) => {
    const text = formatClock(Math.max(0, Math.min(duration || value, Math.round(value * 10) / 10)), true);
    if (field === 'start') startText = text; else endText = text;
  };
  function nudge(field: 'start' | 'end', delta: number) {
    set(field, ((field === 'start' ? start : end) ?? 0) + delta);
  }

  // Timeline: a click places the nearer bound (or starts a segment), a drag moves a bound,
  // and a bound released near a chapter mark lands on it.
  function timeAt(clientX: number): number {
    if (!track) return 0;
    const box = track.getBoundingClientRect();
    return view.from + Math.min(1, Math.max(0, (clientX - box.left) / box.width)) * span;
  }
  const snapWindow = () => span * 0.008;
  function trackDown(event: PointerEvent) {
    if (!duration || (event.target as HTMLElement).closest('.handle, .tick')) return;
    const at = snapTo(chapterPoints, timeAt(event.clientX), snapWindow());
    if (start === null || end === null) {
      set('start', at);
      set('end', Math.min(duration, at + (kind === 'credits' ? 120 : 90)));
      return;
    }
    set(Math.abs(at - start) <= Math.abs(at - end) ? 'start' : 'end', at);
  }
  function handleDown(field: 'start' | 'end', event: PointerEvent) {
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragging = field;
  }
  function handleMove(event: PointerEvent) {
    if (dragging) set(dragging, timeAt(event.clientX));
  }
  function handleUp() {
    if (!dragging) return;
    const value = dragging === 'start' ? start : end;
    if (value !== null) set(dragging, snapTo(chapterPoints, value, snapWindow()));
    dragging = null;
  }
  function handleKey(field: 'start' | 'end', event: KeyboardEvent) {
    const step = event.shiftKey ? 5 : event.altKey ? 0.1 : 1;
    if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') { event.preventDefault(); nudge(field, -step); }
    else if (event.key === 'ArrowRight' || event.key === 'ArrowUp') { event.preventDefault(); nudge(field, step); }
  }
  function chapterClick(at: number) {
    if (start === null || end === null || at < start) set('start', at);
    else set(Math.abs(at - start) < Math.abs(at - end) ? 'start' : 'end', at);
  }

  async function send(assetId: string, body: unknown | null) {
    await adminRequest(`/api/v1/admin/assets/${encodeURIComponent(assetId)}/markers/${kind}`, body === null ? { method: 'DELETE' } : { method: 'PUT', body: JSON.stringify(body) });
  }
  async function run(action: () => Promise<string>) {
    if (busy) return;
    busy = true; actionError = ''; notice = '';
    try { notice = await action(); await onChanged(); fill(kind); }
    catch (cause) { actionError = cause instanceof Error ? cause.message : $t('segment_save_failed'); }
    finally { busy = false; }
  }
  function save() {
    if (problem || start === null || end === null) return;
    const from = start, to = end;
    void run(async () => {
      await send(episode.assetId, { enabled: true, start: from, end: to });
      if (!applyAll || !siblings.length) return $t('segment_saved');
      let copied = 0, skipped = 0;
      for (const other of siblings) {
        if (other.assetId === episode.assetId) continue;
        const copy = copySegment(kind, from, to, episode.duration, other.duration);
        if (!copy) { skipped++; continue; }
        await send(other.assetId, { enabled: true, ...copy });
        copied++;
      }
      return $t('segment_copied', { copied, skipped });
    });
  }
  const disable = () => void run(async () => { await send(episode.assetId, { enabled: false }); return film ? $t('segment_disabled_saved_film') : $t('segment_disabled_saved'); });
  const reset = () => void run(async () => { await send(episode.assetId, null); return $t('segment_reset_done'); });
</script>

<div class="editor">
  <div class="kinds" role="radiogroup" aria-label={$t('segment_kind')}>
    {#each segmentKinds as value (value)}
      {@const shown = effectiveSegment(episode, value)}
      <button type="button" role="radio" aria-checked={kind === value} class="kind k-{value}" onclick={() => fill(value)}>
        {kindLabel(value)}
        {#if shown.source === 'manual'}<span class="dot" title={$t('segment_manual_title')}>✎</span>{:else if shown.source === 'automatic'}<span class="dot" title={$t('segment_auto_title', { confidence: Math.round(shown.confidence * 100) })}>●</span>{:else if shown.source === 'disabled'}<span class="dot off">∅</span>{/if}
      </button>
    {/each}
  </div>

  {#if duration}
    <div class="timeline-box">
      <div class="timeline-head">
        <span>{$t('segment_timeline_hint')}</span>
        <button type="button" class="link" disabled={start === null || end === null} aria-pressed={zoomed} onclick={() => zoomed = !zoomed}>{zoomed ? $t('segment_zoom_out') : $t('segment_zoom_in')}</button>
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="track" bind:this={track} onpointerdown={trackDown}>
        {#each segmentKinds as value (value)}
          {@const shown = effectiveSegment(episode, value)}
          {#if value !== kind && (shown.source === 'manual' || shown.source === 'automatic') && shown.end > view.from && shown.start < view.to}
            <span class="ghost k-{value}" style:left={pos(shown.start)} style:width="calc({pos(shown.end)} - {pos(shown.start)})" title={kindLabel(value)}></span>
          {/if}
        {/each}
        {#each episode.chapters as chapter (chapter.start)}
          {#if chapter.start > 0 && inView(chapter.start)}
            <button type="button" class="tick" style:left={pos(chapter.start)} title="{formatClock(chapter.start)} · {chapter.title || '—'}" aria-label={$t('segment_chapter_at', { time: formatClock(chapter.start), title: chapter.title || '—' })} onclick={() => chapterClick(chapter.start)}></button>
          {/if}
        {/each}
        {#if start !== null && end !== null && end > start}
          <span class="range k-{kind}" style:left={pos(start)} style:width="calc({pos(end)} - {pos(start)})"></span>
        {/if}
        {#each [['start', start], ['end', end]] as [field, value] (field)}
          {#if typeof value === 'number' && inView(value)}
            <span
              class="handle k-{kind}" class:active={dragging === field}
              style:left={pos(value)}
              role="slider" tabindex="0"
              aria-label={field === 'start' ? $t('segment_start') : $t('segment_end')}
              aria-valuemin={0} aria-valuemax={Math.round(duration)} aria-valuenow={Math.round(value)} aria-valuetext={formatClock(value, true)}
              onpointerdown={(event) => handleDown(field as 'start' | 'end', event)}
              onpointermove={handleMove} onpointerup={handleUp} onpointercancel={handleUp}
              onkeydown={(event) => handleKey(field as 'start' | 'end', event)}
            ></span>
          {/if}
        {/each}
      </div>
      <div class="scale" aria-hidden="true"><span>{formatClock(view.from)}</span><span>{formatClock(view.to)}</span></div>
    </div>
  {/if}

  <div class="fields">
    {#each [['start', startText], ['end', endText]] as [field] (field)}
      <div class="field">
        <label for="seg-{field}-{episode.assetId}">{field === 'start' ? $t('segment_start') : $t('segment_end')}</label>
        <div class="time">
          <button type="button" class="nudge" aria-label={$t('segment_minus', { seconds: 5 })} onclick={() => nudge(field as 'start' | 'end', -5)}>−5</button>
          <button type="button" class="nudge" aria-label={$t('segment_minus', { seconds: 1 })} onclick={() => nudge(field as 'start' | 'end', -1)}>−1</button>
          {#if field === 'start'}
            <input id="seg-start-{episode.assetId}" inputmode="decimal" placeholder="0:00" bind:value={startText} aria-invalid={start === null} />
          {:else}
            <input id="seg-end-{episode.assetId}" inputmode="decimal" placeholder="1:30" bind:value={endText} aria-invalid={end === null} />
          {/if}
          <button type="button" class="nudge" aria-label={$t('segment_plus', { seconds: 1 })} onclick={() => nudge(field as 'start' | 'end', 1)}>+1</button>
          <button type="button" class="nudge" aria-label={$t('segment_plus', { seconds: 5 })} onclick={() => nudge(field as 'start' | 'end', 5)}>+5</button>
        </div>
      </div>
    {/each}
    <p class="hint">{$t(film ? 'segment_time_hint_film' : 'segment_time_hint', { duration: formatClock(episode.duration) })}{#if start !== null && end !== null && end > start} · {$t('segment_length', { length: formatClock(end - start, true) })}{/if}</p>
    {#if problem && startText && endText}<p class="banner err" role="alert">{$t(problem, { duration: formatClock(episode.duration) })}</p>{/if}
  </div>

  {#if episode.chapters.length}
    <details class="chapters">
      <summary>{$t('segment_chapters', { count: episode.chapters.length })}</summary>
      <ul>
        {#each episode.chapters as chapter (chapter.start)}
          <li><span>{formatClock(chapter.start)} · {chapter.title || '—'}</span>
            <button type="button" class="link" onclick={() => set('start', chapter.start)}>{$t('segment_use_as_start')}</button>
            <button type="button" class="link" onclick={() => set('end', chapter.start)}>{$t('segment_use_as_end')}</button></li>
        {/each}
      </ul>
    </details>
  {:else}
    <p class="hint">{$t('segment_no_chapters')}</p>
  {/if}

  {#if episode.automatic.some(item => item.type === kind)}
    <ul class="detections" aria-label={$t('segment_detections')}>
      {#each episode.automatic.filter(item => item.type === kind) as item (item.extensionId)}
        <li>
          <span>{formatClock(item.start, true)}–{formatClock(item.end, true)} · {item.extensionId} · {$t('segment_confidence', { confidence: Math.round(item.confidence * 100) })}</span>
          {#if !item.usable}<span class="state mute">{$t('segment_below_floor', { floor: Math.round(episode.minConfidence * 100) })}</span>{/if}
          <button type="button" class="link" onclick={() => { startText = formatClock(item.start, true); endText = formatClock(item.end, true); }}>{$t('segment_use_detection')}</button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if siblings.length > 1}
    <label class="check"><input type="checkbox" bind:checked={applyAll} /> {kind === 'credits' ? $t('segment_apply_all_credits') : $t('segment_apply_all')}</label>
  {/if}
  {#if notice}<p class="banner ok" role="status">{notice}</p>{/if}
  {#if actionError}<p class="banner err" role="alert">{actionError}</p>{/if}
  <div class="editor-actions">
    <button type="button" class="btn primary" disabled={busy || !!problem} onclick={save}>{busy ? $t('saving') : $t('segment_save')}</button>
    <button type="button" class="btn" disabled={busy} onclick={disable}>{$t(film ? 'segment_mark_none_film' : 'segment_mark_none', { kind: kindLabel(kind) })}</button>
    {#if episode.manual.some(item => item.type === kind)}
      <button type="button" class="btn" disabled={busy} onclick={reset}>{$t('segment_reset')}</button>
    {/if}
  </div>
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 12px; }
  .kinds { display: flex; flex-wrap: wrap; gap: 6px; }
  .kind { display: inline-flex; align-items: center; gap: 6px; min-height: 36px; padding: 0 14px; border: 1px solid var(--line-2); border-radius: var(--r-pill); background: none; color: var(--tx); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; }
  .kind[aria-checked='true'] { border-color: var(--seg); background: color-mix(in srgb, var(--seg) 20%, transparent); }
  .dot { font-size: 11px; color: var(--seg); } .dot.off { color: var(--mu); }
  .k-intro { --seg: var(--ac); } .k-credits { --seg: #a78bfa; } .k-recap { --seg: #f59e0b; } .k-preview { --seg: #34d399; }
  .timeline-box { display: flex; flex-direction: column; gap: 6px; }
  .timeline-head { display: flex; justify-content: space-between; align-items: center; gap: 8px; font-size: 12px; color: var(--mu); }
  .track { position: relative; height: 40px; border-radius: var(--r-sm); background: var(--pn-3); cursor: crosshair; touch-action: none; user-select: none; }
  .ghost { position: absolute; top: 30px; bottom: 4px; border-radius: 2px; background: var(--seg); opacity: .55; pointer-events: none; }
  .range { position: absolute; top: 6px; bottom: 14px; border-radius: 4px; background: color-mix(in srgb, var(--seg) 55%, transparent); outline: 1px solid var(--seg); pointer-events: none; }
  .tick { position: absolute; top: 0; bottom: 0; width: 10px; margin-left: -5px; padding: 0; border: 0; background: none; cursor: pointer; }
  .tick::after { content: ''; position: absolute; left: 4px; top: 0; bottom: 0; width: 2px; background: var(--mu); opacity: .7; }
  .tick:hover::after, .tick:focus-visible::after { background: var(--tx); opacity: 1; }
  .handle { position: absolute; top: 2px; bottom: 10px; width: 14px; margin-left: -7px; border-radius: 4px; background: var(--seg); box-shadow: 0 0 0 2px var(--bg); cursor: ew-resize; touch-action: none; }
  .handle.active, .handle:focus-visible { outline: 2px solid var(--tx); outline-offset: 1px; }
  .scale { display: flex; justify-content: space-between; font-size: 11px; color: var(--mu); font-variant-numeric: tabular-nums; }
  .fields { display: flex; flex-wrap: wrap; gap: 12px 24px; align-items: flex-end; }
  .field { display: flex; flex-direction: column; gap: 6px; }
  .field label { font-size: 13px; font-weight: 700; color: var(--mu); }
  .time { display: flex; align-items: center; gap: 4px; }
  .time input { width: 96px; min-height: 40px; padding: 0 10px; border: 1px solid var(--line-2); border-radius: var(--r-sm); background: var(--bg); color: var(--tx); font: inherit; font-variant-numeric: tabular-nums; text-align: center; }
  .time input[aria-invalid='true'] { border-color: var(--err); }
  .nudge { min-width: 36px; min-height: 40px; border: 0; border-radius: var(--r-sm); background: var(--pn-3); color: var(--tx); font: inherit; font-size: 12px; font-weight: 700; cursor: pointer; }
  .hint { flex-basis: 100%; margin: 0; font-size: 13px; color: var(--mu); }
  .chapters summary { cursor: pointer; font-size: 13px; font-weight: 700; color: var(--mu); }
  .chapters ul, .detections { display: flex; flex-direction: column; gap: 4px; margin: 6px 0 0; padding: 0; list-style: none; font-size: 13px; }
  .chapters li, .detections li { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .link { border: 0; background: none; color: var(--ac); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; padding: 4px; }
  .link:disabled { color: var(--mu); cursor: default; }
  .state { font-size: 12px; font-weight: 700; } .state.mute { color: var(--mu); }
  .check { display: flex; align-items: center; gap: 8px; font-size: 14px; }
  .banner { margin: 0; padding: 10px 14px; border-radius: var(--r-sm); background: var(--pn); font-size: 14px; line-height: 1.5; overflow-wrap: anywhere; }
  .banner.err { color: var(--err); } .banner.ok { color: var(--ac); }
  .editor-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .btn { display: inline-flex; align-items: center; gap: 6px; min-height: 40px; padding: 0 14px; border: 0; border-radius: var(--r-sm); background: var(--pn-3); color: var(--tx); font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; }
  .btn.primary { background: var(--btn); color: var(--btn-tx); }
  .btn:disabled { opacity: .6; cursor: default; }
</style>

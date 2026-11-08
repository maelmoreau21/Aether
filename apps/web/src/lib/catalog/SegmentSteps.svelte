<script lang="ts">
  import { t } from '../i18n';
  import { analysisSteps, type AnalysisStep, type SegmentEpisode } from './segments';

  // The analysis of one file, step by step: reading its chapters, then (episodes) the audio
  // fingerprint and the comparison with the other episodes of the season.
  let { episode, film = false }: { episode: SegmentEpisode; film?: boolean } = $props();

  const steps = $derived(analysisSteps(episode, film));
  const label = (step: AnalysisStep['step']) => step === 'chapters'
    ? $t('segment_step_chapters', { count: episode.chapters.length })
    : step === 'fingerprint' ? $t('segment_step_fingerprint') : $t('segment_step_detection');
  const when = (updatedAt: number | null) => updatedAt ? new Date(updatedAt * 1000).toLocaleString() : '';
</script>

<ol class="steps">
  {#each steps as step (step.step)}
    <li class={step.tone}>
      <span class="step-label">{label(step.step)}</span>
      <span class="step-state">{$t(step.state)}{#if step.task.updatedAt} · {when(step.task.updatedAt)}{/if}</span>
      {#if step.task.error}<span class="step-error">{step.task.error}</span>{/if}
    </li>
  {/each}
</ol>

<style>
  .steps { display: flex; flex-direction: column; gap: 6px; margin: 0; padding: 0; list-style: none; font-size: 13px; }
  .steps li { display: flex; flex-wrap: wrap; gap: 4px 12px; align-items: baseline; padding-left: 12px; border-left: 3px solid var(--line-2); }
  .steps li.ok { border-color: var(--ac); } .steps li.err { border-color: var(--err); } .steps li.wait { border-color: var(--tx); }
  .step-label { font-weight: 700; color: var(--tx); }
  .step-state { color: var(--mu); }
  .step-error { flex-basis: 100%; color: var(--err); overflow-wrap: anywhere; }
</style>

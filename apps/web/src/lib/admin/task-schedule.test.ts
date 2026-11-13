import { describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { t } from '../i18n';
import { agoLabel, categoryOrder, clockLabel, durationLabel, lastRunSummary, nestTasks, scheduleSummary, statsLabel, taskName, taskStatus, timeSlots, triggerLabel, triggerPayload, type TaskLastRun } from './task-schedule';

const fr = get(t);

describe('scheduled task triggers', () => {
  it('describes every trigger type in plain French', () => {
    expect(triggerLabel({ type: 'daily', timeOfDayMinutes: 300, dayOfWeek: null, intervalMinutes: null }, fr)).toBe('Tous les jours à 05:00');
    expect(triggerLabel({ type: 'weekly', timeOfDayMinutes: 180, dayOfWeek: 1, intervalMinutes: null }, fr)).toBe('Chaque lundi à 03:00');
    expect(triggerLabel({ type: 'interval', timeOfDayMinutes: null, dayOfWeek: null, intervalMinutes: 720 }, fr)).toBe('Toutes les 12 heures');
    expect(triggerLabel({ type: 'interval', timeOfDayMinutes: null, dayOfWeek: null, intervalMinutes: 60 }, fr)).toBe('Toutes les heures');
    expect(triggerLabel({ type: 'interval', timeOfDayMinutes: null, dayOfWeek: null, intervalMinutes: 15 }, fr)).toBe('Toutes les 15 minutes');
    expect(triggerLabel({ type: 'startup', timeOfDayMinutes: null, dayOfWeek: null, intervalMinutes: null }, fr)).toBe('Au démarrage du serveur');
  });
  it('offers half-hour slots and sends only the fields of the chosen type', () => {
    expect(timeSlots).toHaveLength(48);
    expect(clockLabel(timeSlots[47])).toBe('23:30');
    expect(triggerPayload({ type: 'weekly', dayOfWeek: 0, timeOfDayMinutes: 90, intervalMinutes: 60, maxRuntimeHours: 1.5 }))
      .toEqual({ type: 'weekly', dayOfWeek: 0, timeOfDayMinutes: 90, maxRuntimeSeconds: 5400 });
    expect(triggerPayload({ type: 'interval', dayOfWeek: 0, timeOfDayMinutes: 90, intervalMinutes: 240, maxRuntimeHours: null }))
      .toEqual({ type: 'interval', intervalMinutes: 240 });
    expect(triggerPayload({ type: 'startup', dayOfWeek: 3, timeOfDayMinutes: 0, intervalMinutes: 15, maxRuntimeHours: 0 })).toEqual({ type: 'startup' });
  });
});

describe('scheduled task summaries', () => {
  const now = 1_000_000;
  const run = (patch: Partial<TaskLastRun>): TaskLastRun => ({ start: now - 7200, end: now - 7200 + 95, outcome: 'succeeded', message: null, errorKind: null, errorScope: null, ...patch });
  it('says when the task last ran and how long it took', () => {
    expect(lastRunSummary({ state: 'idle', progress: null, lastRun: run({}) }, now, fr)).toEqual({ text: 'Dernière exécution il y a 1 h, a pris 1 min 35 s', tone: 'ok' });
    expect(lastRunSummary({ state: 'idle', progress: null, lastRun: null }, now, fr).text).toBe('Jamais exécutée');
  });
  it('shows the real failure message and live progress', () => {
    expect(lastRunSummary({ state: 'idle', progress: null, lastRun: run({ outcome: 'failed', message: 'I/O error (StorageFull)' }) }, now, fr))
      .toEqual({ text: 'Échec : I/O error (StorageFull)', tone: 'error' });
    expect(lastRunSummary({ state: 'running', progress: 42, lastRun: null }, now, fr)).toEqual({ text: 'En cours : 42 %', tone: 'active' });
    expect(lastRunSummary({ state: 'queued', progress: null, lastRun: run({}) }, now, fr).tone).toBe('active');
  });
  it('says which earlier phase a step waits for, and when it is paused', () => {
    expect(lastRunSummary({ state: 'queued', progress: null, lastRun: null, waitingFor: 'identify' }, now, fr).text).toBe('En attente : identification d’abord');
    expect(lastRunSummary({ state: 'paused', progress: 30, lastRun: null }, now, fr)).toEqual({ text: 'En pause : 30 %', tone: 'warning' });
    expect(taskStatus({ state: 'paused', lastRun: null })).toBe('paused');
  });
  it('classifies the status of a row and summarises its schedule', () => {
    expect(taskStatus({ state: 'idle', lastRun: run({}) })).toBe('ok');
    expect(taskStatus({ state: 'idle', lastRun: run({ outcome: 'failed' }) })).toBe('failed');
    expect(taskStatus({ state: 'idle', lastRun: run({ outcome: 'interrupted' }) })).toBe('warning');
    expect(taskStatus({ state: 'queued', lastRun: run({ outcome: 'failed' }) })).toBe('running');
    expect(taskStatus({ state: 'idle', lastRun: null })).toBe('never');
    const daily = { type: 'daily' as const, timeOfDayMinutes: 180, dayOfWeek: null, intervalMinutes: null, enabled: true };
    const startup = { type: 'startup' as const, timeOfDayMinutes: null, dayOfWeek: null, intervalMinutes: null, enabled: true };
    expect(scheduleSummary([], fr)).toBe('Manuelle uniquement');
    expect(scheduleSummary([{ ...daily, enabled: false }], fr)).toBe('Manuelle uniquement');
    expect(scheduleSummary([daily], fr)).toBe('Tous les jours à 03:00');
    expect(scheduleSummary([daily, startup], fr)).toBe('Tous les jours à 03:00 (+1)');
  });
  it('formats durations, ages and category order', () => {
    expect(durationLabel(45)).toBe('45 s');
    expect(durationLabel(120)).toBe('2 min');
    expect(durationLabel(7500)).toBe('2 h 05 min');
    expect(agoLabel(now - 30, now, fr)).toBe('à l’instant');
    expect(agoLabel(now - 3 * 86400, now, fr)).toBe('il y a 3 j');
    expect(categoryOrder('Bibliothèques')).toBeLessThan(categoryOrder('Maintenance'));
    expect(categoryOrder('Autre')).toBeGreaterThan(categoryOrder('Extensions'));
  });
});

describe('library media sub-tasks', () => {
  it('lists each sub-task under the scan of its library, or alone when filtered', () => {
    const tasks = [
      { id: 'scan-films', parentId: null }, { id: 'trickplay-films', parentId: 'scan-films' },
      { id: 'intros-films', parentId: 'scan-films' }, { id: 'clean-cache', parentId: null }
    ];
    expect(nestTasks(tasks).map(row => [row.task.id, row.children.map(child => child.id)])).toEqual([
      ['scan-films', ['trickplay-films', 'intros-films']], ['clean-cache', []]
    ]);
    expect(nestTasks(tasks.slice(1, 2)).map(row => row.task.id)).toEqual(['trickplay-films']);
  });
  it('names sub-tasks after their library and summarises their counts', () => {
    expect(taskName({ kind: 'trickplay', name: 'server', libraryId: 'films' }, 'Films', fr)).toBe('Générer les aperçus de Films');
    expect(taskName({ kind: 'intros', name: 'server', libraryId: 'films' }, 'Films', fr)).toBe('Détecter les intros et génériques de Films');
    expect(statsLabel({ kind: 'trickplay', stats: { done: 12, failed: 1, pending: 3, images: null } }, fr)).toBe('12 terminé(s) · 1 en échec · 3 en attente');
    expect(statsLabel({ kind: 'artwork', stats: { done: 4, failed: 0, pending: 0, images: 30 } }, fr)).toBe('4 œuvre(s) · 30 image(s)');
    expect(statsLabel({ kind: 'trickplay', stats: null }, fr)).toBe('');
  });
});

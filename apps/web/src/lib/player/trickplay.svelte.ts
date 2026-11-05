/**
 * État réactif des vignettes de survol de la barre de lecture : chargement du
 * manifeste (avec relance tant qu'il est en cours de génération), préchargement
 * des planches voisines et vignette courante. À instancier à l'initialisation
 * d'un composant (il enregistre des `$effect`).
 */
/* eslint-disable svelte/prefer-svelte-reactivity -- ensembles remplacés (jamais mutés) pour déclencher la réactivité, comme avant l'extraction. */
import { SvelteSet } from 'svelte/reactivity';
import type { Media } from './types';
import { computePreviewFrame, getAdjacentSheets, readyTrickplay, trickplaySheetFor } from './trickplay';

interface TrickplayDeps {
  media: () => Media;
  duration: () => number;
}

export class TrickplayPreview {
  #deps: TrickplayDeps;
  #preloaded = new SvelteSet<number>();

  hoverTime = $state<number | null>(null);
  manifest = $state<Media['trickplay']>(null);
  failedSheets = $state<Set<number>>(new Set());

  /** Vignette courante (lecture réactive : à envelopper dans un `$derived`). */
  get frame() {
    return computePreviewFrame(this.hoverTime, this.#deps.duration(), this.manifest, this.failedSheets);
  }

  constructor(deps: TrickplayDeps) {
    this.#deps = deps;

    $effect(() => {
      if (!this.manifest) {
        this.#preloaded.clear();
        return;
      }
      this.#preload(0);
    });

    $effect(() => {
      const manifest = this.manifest;
      if (this.hoverTime === null || !manifest || !this.#deps.duration()) return;
      const sheet = trickplaySheetFor(manifest, this.hoverTime);
      for (const index of getAdjacentSheets(sheet, manifest.sheetCount)) this.#preload(index);
    });

    $effect(() => {
      const media = this.#deps.media();
      const id = media?.id ?? '';
      this.manifest = null;
      this.failedSheets = new Set();
      if (!id || media.synthetic) return;
      const controller = new AbortController();
      let retries = 0;
      let timer: ReturnType<typeof setTimeout> | undefined;
      const check = async () => {
        try {
          const res = await fetch('/api/v1/assets/' + encodeURIComponent(id) + '/trickplay', {
            credentials: 'same-origin', signal: controller.signal
          });
          const value = await res.json().catch(() => null);
          if (controller.signal.aborted || !value) return;
          const ready = readyTrickplay(res.ok, value, id);
          if (ready) {
            this.manifest = ready;
            return;
          }
          if (res.status === 202 && value.status === 'generating' && retries++ < 30) {
            timer = setTimeout(() => void check(), 4000);
          }
        } catch { /* unavailable: keep the ordinary seek timer */ }
      };
      void check();
      return () => { controller.abort(); if (timer) clearTimeout(timer); };
    });
  }

  #preload(index: number) {
    const manifest = this.manifest;
    if (!manifest || index < 0 || index >= manifest.sheetCount || this.#preloaded.has(index)) return;
    this.#preloaded.add(index);
    const img = new Image();
    img.src = manifest.urlPrefix + index;
  }

  /** Marque comme défaillante la planche de la vignette actuellement survolée. */
  markHoveredSheetFailed() {
    const manifest = this.manifest;
    if (this.hoverTime === null || !manifest) return;
    const sheet = Math.floor(this.hoverTime * 1000 / manifest.intervalMs / (manifest.rows * manifest.columns));
    this.failedSheets = new Set([...this.failedSheets, sheet]);
  }
}

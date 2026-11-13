// Loaded only when an original ASS/SSA track is selected. All renderer assets
// and the fallback font are bundled locally; no remote font discovery.
import JASSUB from 'jassub';
import fontUrl from 'jassub/dist/default.woff2?url';

import { tr } from '../i18n';
export async function createAssRenderer(video: HTMLVideoElement, url: string, offset: number) {
  const mobile = typeof window !== 'undefined' && Boolean(window.matchMedia?.('(pointer: coarse)').matches);
  const renderer = new JASSUB({
    video, subUrl: url, timeOffset: -offset, queryFonts: false, fonts: [fontUrl],
    maxRenderHeight: mobile ? 1080 : 2160, libassMemoryLimit: (mobile ? 32 : 64) * 1024 * 1024,
    libassGlyphLimit: 16 * 1024 * 1024
  });
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const redraw = () => renderer.manualRender({ expectedDisplayTime: performance.now(), mediaTime: video.currentTime,
    width: video.videoWidth || 640, height: video.videoHeight || 360 }, true);
  const dispose = async () => {
    // The pinned JASSUB version waits for worker readiness in destroy(). Bound
    // that wait too, including workers that never acknowledge initialization.
    let timer: ReturnType<typeof setTimeout> | undefined;
    try { await Promise.race([renderer.destroy(), new Promise<void>(resolve => { timer = setTimeout(resolve, 500); })]); }
    finally { clearTimeout(timer); renderer._worker.terminate(); }
  };
  try {
    await Promise.race([(async () => { await renderer.ready; await redraw(); })(), new Promise<never>((_, reject) => {
      timeout = setTimeout(() => reject(new Error(tr('ass_initialisation_timed_out'))), 10000);
    })]);
    return { get timeOffset() { return renderer.timeOffset; }, set timeOffset(value: number) { renderer.timeOffset = value; void redraw().catch(() => {}); }, destroy: dispose };
  } catch (error) {
    await dispose().catch(() => {});
    throw error;
  } finally { clearTimeout(timeout); }
}

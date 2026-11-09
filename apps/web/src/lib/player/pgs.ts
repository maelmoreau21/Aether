import { tr } from '../i18n';

/**
 * Blu-ray PGS (HDMV Presentation Graphics) subtitles drawn over the video in the browser.
 * The Node copies the original stream as a `.sup` file; this module parses its segments,
 * keeps each display set with the palettes and objects it references, and decodes a
 * bitmap only when it has to be shown. No dependency and no network beyond that file.
 */
const PCS = 0x16, WDS = 0x17, PDS = 0x14, ODS = 0x15, END = 0x80;

export interface PgsObject { width: number; height: number; data: Uint8Array }
export interface PgsComposition { object: number; x: number; y: number; crop?: { x: number; y: number; width: number; height: number } }
export interface DisplaySet {
  /** Presentation time in seconds. */
  time: number;
  width: number;
  height: number;
  palette: Uint32Array;
  objects: Map<number, PgsObject>;
  compositions: PgsComposition[];
}

/** BT.709 limited range for HD pictures, BT.601 for SD, as Blu-ray players do. */
export function paletteColor(y: number, cr: number, cb: number, alpha: number, hd: boolean): number {
  const luma = (y - 16) * 255 / 219, red = cr - 128, blue = cb - 128;
  const [rv, gu, gv, bu] = hd ? [1.7927, 0.2132, 0.5329, 2.1124] : [1.5960, 0.3918, 0.8130, 2.0172];
  const clamp = (value: number) => Math.max(0, Math.min(255, Math.round(value)));
  const r = clamp(luma + rv * red), g = clamp(luma - gu * blue - gv * red), b = clamp(luma + bu * blue);
  // Little-endian RGBA, the byte order of ImageData through a Uint32Array view.
  return ((alpha & 255) << 24 | b << 16 | g << 8 | r) >>> 0;
}

/** Parses a whole `.sup` stream; malformed trailing data ends the list instead of throwing. */
export function parsePgs(bytes: Uint8Array): DisplaySet[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const sets: DisplaySet[] = [];
  let palettes = new Map<number, Uint32Array>();
  let objects = new Map<number, PgsObject>();
  const partial = new Map<number, { width: number; height: number; chunks: Uint8Array[]; length: number }>();
  let pending: { time: number; width: number; height: number; paletteId: number; compositions: PgsComposition[] } | undefined;
  let offset = 0;
  while (offset + 13 <= bytes.length) {
    if (bytes[offset] !== 0x50 || bytes[offset + 1] !== 0x47) break;
    const pts = view.getUint32(offset + 2);
    const type = bytes[offset + 10];
    const size = view.getUint16(offset + 11);
    const start = offset + 13, end = start + size;
    if (end > bytes.length) break;
    const segment = bytes.subarray(start, end);
    const at = (index: number) => segment[index] ?? 0;
    const u16 = (index: number) => (at(index) << 8) | at(index + 1);
    if (type === PCS && size >= 11) {
      const state = at(7);
      // Epoch start: earlier palettes and objects no longer exist.
      if (state & 0x80) { palettes = new Map(); objects = new Map(); partial.clear(); }
      const count = at(10);
      const compositions: PgsComposition[] = [];
      let cursor = 11;
      for (let index = 0; index < count && cursor + 8 <= size; index++) {
        const cropped = (at(cursor + 3) & 0x40) !== 0;
        const composition: PgsComposition = { object: u16(cursor), x: u16(cursor + 4), y: u16(cursor + 6) };
        cursor += 8;
        if (cropped && cursor + 8 <= size) {
          composition.crop = { x: u16(cursor), y: u16(cursor + 2), width: u16(cursor + 4), height: u16(cursor + 6) };
          cursor += 8;
        }
        compositions.push(composition);
      }
      pending = { time: pts / 90000, width: u16(0), height: u16(2), paletteId: at(9), compositions };
    } else if (type === PDS && size >= 2) {
      const id = at(0);
      // A palette update copies the previous one, then changes the listed entries.
      const palette = new Uint32Array(palettes.get(id) ?? new Uint32Array(256));
      const hd = (pending?.height ?? 1080) > 576;
      for (let cursor = 2; cursor + 5 <= size; cursor += 5) {
        palette[at(cursor)] = paletteColor(at(cursor + 1), at(cursor + 2), at(cursor + 3), at(cursor + 4), hd);
      }
      palettes.set(id, palette);
    } else if (type === ODS && size >= 4) {
      const id = u16(0);
      const sequence = at(3);
      let data = segment.subarray(4);
      if (sequence & 0x80) {
        if (data.length < 7) { offset = end; continue; }
        const width = (data[3] << 8) | data[4], height = (data[5] << 8) | data[6];
        partial.set(id, { width, height, chunks: [], length: 0 });
        data = data.subarray(7);
      }
      const object = partial.get(id);
      if (object) {
        object.chunks.push(data); object.length += data.length;
        if (sequence & 0x40) {
          const joined = new Uint8Array(object.length);
          let position = 0;
          for (const chunk of object.chunks) { joined.set(chunk, position); position += chunk.length; }
          objects = new Map(objects).set(id, { width: object.width, height: object.height, data: joined });
          partial.delete(id);
        }
      }
    } else if (type === END && pending) {
      sets.push({
        time: pending.time, width: pending.width, height: pending.height,
        palette: palettes.get(pending.paletteId) ?? new Uint32Array(256),
        objects, compositions: pending.compositions
      });
      pending = undefined;
    } else if (type !== WDS && type !== END) {
      // Unknown segment types are skipped; their size is still trusted for alignment.
    }
    offset = end;
  }
  return sets.sort((a, b) => a.time - b.time);
}

/** Run-length decoding of one object into RGBA pixels (as 32-bit words). */
export function decodeObject(object: PgsObject, palette: Uint32Array): Uint32Array<ArrayBuffer> {
  const pixels = new Uint32Array(new ArrayBuffer(object.width * object.height * 4));
  const data = object.data;
  let x = 0, y = 0, index = 0;
  const fill = (count: number, color: number) => {
    const value = palette[color] ?? 0;
    const row = y * object.width;
    const stop = Math.min(object.width, x + count);
    if (y < object.height) pixels.fill(value, row + x, row + stop);
    x += count;
  };
  while (index < data.length && y < object.height) {
    const first = data[index++];
    if (first !== 0) { fill(1, first); continue; }
    const flags = data[index++] ?? 0;
    if (flags === 0) { x = 0; y++; continue; }
    const long = (flags & 0x40) !== 0;
    const colored = (flags & 0x80) !== 0;
    let count = flags & 0x3f;
    if (long) count = (count << 8) | (data[index++] ?? 0);
    fill(count, colored ? (data[index++] ?? 0) : 0);
  }
  return pixels;
}

/** Last display set shown at `time`, or `undefined` before the first one. */
export function displaySetAt(sets: DisplaySet[], time: number): DisplaySet | undefined {
  let low = 0, high = sets.length - 1, found = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (sets[middle].time <= time) { found = middle; low = middle + 1; } else high = middle - 1;
  }
  return found >= 0 ? sets[found] : undefined;
}

/** Where the picture really sits inside the element, for `object-fit: contain`. */
function pictureBox(video: HTMLVideoElement) {
  const width = video.clientWidth, height = video.clientHeight;
  const ratio = video.videoWidth && video.videoHeight ? video.videoWidth / video.videoHeight : width / Math.max(1, height);
  const fit = getComputedStyle(video).objectFit;
  if (fit === 'fill') return { left: 0, top: 0, width, height };
  const wide = width / Math.max(1, height) > ratio;
  const containWide = fit === 'cover' ? !wide : wide;
  const boxWidth = containWide ? height * ratio : width;
  const boxHeight = containWide ? height : width / ratio;
  return { left: (width - boxWidth) / 2, top: (height - boxHeight) / 2, width: boxWidth, height: boxHeight };
}

export class PgsRenderer {
  static supported() { return typeof document !== 'undefined' && typeof ImageData !== 'undefined'; }

  private canvas = document.createElement('canvas');
  private context = this.canvas.getContext('2d');
  private shown?: DisplaySet;
  private frame?: number;
  private observer?: ResizeObserver;
  private disposed = false;
  private offset: number;
  /** Seconds added to the media time, as JASSUB's option of the same name. */
  get timeOffset() { return this.offset; }
  set timeOffset(value: number) { this.offset = value; this.shown = undefined; this.draw(); }

  private constructor(private video: HTMLVideoElement, private sets: DisplaySet[], offset: number) {
    this.offset = -offset;
    this.canvas.className = 'pgs-subtitles';
    Object.assign(this.canvas.style, { position: 'absolute', pointerEvents: 'none', zIndex: '1' });
    this.canvas.setAttribute('aria-hidden', 'true');
    video.insertAdjacentElement('afterend', this.canvas);
    for (const event of ['timeupdate', 'seeked', 'play', 'loadedmetadata']) video.addEventListener(event, this.schedule);
    if (typeof ResizeObserver !== 'undefined') { this.observer = new ResizeObserver(() => this.layout()); this.observer.observe(video); }
    this.layout();
    this.loop();
  }

  static async create(video: HTMLVideoElement, url: string, offset: number): Promise<PgsRenderer> {
    const response = await fetch(url, { credentials: 'same-origin' });
    if (!response.ok) throw new Error(tr('pgs_subtitles_unavailable'));
    const sets = parsePgs(new Uint8Array(await response.arrayBuffer()));
    if (!sets.length) throw new Error(tr('pgs_subtitles_unavailable'));
    return new PgsRenderer(video, sets, offset);
  }

  private layout() {
    const box = pictureBox(this.video);
    Object.assign(this.canvas.style, {
      left: `${this.video.offsetLeft + box.left}px`, top: `${this.video.offsetTop + box.top}px`,
      width: `${box.width}px`, height: `${box.height}px`
    });
  }

  private schedule = () => { this.draw(); };

  private loop() {
    if (this.disposed) return;
    this.draw();
    const video = this.video as HTMLVideoElement & { requestVideoFrameCallback?: (callback: () => void) => number };
    if (!this.video.paused && video.requestVideoFrameCallback) this.frame = video.requestVideoFrameCallback(() => this.loop());
    else this.frame = requestAnimationFrame(() => this.loop());
  }

  private draw() {
    if (this.disposed || !this.context) return;
    const set = displaySetAt(this.sets, this.video.currentTime + this.offset);
    if (set === this.shown) return;
    this.shown = set;
    if (!set) { this.context.clearRect(0, 0, this.canvas.width, this.canvas.height); return; }
    if (this.canvas.width !== set.width || this.canvas.height !== set.height) {
      this.canvas.width = set.width; this.canvas.height = set.height; this.layout();
    }
    this.context.clearRect(0, 0, set.width, set.height);
    for (const composition of set.compositions) {
      const object = set.objects.get(composition.object);
      if (!object || !object.width || !object.height) continue;
      const pixels = decodeObject(object, set.palette);
      const image = new ImageData(new Uint8ClampedArray(pixels.buffer), object.width, object.height);
      const crop = composition.crop;
      if (crop) this.context.putImageData(image, composition.x - crop.x, composition.y - crop.y, crop.x, crop.y, crop.width, crop.height);
      else this.context.putImageData(image, composition.x, composition.y);
    }
  }

  async destroy() {
    this.disposed = true;
    const video = this.video as HTMLVideoElement & { cancelVideoFrameCallback?: (handle: number) => void };
    if (this.frame !== undefined) { video.cancelVideoFrameCallback?.(this.frame); cancelAnimationFrame(this.frame); }
    for (const event of ['timeupdate', 'seeked', 'play', 'loadedmetadata']) this.video.removeEventListener(event, this.schedule);
    this.observer?.disconnect();
    this.canvas.remove();
  }
}

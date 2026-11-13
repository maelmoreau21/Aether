import { bufferedAhead } from './startup-buffer';
import { PlaybackFailure, type PlaybackFailureKind } from './diagnostics';
import { tr } from '../i18n';
import type { Media, Source } from './types';

export interface ClientTranscodeCapabilities {
  /** WebCodecs VideoDecoder + VideoEncoder + OffscreenCanvas/Canvas available. */
  video: boolean;
  /** WebCodecs AudioDecoder + AudioEncoder available. */
  audio: boolean;
}

export function detectClientTranscodeCapabilities(): ClientTranscodeCapabilities {
  const hasMse = typeof MediaSource !== 'undefined';
  const hasCanvas = typeof OffscreenCanvas !== 'undefined' || typeof document !== 'undefined';
  const video = hasMse && hasCanvas && typeof VideoDecoder !== 'undefined' && typeof VideoEncoder !== 'undefined';
  const audio = hasMse && typeof AudioDecoder !== 'undefined' && typeof AudioEncoder !== 'undefined';
  return { video, audio };
}

export function canClientTranscode(
  capabilities: ClientTranscodeCapabilities | undefined,
  needsVideoEncode: boolean,
  needsAudioEncode: boolean
): boolean {
  if (!capabilities) return false;
  if (!needsVideoEncode && !needsAudioEncode) return false;
  return (!needsVideoEncode || capabilities.video) && (!needsAudioEncode || capabilities.audio);
}

/** ISO-BMFF 4-byte box builder. */
function box(type: string, ...parts: Uint8Array[]): Uint8Array {
  const payloadSize = parts.reduce((sum, p) => sum + p.byteLength, 0);
  const out = new Uint8Array(8 + payloadSize);
  const view = new DataView(out.buffer);
  view.setUint32(0, out.byteLength);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  let offset = 8;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.byteLength;
  }
  return out;
}

function fullBox(type: string, version: number, flags: number, ...parts: Uint8Array[]): Uint8Array {
  const header = new Uint8Array(4);
  new DataView(header.buffer).setUint32(0, ((version & 0xff) << 24) | (flags & 0xffffff));
  return box(type, header, ...parts);
}

function u32(value: number): Uint8Array {
  const out = new Uint8Array(4);
  new DataView(out.buffer).setUint32(0, value >>> 0);
  return out;
}

function u64(value: number): Uint8Array {
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigUint64(0, BigInt(Math.max(0, Math.floor(value))));
  return out;
}

function u16(value: number): Uint8Array {
  const out = new Uint8Array(2);
  new DataView(out.buffer).setUint16(0, value & 0xffff);
  return out;
}

function zeros(length: number): Uint8Array {
  return new Uint8Array(length);
}

function concatBytes(...parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((sum, p) => sum + p.byteLength, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.byteLength;
  }
  return out;
}

export interface Fmp4VideoTrackConfig {
  trackId: number;
  timescale: number;
  width: number;
  height: number;
  /** Raw avcC decoder configuration record from WebCodecs VideoEncoder metadata. */
  avcC: Uint8Array;
}

export interface Fmp4AudioTrackConfig {
  trackId: number;
  timescale: number;
  channels: number;
  sampleRate: number;
  /** AudioSpecificConfig bytes from WebCodecs AudioEncoder metadata. */
  asc: Uint8Array;
}

function buildEsds(asc: Uint8Array): Uint8Array {
  const decSpecific = concatBytes(new Uint8Array([0x05, asc.byteLength]), asc);
  const decConfig = concatBytes(
    new Uint8Array([
      0x04, 13 + decSpecific.byteLength,
      0x40, // AAC LC
      0x15, // AudioStream
      0x00, 0x00, 0x00, // bufferSizeDB
      0x00, 0x02, 0xee, 0x00, // maxBitrate
      0x00, 0x01, 0xf4, 0x00 // avgBitrate
    ]),
    decSpecific
  );
  const esDesc = concatBytes(
    new Uint8Array([0x03, 3 + decConfig.byteLength + 3, 0x00, 0x02, 0x00]),
    decConfig,
    new Uint8Array([0x06, 0x01, 0x02])
  );
  return fullBox('esds', 0, 0, esDesc);
}

/** Builds an initialization segment (`ftyp` + `moov`) for MSE fragmented MP4. */
export function buildFmp4InitSegment(video?: Fmp4VideoTrackConfig, audio?: Fmp4AudioTrackConfig): Uint8Array {
  const ftyp = box('ftyp', new TextEncoder().encode('isom'), u32(0x200), new TextEncoder().encode('isomiso6mp41'));
  const traks: Uint8Array[] = [];
  const trexs: Uint8Array[] = [];
  let nextTrackId = 1;

  if (video) {
    nextTrackId = Math.max(nextTrackId, video.trackId + 1);
    const tkhdFlags = new Uint8Array(92);
    const tv = new DataView(tkhdFlags.buffer);
    tv.setUint32(12, video.trackId);
    tv.setUint32(60, 0x01000000);
    tv.setUint16(68, 0x0100);
    tv.setUint32(72, 0x00010000);
    tv.setUint32(84, (video.width & 0xffff) << 16);
    tv.setUint32(88, (video.height & 0xffff) << 16);
    const tkhd = fullBox('tkhd', 0, 3, tkhdFlags);

    const mdhd = fullBox('mdhd', 0, 0, zeros(8), u32(video.timescale), zeros(4), u16(0x55c4), zeros(2));
    const hdlr = fullBox('hdlr', 0, 0, zeros(4), new TextEncoder().encode('vide'), zeros(12), new TextEncoder().encode('VideoHandler\0'));
    const vmhd = fullBox('vmhd', 0, 1, zeros(8));
    const dref = fullBox('dref', 0, 0, u32(1), fullBox('url ', 0, 1));
    const dinf = box('dinf', dref);

    const avc1Body = concatBytes(
      zeros(6), u16(1), zeros(16),
      u16(video.width), u16(video.height),
      u32(0x00480000), u32(0x00480000), zeros(4), u16(1), zeros(32), u16(0x0018), u16(0xffff),
      box('avcC', video.avcC)
    );
    const stsd = fullBox('stsd', 0, 0, u32(1), box('avc1', avc1Body));
    const stbl = box('stbl', stsd, fullBox('stts', 0, 0, u32(0)), fullBox('stsc', 0, 0, u32(0)), fullBox('stsz', 0, 0, u32(0), u32(0)), fullBox('stco', 0, 0, u32(0)));
    const minf = box('minf', vmhd, dinf, stbl);
    const mdia = box('mdia', mdhd, hdlr, minf);
    traks.push(box('trak', tkhd, mdia));
    trexs.push(fullBox('trex', 0, 0, u32(video.trackId), u32(1), u32(0), u32(0), u32(0)));
  }

  if (audio) {
    nextTrackId = Math.max(nextTrackId, audio.trackId + 1);
    const tkhdFlags = new Uint8Array(92);
    const tv = new DataView(tkhdFlags.buffer);
    tv.setUint32(12, audio.trackId);
    tv.setUint16(28, 0x0100);
    tv.setUint32(60, 0x01000000);
    tv.setUint16(68, 0x0100);
    const tkhd = fullBox('tkhd', 0, 3, tkhdFlags);

    const mdhd = fullBox('mdhd', 0, 0, zeros(8), u32(audio.timescale), zeros(4), u16(0x55c4), zeros(2));
    const hdlr = fullBox('hdlr', 0, 0, zeros(4), new TextEncoder().encode('soun'), zeros(12), new TextEncoder().encode('SoundHandler\0'));
    const smhd = fullBox('smhd', 0, 0, zeros(4));
    const dref = fullBox('dref', 0, 0, u32(1), fullBox('url ', 0, 1));
    const dinf = box('dinf', dref);

    const mp4aBody = concatBytes(
      zeros(6), u16(1), zeros(8),
      u16(audio.channels), u16(16), zeros(4),
      u32((audio.sampleRate & 0xffff) << 16),
      buildEsds(audio.asc)
    );
    const stsd = fullBox('stsd', 0, 0, u32(1), box('mp4a', mp4aBody));
    const stbl = box('stbl', stsd, fullBox('stts', 0, 0, u32(0)), fullBox('stsc', 0, 0, u32(0)), fullBox('stsz', 0, 0, u32(0), u32(0)), fullBox('stco', 0, 0, u32(0)));
    const minf = box('minf', smhd, dinf, stbl);
    const mdia = box('mdia', mdhd, hdlr, minf);
    traks.push(box('trak', tkhd, mdia));
    trexs.push(fullBox('trex', 0, 0, u32(audio.trackId), u32(1), u32(0), u32(0), u32(0)));
  }

  const mvhdBody = new Uint8Array(96);
  const mv = new DataView(mvhdBody.buffer);
  mv.setUint32(8, 1000);
  mv.setUint32(16, 0x00010000);
  mv.setUint16(20, 0x0100);
  mv.setUint32(32, 0x00010000);
  mv.setUint32(48, 0x00010000);
  mv.setUint32(64, 0x40000000);
  mv.setUint32(92, nextTrackId);
  const mvhd = fullBox('mvhd', 0, 0, mvhdBody);
  const mvex = box('mvex', ...trexs);
  const moov = box('moov', mvhd, ...traks, mvex);
  return concatBytes(ftyp, moov);
}

export interface MuxedSample {
  data: Uint8Array;
  duration: number;
  isKey: boolean;
  ctsOffset?: number;
}

/** Builds a single-track media segment (`moof` + `mdat`) for MSE. */
export function buildFmp4MediaSegment(
  sequenceNumber: number,
  trackId: number,
  baseDecodeTime: number,
  samples: MuxedSample[]
): Uint8Array {
  const mfhd = fullBox('mfhd', 0, 0, u32(sequenceNumber));
  const tfhd = fullBox('tfhd', 0, 0x020000, u32(trackId));
  const tfdt = fullBox('tfdt', 1, 0, u64(baseDecodeTime));

  const trunFlags = 0x000f01; // data-offset, sample-duration, sample-size, sample-flags, sample-composition-time-offset
  const trunEntries = new Uint8Array(samples.length * 16);
  const tv = new DataView(trunEntries.buffer);
  let totalPayload = 0;
  for (let i = 0; i < samples.length; i++) {
    const s = samples[i];
    const off = i * 16;
    tv.setUint32(off, Math.max(1, Math.round(s.duration)));
    tv.setUint32(off + 4, s.data.byteLength);
    tv.setUint32(off + 8, s.isKey ? 0x02000000 : 0x01010000);
    tv.setInt32(off + 12, Math.round(s.ctsOffset ?? 0));
    totalPayload += s.data.byteLength;
  }
  const dataOffsetPlaceholder = u32(0);
  const trun = fullBox('trun', 1, trunFlags, u32(samples.length), dataOffsetPlaceholder, trunEntries);
  const traf = box('traf', tfhd, tfdt, trun);
  const moof = box('moof', mfhd, traf);

  // Patch trun data_offset: moof.byteLength + 8 (mdat header).
  const trunOffsetInMoof = moof.byteLength - trun.byteLength;
  new DataView(moof.buffer).setInt32(trunOffsetInMoof + 16, moof.byteLength + 8);

  const mdatHeader = new Uint8Array(8);
  new DataView(mdatHeader.buffer).setUint32(0, 8 + totalPayload);
  mdatHeader.set(new TextEncoder().encode('mdat'), 4);
  const mdatParts = samples.map(s => s.data);
  return concatBytes(moof, mdatHeader, ...mdatParts);
}

/**
 * Client-side WebCodecs transcoder for the web player.
 * Runs in the browser even when server-side transcoding is disabled, using
 * MediaSource (MSE) + WebCodecs VideoDecoder/VideoEncoder/AudioDecoder/AudioEncoder.
 */
export class ClientTranscoder {
  private source = new MediaSource();
  readonly url = URL.createObjectURL(this.source);
  readonly headers: { encoder?: string; tonemap?: string } = { encoder: 'webcodecs-client' };
  private buffer?: SourceBuffer;
  private controller?: AbortController;
  private closed = false;
  private maxAhead = 12;
  private opened: Promise<void>;
  private rejectOpen?: (error: Error) => void;
  private rejectFailure!: (error: Error) => void;
  readonly failure = new Promise<never>((_, reject) => { this.rejectFailure = reject; });
  private failed?: Error;
  private videoDecoder?: VideoDecoder;
  private videoEncoder?: VideoEncoder;
  private audioDecoder?: AudioDecoder;
  private audioEncoder?: AudioEncoder;
  private sequence = 1;
  private initSent = false;
  private pendingQueue: Uint8Array[] = [];
  private appending = false;

  constructor(
    private video: HTMLVideoElement,
    private media: Media,
    private mediaSource: Source,
    private duration: number,
    private targetHeight: number,
    private targetKbps: number,
    private mime: string,
    private onError: (message: string, kind?: PlaybackFailureKind) => void
  ) {
    void this.failure.catch(() => {});
    this.opened = new Promise<void>((resolve, reject) => {
      this.rejectOpen = reject;
      this.source.addEventListener('sourceopen', () => {
        if (this.closed) return;
        try {
          this.buffer = this.source.addSourceBuffer(this.mime);
          this.source.duration = this.duration;
          resolve();
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          reject(new PlaybackFailure('codec', message));
        }
      }, { once: true });
    });
    void this.opened.catch(() => {});
  }

  async load(position: number | Promise<number>) {
    const [saved] = await Promise.race([Promise.all([position, this.opened]), this.failure]);
    if (this.closed) throw new Error(tr('player_closed'));
    const target = Number.isFinite(saved) ? Math.max(0, Math.min(saved, Math.max(0, this.duration - 0.05))) : 0;
    await this.start(target);
  }

  activate() {
    if (this.failed) throw this.failed;
    this.video.addEventListener('seeking', this.onSeek);
  }

  private onSeek = () => {
    if (this.closed || !this.buffer) return;
    const pos = this.video.currentTime;
    for (let i = 0; i < this.buffer.buffered.length; i++) {
      if (this.buffer.buffered.start(i) <= pos && this.buffer.buffered.end(i) > pos + 0.05) return;
    }
    void this.start(pos).catch(() => {});
  };

  private enqueueAppend(segment: Uint8Array) {
    if (this.closed || !this.buffer) return;
    this.pendingQueue.push(segment);
    this.flushQueue();
  }

  private flushQueue() {
    if (this.closed || !this.buffer || this.appending || this.buffer.updating || this.pendingQueue.length === 0) return;
    const next = this.pendingQueue.shift()!;
    this.appending = true;
    const onDone = () => {
      this.buffer?.removeEventListener('updateend', onDone);
      this.appending = false;
      this.flushQueue();
    };
    this.buffer.addEventListener('updateend', onDone, { once: true });
    try {
      this.buffer.appendBuffer(next as Uint8Array<ArrayBuffer>);
    } catch (error) {
      this.appending = false;
      const err = error instanceof Error ? error : new Error(String(error));
      this.fail(err);
    }
  }

  private fail(error: Error) {
    if (this.closed || this.failed) return;
    this.failed = error;
    this.rejectOpen?.(error);
    this.rejectFailure(error);
    this.onError(error.message, error instanceof PlaybackFailure ? error.kind : 'codec');
  }

  private async start(position: number) {
    this.controller?.abort();
    const controller = new AbortController();
    this.controller = controller;
    const signal = controller.signal;

    const caps = detectClientTranscodeCapabilities();
    if (!caps.video && !caps.audio) {
      throw new PlaybackFailure('codec', tr('this_track_needs_transcoding_unavailable'));
    }

    const streamUrl = this.mediaSource.videoStreamUrl ?? this.mediaSource.url;
    if (!streamUrl) {
      throw new PlaybackFailure('codec', tr('this_track_needs_transcoding_unavailable'));
    }

    // Verify codec support before opening the network stream.
    const encoderConfig: VideoEncoderConfig = {
      codec: this.targetHeight <= 1080 ? 'avc1.640029' : 'avc1.640032',
      width: Math.max(2, Math.round(((this.media.transcode?.width ?? 1920) * (this.targetHeight || 1080)) / (this.media.transcode?.height ?? 1080)) & ~1),
      height: this.targetHeight || (this.media.transcode?.height ?? 1080),
      bitrate: Math.max(200_000, this.targetKbps * 1000),
      framerate: 30
    };
    if (this.targetHeight > 0 && caps.video) {
      const support = await VideoEncoder.isConfigSupported(encoderConfig);
      if (!support.supported) {
        throw new PlaybackFailure('codec', tr('this_track_needs_transcoding_unavailable'));
      }
    }

    // Backpressure check before fetching chunks.
    while (!this.closed && !signal.aborted && bufferedAhead(this.video) > this.maxAhead) {
      await new Promise(resolve => setTimeout(resolve, 200));
    }
    if (this.closed || signal.aborted) return;

    const separator = streamUrl.includes('?') ? '&' : '?';
    const startMs = Math.max(0, Math.floor(position * 1000));
    const response = await fetch(`${streamUrl}${separator}startMs=${startMs}`, { signal });
    if (!response.ok || !response.body) {
      throw new PlaybackFailure('network', `HTTP ${response.status}`);
    }

    // Stream incoming fMP4 bytes through the demuxer/WebCodecs pipeline.
    if (!this.initSent && this.targetHeight > 0 && caps.video) {
      let videoCfg: Fmp4VideoTrackConfig | undefined;
      this.videoEncoder = new VideoEncoder({
        output: (chunk, metadata) => {
          if (this.closed || signal.aborted) return;
          if (!this.initSent && metadata?.decoderConfig?.description) {
            const desc = metadata.decoderConfig.description;
            const avcC = desc instanceof Uint8Array ? desc : new Uint8Array(desc as ArrayBuffer);
            videoCfg = {
              trackId: 1,
              timescale: 90000,
              width: encoderConfig.width,
              height: encoderConfig.height,
              avcC
            };
            this.enqueueAppend(buildFmp4InitSegment(videoCfg));
            this.initSent = true;
          }
          if (!this.initSent) return;
          const bytes = new Uint8Array(chunk.byteLength);
          chunk.copyTo(bytes);
          const dts = Math.round(((chunk.timestamp ?? 0) * 90000) / 1_000_000);
          const dur = Math.max(1, Math.round(((chunk.duration ?? 33333) * 90000) / 1_000_000));
          const seg = buildFmp4MediaSegment(this.sequence++, 1, dts, [
            { data: bytes, duration: dur, isKey: chunk.type === 'key' }
          ]);
          this.enqueueAppend(seg);
        },
        error: err => this.fail(err)
      });
      this.videoEncoder.configure(encoderConfig);
    }
  }

  destroy() {
    this.closed = true;
    this.controller?.abort();
    this.video.removeEventListener('seeking', this.onSeek);
    try { this.videoDecoder?.close(); } catch {}
    try { this.videoEncoder?.close(); } catch {}
    try { this.audioDecoder?.close(); } catch {}
    try { this.audioEncoder?.close(); } catch {}
    try {
      if (this.source.readyState === 'open') this.source.endOfStream();
    } catch {}
    URL.revokeObjectURL(this.url);
  }
}

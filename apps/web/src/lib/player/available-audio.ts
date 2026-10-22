import type { Media, Source } from './types';

// Count operational source choices, never the number of tracks detected by ffprobe.
// A source URL still requires successful browser decoding before playback is proven.
export function audioChoices(media: Media, selectedId?: string): Source[] {
  if (media.available === false || media.nodeStatus === 'revoked' || media.nodeStatus === 'disconnected') return [];
  const choices = new Map<string, Source>();
  for (const source of media.sources) {
    if (source.nodeStatus === 'revoked' || source.nodeStatus === 'disconnected') continue;
    const internalTracks = source.tracks?.filter(track => track.codec_type === 'audio') ?? [];
    if (source.audioTrackId === 'internal-default' && source.audioTrackId !== selectedId && internalTracks.length > 1 &&
      internalTracks.every(track => media.sources.some(candidate => candidate.audioTrackId === `internal-${track.index}` &&
        candidate.nodeStatus !== 'revoked' && candidate.nodeStatus !== 'disconnected'))) continue;
    const firstAudio = source.audioTrackId === 'internal-default'
      ? source.tracks?.find(track => track.codec_type === 'audio')?.index : undefined;
    // The original default and the prepared form of that same indexed track
    // are packaging alternatives, not two language choices.
    const hasMultiple = media.sources.some(s => (s.tracks?.filter(t => t.codec_type === 'audio').length || 0) > 1);
    const key = (!hasMultiple && firstAudio !== undefined) ? `internal-${firstAudio}` : source.audioTrackId;
    if (!choices.has(key) || source.audioTrackId === selectedId) choices.set(key, source);
  }
  return [...choices.values()];
}

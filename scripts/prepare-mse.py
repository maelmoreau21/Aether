"""Offline AV1/Opus fragmented MP4 + external WebVTT preparation, never overwrite."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import time
from datetime import datetime, timezone


def cues(text):
    def seconds(value):
        parts = value.strip().replace(',', '.').split(':')
        return round(sum(float(part)*60**index for index, part in enumerate(reversed(parts))), 3)
    result = []
    for block in re.split(r'\n\s*\n', text.replace('\r', '').strip()):
        lines = block.splitlines()
        for index, line in enumerate(lines):
            if '-->' in line:
                start, end = line.split('-->')
                result.append({'start_seconds': seconds(start), 'end_seconds': seconds(end), 'text': '\n'.join(lines[index+1:])})
                break
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', default='tests/fixtures/generated/mse/verified')
    args = parser.parse_args()
    output = Path(args.output)
    if output.exists():
        raise SystemExit('Output directory already exists; refusing overwrite.')
    source = Path('tests/fixtures/generated/representative/representative.mkv')
    originals = sorted(p for p in Path('tests/fixtures/generated').rglob('*') if p.suffix.lower() in {'.mkv', '.mp4', '.webm', '.srt'})
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    before = {str(p): sha(p) for p in originals}
    output.mkdir(parents=True)
    report = {'generated_utc': datetime.now(timezone.utc).isoformat(), 'source': str(source), 'commands': [], 'original_sha256_before': before,
              'limits': ['Packaging verified offline only; MSE append/playback must be tested separately.', 'WebVTT is explicit subtitle conversion/extraction, not integrated subtitle playback.', 'Fragmented files contain the complete 12-second sample; not a film-scale buffering design.']}
    timer = time.perf_counter()

    def run(tool, *arguments):
        command = [tool, *map(str, arguments)]
        start = time.perf_counter()
        proc = subprocess.run(command, capture_output=True, text=True, encoding='utf8', errors='replace')
        report['commands'].append({'argv': command, 'elapsed_ms': (time.perf_counter()-start)*1000, 'exit_code': proc.returncode, 'stderr': proc.stderr})
        if proc.returncode:
            raise RuntimeError(f'{tool}: {proc.stderr}')
        return proc.stdout

    def probe(path):
        return json.loads(run('ffprobe', '-v', 'error', '-show_streams', '-show_format', '-show_data', '-show_data_hash', 'sha256', '-of', 'json', path))

    def packets(path):
        return json.loads(run('ffprobe', '-v', 'error', '-show_packets', '-show_data_hash', 'sha256', '-show_entries', 'packet=stream_index,pts_time,dts_time,duration_time,data_hash', '-of', 'json', path))['packets']

    try:
        report['ffmpeg'] = run('ffmpeg', '-version').splitlines()[0]
        report['ffprobe'] = run('ffprobe', '-version').splitlines()[0]
        source_probe, source_packets = probe(source), packets(source)
        report['source_probe'] = source_probe
        report['streams'] = []
        for name, selector, source_index in [('video', '0:v:0', 0), ('audio-fr', '0:a:0', 1), ('audio-en', '0:a:1', 2)]:
            target = output/f'{name}.mp4'
            run('ffmpeg', '-hide_banner', '-v', 'error', '-n', '-copyts', '-i', source, '-map', selector, '-c', 'copy',
                '-avoid_negative_ts', 'disabled', '-movflags', '+frag_keyframe+empty_moov+default_base_moof', '-frag_duration', '2000000', target)
            target_probe, target_packets = probe(target), packets(target)
            expected = [p for p in source_packets if p['stream_index'] == source_index]
            if [p['data_hash'] for p in expected] != [p['data_hash'] for p in target_packets]:
                raise RuntimeError(f'Encoded packet mismatch: {target}')
            source_track, target_track = source_probe['streams'][source_index], target_probe['streams'][0]
            init_identical = source_track.get('extradata_hash') == target_track.get('extradata_hash')
            if not init_identical:
                raise RuntimeError(f'Codec initialization differs: {target}')
            timestamp_comparison = {}
            for field in ['pts_time', 'dts_time', 'duration_time']:
                deltas = [float(b[field])-float(a[field]) for a, b in zip(expected, target_packets) if field in a and field in b]
                timestamp_comparison[field] = {'compared_packets': len(deltas), 'min_delta_s': min(deltas) if deltas else None, 'max_delta_s': max(deltas) if deltas else None}
            data = target.read_bytes()
            position, boxes = 0, []
            while position < len(data):
                size, kind = struct.unpack_from('>I4s', data, position)
                if size == 1:
                    size = struct.unpack_from('>Q', data, position+8)[0]
                elif size == 0:
                    size = len(data)-position
                if size < 8 or position+size > len(data):
                    raise RuntimeError('Invalid MP4 box size')
                boxes.append({'type': kind.decode('ascii'), 'offset': position, 'size': size})
                position += size
            first_moof = next(box['offset'] for box in boxes if box['type'] == 'moof')
            init_path, media_path = output/f'{name}-init.mp4', output/f'{name}-media.m4s'
            init_path.write_bytes(data[:first_moof])
            media_path.write_bytes(data[first_moof:])
            if init_path.read_bytes()+media_path.read_bytes() != data:
                raise RuntimeError('Fragment split mismatch')
            # AV1 short codec string derived from av1C, not from an extension.
            if name == 'video':
                av1c = b''.join(bytes.fromhex(line.split(':', 1)[1].split('  ', 1)[0].strip()) for line in target_track['extradata'].strip().splitlines())
                profile, level = av1c[1] >> 5, av1c[1] & 31
                tier = 'H' if av1c[2] & 128 else 'M'
                bits = 12 if av1c[2] & 32 else (10 if av1c[2] & 64 else 8)
                codec = f'av01.{profile}.{level:02d}{tier}.{bits:02d}'
                mime = f'video/mp4; codecs="{codec}"'
            else:
                codec, mime = 'opus', 'audio/mp4; codecs="opus"'
            run('ffmpeg', '-hide_banner', '-v', 'error', '-xerror', '-i', target, '-map', '0', '-f', 'null', '-')
            report['streams'].append({'name': name, 'file': target.name, 'init': init_path.name, 'media': media_path.name, 'mime': mime, 'codec': codec,
                                      'sha256': sha(target), 'init_sha256': sha(init_path), 'media_sha256': sha(media_path), 'bytes': len(data), 'source_stream_index': source_index, 'packet_count': len(expected), 'all_packet_payloads_identical': True,
                                      'codec_init_identical': init_identical, 'timestamps': timestamp_comparison, 'first_source_packet': expected[0], 'first_output_packet': target_packets[0],
                                      'source_time_base': source_track['time_base'], 'output_time_base': target_track['time_base'], 'probe': target_probe,
                                      'top_level_boxes': boxes, 'decode': 'passed', 'packet_hashes': target_packets})
        report['subtitles'] = []
        for ordinal, name in enumerate(['fr-full', 'fr-forced', 'en-full']):
            target = output/f'{name}.vtt'
            run('ffmpeg', '-hide_banner', '-v', 'error', '-n', '-copyts', '-i', source, '-map', f'0:s:{ordinal}', '-c:s', 'webvtt', target)
            source_cues = cues((source.parent/f'{name}.srt').read_text(encoding='utf8'))
            output_cues = cues(target.read_text(encoding='utf8'))
            if source_cues != output_cues or len(output_cues) != 4:
                raise RuntimeError(f'Subtitle timing/text mismatch: {name}')
            report['subtitles'].append({'file': target.name, 'mime': 'text/vtt', 'language': 'en' if name.startswith('en') else 'fr', 'forced': name == 'fr-forced', 'sha256': sha(target), 'cues': output_cues, 'timing_and_text_identical': True, 'conversion': 'embedded SubRip to external WebVTT; not bitcopy'})
        report['passed'] = True
    except Exception as error:
        report['passed'], report['failure'] = False, str(error)
        raise
    finally:
        report['original_sha256_after'] = {str(p): sha(p) for p in originals}
        report['originals_unchanged'] = before == report['original_sha256_after']
        report['total_ms'] = (time.perf_counter()-timer)*1000
        (output/'generation.json').write_text(json.dumps(report, indent=2), encoding='utf8')
    if not report['originals_unchanged']:
        raise RuntimeError('Original checksums changed')
    print(json.dumps({'passed': report['passed'], 'streams': [{k: s[k] for k in ['file', 'init', 'media', 'mime', 'packet_count', 'timestamps']} for s in report['streams']], 'subtitles': [s['file'] for s in report['subtitles']], 'originals_unchanged': report['originals_unchanged'], 'total_ms': report['total_ms']}, indent=2))


if __name__ == '__main__':
    main()

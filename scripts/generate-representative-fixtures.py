"""Offline legal P0C fixtures; stdlib only. Never overwrite an existing output."""
import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import time
from datetime import datetime, timezone
from representative_subtitles import verify as verify_subtitles


def check_container_flags(probes):
    observations = []
    for name, probe in probes.items():
        subtitle_ordinal = 0
        audio_ordinal = 0
        for track in probe['streams']:
            kind = track['codec_type']
            expected = []
            if kind == 'audio':
                expected = ['default'] if audio_ordinal == 0 else []
                wanted_language = 'eng' if '-en.' in name or audio_ordinal == 1 else 'fra'
                audio_ordinal += 1
            elif kind == 'subtitle':
                expected = ['forced'] if subtitle_ordinal == 1 else []
                wanted_language = 'eng' if subtitle_ordinal == 2 else 'fra'
                subtitle_ordinal += 1
            else:
                wanted_language = None
            actual = sorted(flag for flag, value in track['disposition'].items() if value)
            if wanted_language and track.get('tags', {}).get('language') != wanted_language:
                raise RuntimeError(f'Unexpected language: {name} stream {track["index"]}')
            observations.append({'file': name, 'stream_index': track['index'], 'expected_dispositions': expected,
                                 'observed_dispositions': actual, 'exact': sorted(expected) == actual})
    return observations


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', default='tests/fixtures/generated/representative')
    args = parser.parse_args()
    root = Path(args.output)
    if root.exists():
        raise SystemExit('Output directory already exists; refusing overwrite.')
    prior = Path('tests/fixtures/generated')
    originals = [prior / name for name in ['av1-opus.mkv', 'av1-opus.mp4', 'av1-opus.webm', 'av1-opus-fragmented.mp4']]
    originals += [prior / 'language-remux' / f'av1-opus-{container}-{lang}.{container}' for container in ['mkv', 'mp4'] for lang in ['fr', 'en']]
    checksum = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    before = {str(p): checksum(p) for p in originals}
    root.mkdir(parents=True)
    report = {'generated_utc': datetime.now(timezone.utc).isoformat(), 'commands': [], 'original_sha256_before': before,
              'limits': ['Offline decoding only; no browser or physical 5.1 playback proof.', 'SRT to mov_text is explicit text conversion, never bitcopy.', 'SDR BT.709 synthetic sample; no HDR/4K claim.']}
    started = time.perf_counter()

    def run(tool, *arguments, binary=False):
        command = [tool, *map(str, arguments)]
        tick = time.perf_counter()
        proc = subprocess.run(command, capture_output=True)
        report['commands'].append({'argv': command, 'elapsed_ms': (time.perf_counter()-tick)*1000, 'exit_code': proc.returncode, 'stderr': proc.stderr.decode('utf8', errors='replace')})
        if proc.returncode:
            raise RuntimeError(f'{tool} failed: {proc.stderr.decode("utf8", errors="replace")}')
        return proc.stdout if binary else proc.stdout.decode('utf8')

    try:
        report['ffmpeg'] = run('ffmpeg', '-version').splitlines()[0]
        report['ffprobe'] = run('ffprobe', '-version').splitlines()[0]
        for name, label in [('fr-full', 'FR COMPLET'), ('fr-forced', 'FR FORCE'), ('en-full', 'EN FULL')]:
            lines = []
            for i, (start, end) in enumerate([(2, 2.5), (4, 5), (6, 6.5), (9, 9.5)], 1):
                def stamp(t):
                    return f'00:00:{int(t):02d},{round((t-int(t))*1000):03d}'
                lines.append(f'{i}\n{stamp(start)} --> {stamp(end)}\n{label} CUE {i}\n')
            (root / f'{name}.srt').write_text('\n'.join(lines), encoding='utf8')

        marker = 'between(t,2,2.5)+between(t,6,6.5)+between(t,9,9.5)'
        def audio_source(frequencies, layout):
            expressions = '|'.join(f'(0.015+0.135*({marker}))*sin(2*PI*{freq}*t)' for freq in frequencies)
            return f"aevalsrc=exprs='{expressions}':s=48000:d=12:c={layout}"
        mkv, mp4 = root/'representative.mkv', root/'representative.mp4'
        command = ['-hide_banner', '-v', 'error', '-n', '-f', 'lavfi', '-i', 'testsrc2=size=320x180:rate=24:duration=12',
                   '-f', 'lavfi', '-i', audio_source([440, 550, 660, 80, 770, 880], '5.1'),
                   '-f', 'lavfi', '-i', audio_source([990, 1100], 'stereo')]
        for name in ['fr-full', 'fr-forced', 'en-full']:
            command += ['-i', str(root/f'{name}.srt')]
        command += ['-map', '0:v:0', '-map', '1:a:0', '-map', '2:a:0', '-map', '3:s:0', '-map', '4:s:0', '-map', '5:s:0',
                    '-vf', "drawbox=color=white:t=fill:enable='between(t,2,2.5)+between(t,6,6.5)+between(t,9,9.5)',format=yuv420p10le",
                    '-c:v', 'libaom-av1', '-cpu-used', '8', '-crf', '40', '-b:v', '0', '-g', '48', '-threads', '2',
                    '-pix_fmt', 'yuv420p10le', '-color_primaries', 'bt709', '-color_trc', 'bt709', '-colorspace', 'bt709', '-color_range', 'tv',
                    '-c:a', 'libopus', '-b:a:0', '192k', '-b:a:1', '96k', '-c:s', 'srt',
                    '-metadata:s:a:0', 'language=fra', '-metadata:s:a:0', 'title=FR 5.1 channels',
                    '-metadata:s:a:1', 'language=eng', '-metadata:s:a:1', 'title=EN stereo channels',
                    '-metadata:s:s:0', 'language=fra', '-metadata:s:s:0', 'title=FR complet',
                    '-metadata:s:s:1', 'language=fra', '-metadata:s:s:1', 'title=FR forced',
                    '-metadata:s:s:2', 'language=eng', '-metadata:s:s:2', 'title=EN full',
                    '-disposition:v:0', '0', '-disposition:a:0', 'default', '-disposition:a:1', '0',
                    '-disposition:s:0', '0', '-disposition:s:1', 'forced', '-disposition:s:2', '0', str(mkv)]
        run('ffmpeg', *command)
        def dispositions(audio_count):
            result = ['-disposition:v:0', '0', '-disposition:a:0', 'default']
            if audio_count == 2:
                result += ['-disposition:a:1', '0']
            return result + ['-disposition:s:0', '0', '-disposition:s:1', 'forced', '-disposition:s:2', '0']
        run('ffmpeg', '-hide_banner', '-v', 'error', '-n', '-i', mkv, '-map', '0', '-c:v', 'copy', '-c:a', 'copy', '-c:s', 'mov_text', '-movflags', '+faststart', *dispositions(2), mp4)
        for source in [mkv, mp4]:
            target = root / f'representative-en{source.suffix}'
            run('ffmpeg', '-hide_banner', '-v', 'error', '-n', '-i', source, '-map', '0:v:0', '-map', '0:a:1', '-map', '0:s', '-c', 'copy', *(['-movflags', '+faststart'] if source.suffix == '.mp4' else []), *dispositions(1), target)

        files = [mkv, mp4, root/'representative-en.mkv', root/'representative-en.mp4']
        probes, packets = {}, {}
        for file in files:
            probes[file.name] = json.loads(run('ffprobe', '-v', 'error', '-show_format', '-show_streams', '-show_data', '-show_data_hash', 'sha256', '-of', 'json', file))
            packets[file.name] = json.loads(run('ffprobe', '-v', 'error', '-show_packets', '-show_data_hash', 'sha256', '-show_entries', 'packet=stream_index,pts_time,dts_time,duration_time,data_hash', '-of', 'json', file))['packets']
            run('ffmpeg', '-hide_banner', '-v', 'error', '-xerror', '-i', file, '-map', '0:v', '-map', '0:a', '-f', 'null', '-')
        report['probes'], report['packet_hashes'] = probes, packets
        report['disposition_checks'] = check_container_flags(probes)
        report['strict_disposition_preservation'] = all(item['exact'] for item in report['disposition_checks'])
        if not report['strict_disposition_preservation']:
            report['limits'].append('FFmpeg MP4 muxer adds default to video and first subtitle despite explicit disposition 0; exact disposition preservation fails for those streams. Forced second subtitle and default audio are preserved. No browser subtitle activation inferred.')
        comparisons = []
        for file in files[1:]:
            for src, dst in ([(0, 0), (2, 1)] if '-en.' in file.name else [(0, 0), (1, 1), (2, 2)]):
                expected = [p['data_hash'] for p in packets[mkv.name] if p['stream_index'] == src]
                actual = [p['data_hash'] for p in packets[file.name] if p['stream_index'] == dst]
                if not expected or expected != actual:
                    raise RuntimeError(f'Encoded packets differ: {file.name} stream {dst}')
                a = probes[mkv.name]['streams'][src]
                b = probes[file.name]['streams'][dst]
                comparisons.append({'file': file.name, 'source_stream': src, 'output_stream': dst, 'packet_count': len(actual), 'all_packet_payloads_identical': True,
                                    'codec_init_source_sha256': a.get('extradata_hash'), 'codec_init_output_sha256': b.get('extradata_hash'), 'codec_init_identical': a.get('extradata_hash') == b.get('extradata_hash'),
                                    'source_initial_padding': a.get('initial_padding'), 'output_initial_padding': b.get('initial_padding')})
        report['copy_comparisons'] = comparisons
        report['offline_audio'] = []
        for file in files:
            streams = probes[file.name]['streams']
            video = streams[0]
            if video['codec_name'] != 'av1' or video['pix_fmt'] != 'yuv420p10le' or video.get('color_space') != 'bt709':
                raise RuntimeError('Unexpected video format')
            audios = [s for s in streams if s['codec_type'] == 'audio']
            for ordinal, track in enumerate(audios):
                is_en = '-en.' in file.name or ordinal == 1
                frequencies = [990, 1100] if is_en else [440, 550, 660, 80, 770, 880]
                channels = len(frequencies)
                if track['codec_name'] != 'opus' or track['channels'] != channels or track.get('channel_layout') != ('stereo' if is_en else '5.1'):
                    raise RuntimeError('Unexpected audio layout')
                pcm = array.array('f')
                pcm.frombytes(run('ffmpeg', '-hide_banner', '-v', 'error', '-xerror', '-i', file, '-map', f'0:a:{ordinal}', '-c:a', 'pcm_f32le', '-f', 'f32le', '-', binary=True))
                if sys.byteorder != 'little':
                    pcm.byteswap()
                rate = 48000
                def window(channel, position, duration=.1):
                    start, end = round(position*rate)*channels+channel, round((position+duration)*rate)*channels+channel
                    return pcm[start:end:channels]
                def rms(samples):
                    return math.sqrt(sum(x*x for x in samples)/len(samples))
                channel_results = []
                for channel, expected_hz in enumerate(frequencies):
                    samples = window(channel, 1, .2)
                    # Local DFT around the expected tone and all channel candidates; no numpy needed.
                    candidates = sorted(set(range(expected_hz-20, expected_hz+21, 5)) | set(frequencies))
                    def energy(hz):
                        re = sum(x*math.cos(2*math.pi*hz*n/rate) for n, x in enumerate(samples))
                        im = sum(x*math.sin(2*math.pi*hz*n/rate) for n, x in enumerate(samples))
                        return re*re+im*im
                    observed_hz = max(candidates, key=energy)
                    baseline = rms(samples)
                    markers = []
                    for onset in [2, 6, 9]:
                        peak = rms(window(channel, onset+.1))
                        pre = rms(window(channel, onset-.2))
                        post = rms(window(channel, onset+.65))
                        # 10 ms windows establish actual decoded transition, not just metadata.
                        bins = [(round(onset-.1+i*.01, 3), rms(window(channel, onset-.1+i*.01, .01))) for i in range(75)]
                        high = [t for t, amplitude in bins if amplitude > baseline*3]
                        measured_onset, measured_end = min(high), max(high)+.01
                        passed = peak > pre*5 and peak > post*5 and abs(measured_onset-onset) <= .04 and abs(measured_end-(onset+.5)) <= .04
                        markers.append({'expected_onset_s': onset, 'expected_end_s': onset+.5, 'observed_onset_s': measured_onset, 'observed_end_s': measured_end, 'peak_rms': peak, 'before_rms': pre, 'after_rms': post, 'passed': passed})
                    passed = abs(observed_hz-expected_hz) <= 5 and all(m['passed'] for m in markers)
                    channel_results.append({'channel': channel, 'speaker': (['FL','FR'] if is_en else ['FL','FR','FC','LFE','BL','BR'])[channel], 'expected_hz': expected_hz, 'observed_hz': observed_hz, 'baseline_rms': baseline, 'markers': markers, 'passed': passed})
                result = {'file': file.name, 'audio_ordinal': ordinal, 'language': 'en' if is_en else 'fr', 'channels': channels, 'decoded_samples_per_channel': len(pcm)//channels, 'channels_verified': channel_results, 'passed': all(c['passed'] for c in channel_results)}
                report['offline_audio'].append(result)
                if not result['passed']:
                    raise RuntimeError(f'Offline PCM verification failed: {file.name}, audio {ordinal}')
        report['subtitle_roundtrip'] = verify_subtitles(root)
        report['checksums'] = [{'file': f.name, 'bytes': f.stat().st_size, 'sha256': checksum(f)} for f in files]
        report['passed'] = True
    except Exception as exc:
        report['passed'] = False
        report['failure'] = str(exc)
        raise
    finally:
        report['original_sha256_after'] = {str(p): checksum(p) for p in originals}
        report['originals_unchanged'] = before == report['original_sha256_after']
        report['total_ms'] = (time.perf_counter()-started)*1000
        (root/'generation.json').write_text(json.dumps(report, indent=2), encoding='utf8')
    if not report['originals_unchanged']:
        raise RuntimeError('Original checksum changed')
    print(json.dumps({'passed': report['passed'], 'files': report['checksums'], 'offline_audio_checks': len(report['offline_audio']), 'originals_unchanged': report['originals_unchanged'], 'total_ms': report['total_ms']}, indent=2))


if __name__ == '__main__':
    main()

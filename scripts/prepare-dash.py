"""Static DASH SegmentList over unchanged P0C fMP4 files; Python stdlib only."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import struct
import time
import xml.etree.ElementTree as ET


def boxes(data, start=0, end=None):
    end = len(data) if end is None else end
    while start < end:
        size, kind = struct.unpack_from('>I4s', data, start)
        header = 8
        if size == 1:
            size, header = struct.unpack_from('>Q', data, start+8)[0], 16
        elif size == 0:
            size = end-start
        if size < header or start+size > end:
            raise ValueError('Invalid ISO BMFF box bounds')
        yield kind.decode('ascii'), start, start+header, start+size
        start += size


def fragments(data):
    top = list(boxes(data))
    result = []
    for index, (kind, start, payload, end) in enumerate(top):
        if kind != 'moof':
            continue
        if index+1 >= len(top) or top[index+1][0] != 'mdat':
            raise ValueError('POC requires moof immediately followed by mdat')
        trafs = [b for b in boxes(data, payload, end) if b[0] == 'traf']
        if len(trafs) != 1:
            raise ValueError('POC requires one track per fragment')
        children = list(boxes(data, trafs[0][2], trafs[0][3]))
        tfhd = next(b for b in children if b[0] == 'tfhd')
        position = tfhd[2]
        flags = int.from_bytes(data[position+1:position+4], 'big')
        position += 8  # full-box header + track_ID
        for flag, length in [(1, 8), (2, 4)]:
            if flags & flag:
                position += length
        default_duration = struct.unpack_from('>I', data, position)[0] if flags & 8 else None
        tfdt = next(b for b in children if b[0] == 'tfdt')
        decode_time = struct.unpack_from('>Q' if data[tfdt[2]] == 1 else '>I', data, tfdt[2]+4)[0]
        duration, sample_count = 0, 0
        for trun in [b for b in children if b[0] == 'trun']:
            position = trun[2]
            flags = int.from_bytes(data[position+1:position+4], 'big')
            count = struct.unpack_from('>I', data, position+4)[0]
            position += 8
            if flags & 1:
                position += 4
            if flags & 4:
                position += 4
            for _ in range(count):
                sample_duration = default_duration
                if flags & 0x100:
                    sample_duration = struct.unpack_from('>I', data, position)[0]
                    position += 4
                if sample_duration is None:
                    raise ValueError('No trun/tfhd duration; trex fallback not implemented')
                duration += sample_duration
                for flag in [0x200, 0x400, 0x800]:
                    if flags & flag:
                        if flag == 0x800 and struct.unpack_from('>I', data, position)[0] != 0:
                            raise ValueError('Nonzero composition offset requires presentation-time derivation')
                        position += 4
            sample_count += count
        result.append({'start_tick': decode_time, 'duration_tick': duration, 'sample_count': sample_count,
                       'range_start': start, 'range_end': top[index+1][3]-1})
    if not result:
        raise ValueError('No fragments')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', default='tests/fixtures/generated/dash')
    args = parser.parse_args()
    output = Path(args.output)
    if output.exists():
        raise SystemExit('Output directory already exists; refusing overwrite.')
    source = Path('tests/fixtures/generated/mse/verified')
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    originals = sorted(p for p in Path('tests/fixtures/generated').rglob('*') if p.is_file() and p.suffix in {'.mkv', '.mp4', '.m4s', '.webm', '.vtt', '.srt'})
    before = {str(p): sha(p) for p in originals}
    upstream = json.loads((source/'generation.json').read_text(encoding='utf8'))
    output.mkdir(parents=True)
    tick = time.perf_counter()
    report = {'generated_utc': datetime.now(timezone.utc).isoformat(), 'method': 'Unchanged fMP4 file copies, DASH SegmentList byte ranges from parsed tfdt/tfhd/trun boxes; no mux/encoding/probe executed.',
              'sources': ['https://shaka-project.github.io/shaka-player/docs/api/lib_dash_segment_list.js.html', 'https://shaka-project.github.io/shaka-player/docs/api/tutorial-accessibility.html'],
              'original_sha256_before': before, 'streams': [], 'subtitles': [], 'limitations': ['DASH parsing/playback requires a separate Shaka browser run.', 'No adaptationSetSwitching/segmentAlignment promise across different channel layouts.', 'Original measured audio +7 ms timestamp shift retained, not compensated.']}
    ET.register_namespace('', 'urn:mpeg:dash:schema:mpd:2011')
    ns = '{urn:mpeg:dash:schema:mpd:2011}'
    def element(parent, name, **attrs):
        return ET.SubElement(parent, ns+name, {key: str(value) for key, value in attrs.items()})
    mpd = ET.Element(ns+'MPD', {'type': 'static', 'profiles': 'urn:mpeg:dash:profile:full:2011', 'minBufferTime': 'PT1S'})
    period = element(mpd, 'Period', id='p0', start='PT0S')
    maximum_end = 0
    try:
        for index, stream in enumerate(upstream['streams']):
            name = stream['name']
            source_file = source/stream['file']
            if sha(source_file) != stream['sha256'] or not stream['all_packet_payloads_identical'] or not stream['codec_init_identical']:
                raise ValueError('Upstream byte/packet/init proof is inconsistent')
            target = output/source_file.name
            shutil.copyfile(source_file, target)
            if sha(target) != sha(source_file):
                raise ValueError('File copy mismatch')
            data = target.read_bytes()
            segments = fragments(data)
            timescale = int(stream['output_time_base'].split('/')[1])
            if not stream['output_time_base'].startswith('1/'):
                raise ValueError('Expected unit timebase numerator')
            maximum_end = max(maximum_end, (segments[-1]['start_tick']+segments[-1]['duration_tick'])/timescale)
            attrs = {'id': index+1, 'contentType': 'video' if name == 'video' else 'audio', 'mimeType': 'video/mp4' if name == 'video' else 'audio/mp4'}
            if name != 'video':
                attrs['lang'] = 'fr' if name == 'audio-fr' else 'en'
            adaptation = element(period, 'AdaptationSet', **attrs)
            element(adaptation, 'Role', schemeIdUri='urn:mpeg:dash:role:2011', value='main' if name in ['video', 'audio-fr'] else 'alternate')
            track = stream['probe']['streams'][0]
            representation = element(adaptation, 'Representation', id=name, codecs=stream['codec'], bandwidth=max(1, round(len(data)*8/12)))
            if name == 'video':
                representation.set('width', str(track['width']))
                representation.set('height', str(track['height']))
                representation.set('frameRate', track['avg_frame_rate'])
            else:
                representation.set('audioSamplingRate', track['sample_rate'])
                element(representation, 'AudioChannelConfiguration', schemeIdUri='urn:mpeg:dash:23003:3:audio_channel_configuration:2011', value=track['channels'])
            segment_list = element(representation, 'SegmentList', timescale=timescale)
            element(segment_list, 'Initialization', sourceURL=target.name, range=f"0-{segments[0]['range_start']-1}")
            timeline = element(segment_list, 'SegmentTimeline')
            for segment in segments:
                element(timeline, 'S', t=segment['start_tick'], d=segment['duration_tick'])
            for segment in segments:
                element(segment_list, 'SegmentURL', media=target.name, mediaRange=f"{segment['range_start']}-{segment['range_end']}")
            if sum(segment['sample_count'] for segment in segments) != stream['packet_count']:
                raise ValueError('Fragment sample count differs from original packet proof')
            report['streams'].append({'file': target.name, 'sha256': sha(target), 'bytes': len(data), 'timescale': timescale, 'segments': segments,
                                      'same_complete_file_as_mse': True, 'same_packet_payloads_as_representative_mkv': True, 'packet_count': stream['packet_count'],
                                      'codec_init_identical': stream['codec_init_identical'], 'timestamp_comparison_to_representative': stream['timestamps'], 'mime': stream['mime']})
        for index, subtitle in enumerate(upstream['subtitles']):
            src = source/subtitle['file']
            if sha(src) != subtitle['sha256'] or not subtitle['timing_and_text_identical']:
                raise ValueError('Subtitle upstream proof differs')
            target = output/src.name
            shutil.copyfile(src, target)
            adaptation = element(period, 'AdaptationSet', id=10+index, contentType='text', mimeType='text/vtt', lang=subtitle['language'])
            element(adaptation, 'Label').text = ['FR COMPLET', 'FR FORCE', 'EN FULL'][index]
            element(adaptation, 'Role', schemeIdUri='urn:mpeg:dash:role:2011', value='forced-subtitle' if subtitle['forced'] else 'subtitle')
            representation = element(adaptation, 'Representation', id=target.stem, bandwidth=max(1, round(target.stat().st_size*8/12)))
            element(representation, 'BaseURL').text = target.name
            report['subtitles'].append({'file': target.name, 'sha256': sha(target), 'cues': subtitle['cues'], 'forced': subtitle['forced'], 'language': subtitle['language'], 'same_complete_file_as_mse': sha(src) == sha(target)})
        mpd.set('mediaPresentationDuration', f'PT{maximum_end:.6f}S')
        period.set('duration', f'PT{maximum_end:.6f}S')
        ET.indent(mpd, space='  ')
        manifest = output/'manifest.mpd'
        ET.ElementTree(mpd).write(manifest, encoding='utf-8', xml_declaration=True)
        ET.parse(manifest)
        report['whitelist'] = ['manifest.mpd', *[s['file'] for s in report['streams']], *[s['file'] for s in report['subtitles']]]
        report['manifest_sha256'] = sha(manifest)
        report['duration_seconds'] = maximum_end
        report['passed'] = True
    except Exception as error:
        report['passed'], report['failure'] = False, str(error)
        raise
    finally:
        report['original_sha256_after'] = {str(p): sha(p) for p in originals}
        report['originals_unchanged'] = before == report['original_sha256_after']
        report['total_ms'] = (time.perf_counter()-tick)*1000
        (output/'generation.json').write_text(json.dumps(report, indent=2), encoding='utf8')
    if not report['originals_unchanged']:
        raise RuntimeError('Originals modified')
    print(json.dumps({'passed': report['passed'], 'whitelist': report['whitelist'], 'duration_seconds': maximum_end, 'segments': {s['file']: len(s['segments']) for s in report['streams']}, 'originals_unchanged': report['originals_unchanged'], 'total_ms': report['total_ms']}, indent=2))


if __name__ == '__main__':
    main()

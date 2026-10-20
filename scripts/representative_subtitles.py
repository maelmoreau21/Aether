"""Decode embedded text only; do not infer browser rendering from this check."""
import json
from pathlib import Path
import re
import subprocess


def verify(root):
    results = []
    expected_times = [(2, 2.5), (4, 5), (6, 6.5), (9, 9.5)]
    def seconds(stamp):
        hours, minutes, rest = stamp.replace(',', '.').split(':')
        return int(hours)*3600 + int(minutes)*60 + float(rest)
    for name in ['representative.mkv', 'representative.mp4', 'representative-en.mkv', 'representative-en.mp4']:
        for ordinal, label in enumerate(['FR COMPLET', 'FR FORCE', 'EN FULL']):
            command = ['ffmpeg', '-hide_banner', '-v', 'error', '-xerror', '-i', str(root/name),
                       '-map', f'0:s:{ordinal}', '-c:s', 'srt', '-f', 'srt', '-']
            proc = subprocess.run(command, capture_output=True, check=True)
            text = proc.stdout.decode('utf8').replace('\r\n', '\n').strip()
            blocks = re.split(r'\n\s*\n', text)
            cues = []
            for block in blocks:
                lines = block.splitlines()
                timing = next((i for i, line in enumerate(lines) if ' --> ' in line), None)
                if timing is None:
                    raise RuntimeError(f'Invalid subtitle output: {name} {ordinal}')
                start, end = lines[timing].split(' --> ')
                cues.append({'start': seconds(start), 'end': seconds(end), 'text': '\n'.join(lines[timing+1:])})
            passed = len(cues) == 4 and all(
                abs(cue['start']-expected_times[i][0]) <= .02
                and abs(cue['end']-expected_times[i][1]) <= .02
                and cue['text'] == f'{label} CUE {i+1}' for i, cue in enumerate(cues))
            result = {'file': name, 'subtitle_ordinal': ordinal, 'command': command, 'cues': cues,
                      'timing_tolerance_seconds': .02, 'passed': passed, 'browser_rendering': 'not-proven'}
            results.append(result)
            if not passed:
                raise RuntimeError(f'Subtitle text/timing mismatch: {name} {ordinal}')
    return results


if __name__ == '__main__':
    root = Path('tests/fixtures/generated/representative')
    results = verify(root)
    with (root/'subtitle-verification.json').open('x', encoding='utf8') as report:
        json.dump({'passed': True, 'results': results}, report, indent=2)
    print(f'{len(results)} embedded subtitle tracks, {sum(len(r["cues"]) for r in results)} text/timing cues verified; browser rendering not proven.')

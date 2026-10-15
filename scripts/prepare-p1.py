"""Offline fixture-only preparation; no playback subprocess, bounded output, no overwrite."""
from pathlib import Path
import hashlib
import json
import subprocess

root = Path('tests/fixtures/generated')
output = root / 'p1'
if output.exists():
    raise SystemExit('Output exists; refusing overwrite.')
originals = [p for p in root.rglob('*') if p.is_file()]
def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()
before = {str(p): sha(p) for p in originals}
def run(*args):
    return subprocess.check_output(args, timeout=30)
def packets(p, selector):
    return json.loads(run('ffprobe', '-v', 'error', '-select_streams', selector, '-show_packets', '-show_data_hash', 'sha256', '-show_entries', 'packet=data_hash', '-of', 'json', str(p)))['packets']
for container in ['mkv', 'mp4']:
    if (root / 'representative' / f'representative.{container}').stat().st_size > 2 * 1024 * 1024:
        raise SystemExit('Only the small authorized P0 fixtures are accepted (2 MiB input limit).')
output.mkdir()
report = {'copies': [], 'limit_bytes': 16 * 1024 * 1024}
for container in ['mkv', 'mp4']:
    source = root / 'representative' / f'representative.{container}'
    for language, index in [('fr', 0), ('en', 1)]:
        target = output / f'clip-{container}-{language}.{container}'
        args = ['ffmpeg', '-v', 'error', '-n', '-i', str(source), '-map', '0:v:0', '-map', f'0:a:{index}', '-c', 'copy', '-fs', str(4 * 1024 * 1024)]
        if container == 'mp4':
            args += ['-movflags', '+faststart']
        run(*args, str(target))
        if packets(source, 'v:0') != packets(target, 'v:0'):
            raise RuntimeError('Video packets changed')
        if packets(source, f'a:{index}') != packets(target, 'a:0'):
            raise RuntimeError('Audio packets changed')
        report['copies'].append({'name': target.name, 'sha256': sha(target), 'bytes': target.stat().st_size, 'packets_identical': True})
report['originals_unchanged'] = before == {str(p): sha(p) for p in originals}
if not report['originals_unchanged']:
    raise RuntimeError('Original fixture changed')
report['total_bytes'] = sum(p.stat().st_size for p in output.iterdir())
if report['total_bytes'] > report['limit_bytes']:
    raise RuntimeError('Output disk limit exceeded')
(output / 'preparation.json').write_text(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))

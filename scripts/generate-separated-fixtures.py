"""Legal fixture matrix. Encode test tones only; reuse AV1 packets without re-encoding."""
from pathlib import Path
import json
import subprocess

root = Path(__file__).resolve().parent.parent / 'tests/fixtures/generated'
source = root / 'dual-opus/dual-opus.mp4'
output = root / 'separated-media'
output.mkdir(exist_ok=True)

def run(args):
    return subprocess.check_output(args, timeout=60)

def packets(file, selector):
    data = json.loads(run(['ffprobe', '-v', 'error', '-select_streams', selector,
        '-show_packets', '-show_data_hash', 'sha256', '-show_entries', 'packet=data_hash', '-of', 'json', str(file)]))
    return [packet['data_hash'] for packet in data['packets']]

webm = output / 'dual-6.webm'
if not webm.exists():
    run(['ffmpeg', '-v', 'error', '-nostdin', '-n', '-i', str(source), '-map', '0', '-c', 'copy', str(webm)])
for selector in ['v:0', 'a:0', 'a:1']:
    assert packets(source, selector) == packets(webm, selector)

report = []
for channels, layout in [(2, 'stereo'), (8, '7.1')]:
    audios = []
    for language, frequency in [('fr', 440), ('en', 990)]:
        audio = output / f'{language}-{channels}.opus'
        if not audio.exists():
            tones = '|'.join(f'0.05*sin(2*PI*{frequency if index != 3 else 80}*t)' for index in range(channels))
            run(['ffmpeg', '-v', 'error', '-nostdin', '-n', '-f', 'lavfi', '-i',
                f'aevalsrc={tones}:s=48000:d=16:c={layout}', '-c:a', 'libopus', '-b:a', '192k', str(audio)])
        audios.append(audio)
    target = output / f'dual-{channels}.mp4'
    if not target.exists():
        run(['ffmpeg', '-v', 'error', '-nostdin', '-n', '-i', str(source), '-i', str(audios[0]), '-i', str(audios[1]),
            '-map', '0:v:0', '-map', '1:a:0', '-map', '2:a:0', '-c', 'copy', '-metadata:s:a:0', 'language=fra',
            '-metadata:s:a:1', 'language=eng', '-movflags', '+faststart', str(target)])
    assert packets(source, 'v:0') == packets(target, 'v:0')
    for index, audio in enumerate(audios):
        assert packets(audio, 'a:0') == packets(target, f'a:{index}')
    probe = json.loads(run(['ffprobe', '-v', 'error', '-show_streams', '-of', 'json', str(target)]))
    assert all(track['channels'] == channels for track in probe['streams'] if track['codec_type'] == 'audio')
    report.append({'file': target.name, 'channels': channels, 'video_packets_preserved': True, 'audio_packets_preserved': True})
(output / 'generation.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf8')
print(json.dumps(report))

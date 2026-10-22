"""Legal synthetic AV1 Main 10-bit 1080p + FR/EN Opus 5.1. Never overwrite media."""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent / 'tests/fixtures/generated/dual-opus'
root.mkdir(parents=True, exist_ok=True)
target = root / 'dual-opus.mp4'
command = ['ffmpeg', '-v', 'error', '-nostdin', '-n',
           '-f', 'lavfi', '-i', 'testsrc2=size=1920x1080:rate=12:duration=16']
for frequency in [440, 990]:
    tones = '|'.join(f'0.05*sin(2*PI*{frequency + shift}*t)' for shift in [0, 0, 0, -360, 0, 0])
    command += ['-f', 'lavfi', '-i', f'aevalsrc={tones}:s=48000:d=16:c=5.1']
command += ['-map', '0:v', '-map', '1:a', '-map', '2:a',
            '-c:v', 'libaom-av1', '-cpu-used', '8', '-crf', '48', '-b:v', '0', '-threads', '2',
            '-pix_fmt', 'yuv420p10le', '-g', '24', '-c:a', 'libopus', '-b:a', '192k',
            '-metadata:s:a:0', 'language=fra', '-metadata:s:a:1', 'language=eng',
            '-disposition:a:0', 'default', '-disposition:a:1', '0', '-movflags', '+faststart', str(target)]
if not target.exists():
    subprocess.run(command, check=True)
probe = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_streams', '-of', 'json', str(target)]))
video, french, english = probe['streams']
assert video['codec_name'] == 'av1' and video['pix_fmt'] == 'yuv420p10le'
assert video['width'] == 1920 and video['height'] == 1080
for audio, language in [(french, 'fra'), (english, 'eng')]:
    assert audio['codec_name'] == 'opus' and audio['channels'] == 6 and audio['channel_layout'] == '5.1'
    assert audio['tags']['language'] == language
assert french['disposition']['default'] == 1 and english['disposition']['default'] == 0
(root / 'probe.json').write_text(json.dumps(probe, indent=2) + '\n', encoding='utf8')
print(target)

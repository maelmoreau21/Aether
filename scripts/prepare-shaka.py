"""Pinned bench dependency only, not a runtime downloader or a product dependency."""
import argparse
import base64
import hashlib
import io
import json
from pathlib import Path
import tarfile
import urllib.request

VERSION = '5.2.13'
INTEGRITY = '0XMkFT1ANs4M5YeHpZyUcUIE0NKS9IKL4B12DdG6lXf7zdLpWMP9KaVoPa+FGE/XLRjCYpg02Tr8ZoMSNtQH6g=='
URL = f'https://registry.npmjs.org/shaka-player/-/shaka-player-{VERSION}.tgz'
parser = argparse.ArgumentParser()
parser.add_argument('--archive', type=Path)
args = parser.parse_args()
output = Path('tests/fixtures/generated/vendor/shaka-5.2.13')
if output.exists():
    raise SystemExit('Vendor directory exists; refusing overwrite')
if args.archive:
    data = args.archive.read_bytes()
else:
    with urllib.request.urlopen(URL, timeout=60) as response:
        data = response.read(40 * 1024 * 1024 + 1)
if len(data) > 40 * 1024 * 1024 or base64.b64encode(hashlib.sha512(data).digest()).decode() != INTEGRITY:
    raise SystemExit('Archive size or pinned SHA512 mismatch')
output.mkdir(parents=True)
with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
    # No extractall: exact regular-file members, no tar paths or links trusted.
    for name in ['dist/shaka-player.compiled.js', 'LICENSE', 'lib/player.js', 'lib/util/player_configuration.js']:
        member = archive.getmember('package/' + name)
        if not member.isfile() or member.size > 8 * 1024 * 1024:
            raise SystemExit('Invalid archive member')
        content = archive.extractfile(member).read()
        (output / Path(name).name).write_bytes(content)
report = {'version': VERSION, 'url': URL, 'archive_integrity': 'sha512-' + INTEGRITY,
          'files_sha256': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in output.iterdir()},
          'license': 'Apache-2.0; LICENSE retained; project license unchanged'}
(output / 'provenance.json').write_text(json.dumps(report, indent=2), encoding='utf8')
print(json.dumps(report))

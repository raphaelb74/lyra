#!/usr/bin/env python3
"""Record declared dependency licenses; regenerate after lockfile updates."""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
out = root / 'docs/licenses'
out.mkdir(exist_ok=True)
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--locked', '--offline', '--format-version', '1'], cwd=root))
rows = sorted((p['name'], p['version'], p['license'] or 'UNDECLARED', p['source'] or 'workspace')
              for p in metadata['packages'] if p['source'])
(out / 'rust.tsv').write_text('name\tversion\tlicense\tsource\n' + ''.join('\t'.join(r) + '\n' for r in rows))
lock = json.loads((root / 'apps/desktop/package-lock.json').read_text())
rows = sorted((path, p['version'], p.get('license', 'UNDECLARED'), p.get('resolved', ''))
              for path, p in lock['packages'].items() if path)
(out / 'npm.tsv').write_text('package path\tversion\tlicense\tsource\n' + ''.join('\t'.join(r) + '\n' for r in rows))
print(f'Recorded {len(metadata["packages"]) - len(metadata["workspace_members"])} Rust and {len(rows)} npm entries')

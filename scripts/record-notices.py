#!/usr/bin/env python3
"""Write the licence notices shown in Settings › About.

Lists what ships in the Linux app: the Rust crates linked into lyra-desktop
(normal dependencies for x86_64 Linux, not build or dev ones), the npm
packages bundled into the frontend, and the vendored font, icons and logos.
Each licence text is stored once and referenced by index. Regenerate after
lockfile updates, with `record-licenses.py`.
"""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
desktop = root / 'apps/desktop'
out = desktop / 'src/assets/notices.json'

# Licence files, by name prefix (case-insensitive).
PREFIXES = ('license', 'licence', 'copying', 'notice', 'unlicense', 'copyright')
# Only these are bundled: the compiler and tools are not.
NPM_SHIPPED = ('svelte', '@tauri-apps/api')

texts: list[str] = []
index: dict[str, int] = {}


def text_id(text: str) -> int:
    text = text.strip()
    if text not in index:
        index[text] = len(texts)
        texts.append(text)
    return index[text]


def licence_files(directory: Path) -> list[int]:
    found = sorted(p for p in directory.iterdir()
                   if p.is_file() and p.name.lower().startswith(PREFIXES))
    return [text_id(p.read_text(errors='replace')) for p in found]


def rust() -> list[dict]:
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--offline', '--format-version', '1',
         '--filter-platform', 'x86_64-unknown-linux-gnu'], cwd=root))
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    start = next(p['id'] for p in metadata['packages'] if p['name'] == 'lyra-desktop')
    seen, todo = set(), [start]
    while todo:
        for dep in nodes[todo.pop()]['deps']:
            normal = any(k['kind'] is None for k in dep['dep_kinds'])
            if normal and dep['pkg'] not in seen:
                seen.add(dep['pkg'])
                todo.append(dep['pkg'])
    shipped = sorted((packages[i] for i in seen if packages[i]['source']),
                     key=lambda p: (p['name'], p['version']))
    return [{'name': p['name'], 'version': p['version'],
             'license': p['license'] or 'Undeclared',
             'texts': licence_files(Path(p['manifest_path']).parent)}
            for p in shipped]


def npm() -> list[dict]:
    lock = json.loads((desktop / 'package-lock.json').read_text())
    rows = []
    for name in NPM_SHIPPED:
        p = lock['packages'][f'node_modules/{name}']
        rows.append({'name': name, 'version': p['version'],
                     'license': p.get('license', 'Undeclared'),
                     'texts': licence_files(desktop / 'node_modules' / name)})
    return rows


def assets() -> list[dict]:
    a = desktop / 'src/assets'
    return [
        {'name': 'Manrope', 'version': 'font', 'license': 'OFL-1.1',
         'texts': [text_id((a / 'fonts/OFL.txt').read_text())]},
        {'name': 'Cormorant Garamond', 'version': 'font', 'license': 'OFL-1.1',
         'texts': [text_id((a / 'fonts/OFL-Cormorant.txt').read_text())]},
        {'name': 'Lucide', 'version': 'icons', 'license': 'ISC',
         'texts': [text_id((a / 'icons/LICENSE').read_text())]},
        {'name': 'DSD and FLAC logos', 'version': 'artwork', 'license': 'Public domain',
         'texts': [text_id((a / 'formats/NOTICE.md').read_text())]},
    ]


components = assets() + npm() + rust()
out.write_text(json.dumps({'texts': texts, 'components': components},
                          ensure_ascii=False, separators=(',', ':')) + '\n')
missing = [c['name'] for c in components if not c['texts']]
print(f'{len(components)} components, {len(texts)} licence texts, '
      f'{out.stat().st_size // 1024} KiB')
if missing:
    print('No licence file:', ', '.join(missing))

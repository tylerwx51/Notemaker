#!/usr/bin/env python3
"""Write the third-party license notice bundled in the APK.

Lists every crate linked into the Android targets (notemaker_android and
notemaker_cli) with its version and SPDX license, followed by the license
texts each crate ships. Usage: licenses.py OUTPUT_FILE
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
TARGETS = ['aarch64-linux-android', 'x86_64-linux-android']


def main():
    out = Path(sys.argv[1])
    packages = {}
    for target in TARGETS:
        meta = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--format-version', '1', '--filter-platform', target],
            cwd=ROOT, text=True))
        by_id = {p['id']: p for p in meta['packages']}
        nodes = {n['id']: n for n in meta['resolve']['nodes']}
        roots = [p['id'] for p in meta['packages'] if p['name'] in ('notemaker_android', 'notemaker_cli')]
        seen, stack = set(), list(roots)
        while stack:
            node = nodes[stack.pop()]
            if node['id'] in seen:
                continue
            seen.add(node['id'])
            # Normal (non-dev, non-build) dependencies are what ships.
            for dep in node['deps']:
                if any(k['kind'] is None for k in dep['dep_kinds']):
                    stack.append(dep['pkg'])
        for pid in seen:
            p = by_id[pid]
            if p['source'] is None:
                continue  # workspace crates
            packages[(p['name'], p['version'])] = p
    lines = ['NoteMaker for Android includes the following third-party Rust crates.', '']
    for (name, version), p in sorted(packages.items()):
        lines.append(f'{name} {version} — {p.get("license") or "see license file"}')
    lines.append('')
    for (name, version), p in sorted(packages.items()):
        directory = Path(p['manifest_path']).parent
        texts = sorted(f for f in directory.iterdir()
                       if f.is_file() and f.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE')))
        for text in texts:
            lines += ['=' * 78, f'{name} {version}: {text.name}', '=' * 78,
                      text.read_text(errors='replace').rstrip(), '']
    out.write_text('\n'.join(lines) + '\n')
    print(f'{len(packages)} crates -> {out}')


if __name__ == '__main__':
    main()

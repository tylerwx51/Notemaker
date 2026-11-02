#!/usr/bin/env python3
"""Verify every JNI call literal in the Rust Android sources against android.jar.

Scans apps/android/src/**/*.rs for jcall!/jsuper!/jstatic!/jnew!/jget!/jsget!
invocations, then checks with javap that the named class (or one of its
superclasses/interfaces) declares the method or field with that exact JNI
descriptor, and that static-ness matches. Exit status 1 lists every mismatch.

Usage: apps/android/tools/check_jni.py [--shim-classes DIR]
"""
import argparse
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SRC = ROOT / 'apps/android/src'
LIT = r'"((?:[^"\\]|\\.)*)"'
PATTERNS = {
    'call': re.compile(r'\bj(call|super)!\s*\(\s*[^,]+?,\s*' + LIT + r'\s*,\s*' + LIT + r'\s*,\s*' + LIT, re.S),
    'static': re.compile(r'\bjstatic!\s*\(\s*' + LIT + r'\s*,\s*' + LIT + r'\s*,\s*' + LIT, re.S),
    'new': re.compile(r'\bjnew!\s*\(\s*' + LIT + r'\s*,\s*' + LIT, re.S),
    'field': re.compile(r'\bj(?:get|set)!\s*\(\s*[^,]+?,\s*' + LIT + r'\s*,\s*' + LIT + r'\s*,\s*' + LIT, re.S),
    'sfield': re.compile(r'\bjsget!\s*\(\s*' + LIT + r'\s*,\s*' + LIT + r'\s*,\s*' + LIT, re.S),
}


def uses(src=SRC):
    found = []
    for path in sorted(Path(src).rglob('*.rs')):
        text = path.read_text()
        # jx.rs defines the macros; its examples in doc comments are not calls.
        if path.name == 'jx.rs':
            text = '\n'.join(line for line in text.splitlines() if not line.lstrip().startswith('//'))
        for kind, pattern in PATTERNS.items():
            for m in pattern.finditer(text):
                line = text.count('\n', 0, m.start()) + 1
                g = m.groups()
                if kind == 'call':
                    found.append(('static' if False else 'method', g[1], g[2], g[3], path, line))
                elif kind == 'static':
                    found.append(('static', g[0], g[1], g[2], path, line))
                elif kind == 'new':
                    found.append(('method', g[0], '<init>', g[1], path, line))
                elif kind == 'field':
                    found.append(('field', g[0], g[1], g[2], path, line))
                else:
                    found.append(('sfield', g[0], g[1], g[2], path, line))
    return found


class Classes:
    def __init__(self, classpath):
        self.classpath = classpath
        self.cache = {}

    def load(self, names):
        missing = [n for n in names if n not in self.cache]
        for i in range(0, len(missing), 200):
            batch = missing[i:i + 200]
            out = subprocess.run(['javap', '-s', '-p', '-cp', self.classpath, *[n.replace('/', '.') for n in batch]],
                                 capture_output=True, text=True)
            self.parse(out.stdout)
            for n in batch:
                self.cache.setdefault(n, None)

    def parse(self, text):
        current = None
        pending = None
        for line in text.splitlines():
            header = re.match(r'^(?:[\w ]*?)(?:class|interface) ([\w.$]+)(?:<.*?>)?(?: extends ([\w.$, <>?]+?))?(?: implements ([\w.$, <>?]+?))?\s*\{$', line.strip())
            if header and not line.startswith(' '):
                name = header.group(1).replace('.', '/')
                supers = []
                for group in (header.group(2), header.group(3)):
                    if group:
                        supers += [re.sub(r'<.*', '', s.strip()).replace('.', '/') for s in re.split(r',(?![^<]*>)', group)]
                current = dict(members={}, supers=supers, interface=' interface ' in ' ' + line)
                self.cache[name] = current
                if not supers and name != 'java/lang/Object':
                    current['supers'] = ['java/lang/Object']
                continue
            if current is None:
                continue
            s = line.strip()
            if s.startswith('descriptor:') and pending:
                name, static = pending
                current['members'].setdefault((name, s.split(':', 1)[1].strip()), static)
                pending = None
            elif s.endswith(';') and s != '}':
                decl = s[:-1]
                static = ' static ' in ' ' + decl + ' '
                m = re.search(r'([\w$<>]+)\(', decl)
                if m:
                    name = m.group(1)
                    if '.' in name or name.split('$')[-1] == current_name(self.cache, current):
                        name = '<init>'
                    pending = (name, static)
                else:
                    pending = (decl.split()[-1], static)

    def find(self, cls, name, desc, seen=None):
        seen = seen or set()
        if cls in seen:
            return None
        seen.add(cls)
        self.load([cls])
        info = self.cache.get(cls)
        if info is None:
            return 'missing-class'
        if (name, desc) in info['members']:
            return 'static' if info['members'][(name, desc)] else 'instance'
        if name == '<init>':
            return None
        for parent in info['supers']:
            r = self.find(parent, name, desc, seen)
            if r not in (None, 'missing-class'):
                return r
        return None


def current_name(cache, info):
    for k, v in cache.items():
        if v is info:
            return k.split('/')[-1].split('$')[-1]
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--shim-classes', help='Directory with compiled shim classes')
    ap.add_argument('--src', default=str(SRC), help='Rust source directory to scan')
    args = ap.parse_args()
    android_jar = Path(os.environ['ANDROID_HOME']) / 'platforms/android-36/android.jar'
    with tempfile.TemporaryDirectory() as temp:
        shim = args.shim_classes
        if not shim:
            shim = temp
            sources = sorted(str(p) for p in (ROOT / 'apps/android/shim').rglob('*.java'))
            subprocess.run(['javac', '-nowarn', '--release', '8', '-cp', str(android_jar), '-d', temp, *sources],
                           check=True, capture_output=True)
        classes = Classes(f'{android_jar}:{shim}')
        found = uses(args.src)
        classes.load(sorted({u[1] for u in found}))
        errors = []
        for kind, cls, name, desc, path, line in found:
            where = f'{path}:{line}'
            r = classes.find(cls, name, desc)
            if r == 'missing-class':
                errors.append(f'{where}: class not found: {cls}')
            elif r is None:
                errors.append(f'{where}: no {"field" if "field" in kind else "method"} {cls}.{name} {desc}')
            elif kind in ('static', 'sfield') and r != 'static':
                errors.append(f'{where}: {cls}.{name} {desc} is not static')
            elif kind in ('method', 'field') and r == 'static' and name != '<init>':
                errors.append(f'{where}: {cls}.{name} {desc} is static')
        print(f'checked {len(found)} JNI references in {len({u[4] for u in found})} files')
        for e in errors:
            print(e)
        sys.exit(1 if errors else 0)


if __name__ == '__main__':
    main()

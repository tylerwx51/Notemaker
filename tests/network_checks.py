#!/usr/bin/env python3
"""Verify an explicitly configured multi-machine proxy tree using a fresh test document."""
import argparse
import datetime
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid
from topology_support import http

ROOT = Path(__file__).resolve().parents[1]


def check(topology, out, timeout=90):
    config = json.loads(topology.read_text())
    server = config['server'].rstrip('/')
    paths = [(topology.parent / p).resolve() for p in config['proxies']]
    if len(paths) < 2:
        raise ValueError('Network checks require at least two proxy descriptors')
    peers = [json.loads(path.read_text()) for path in paths]
    root = http(server + '/api/info')
    if root.get('kind') != 'server' or root.get('protocol', 0) < 3:
        raise AssertionError('Expected a protocol 3 main server')
    infos = [http(p['url'] + '/api/info', token=p['token']) for p in peers]
    if len({p['id'] for p in infos}) != len(peers):
        raise AssertionError('Select distinct proxies')
    if not all(p.get('kind') == 'proxy' and p.get('protocol', 0) >= 3 and
               p.get('rootId') == root['id'] for p in infos):
        raise AssertionError('Proxies must already belong to the selected main server')
    document = 'test-network-' + uuid.uuid4().hex
    report = dict(document=document, root_id=root['id'], proxies=[p['id'] for p in infos], status='running')
    # Persist the document name before editing, including for failed runs.
    (out / 'report.json').write_text(json.dumps(report, indent=2))
    def cli(index, *args, value=None):
        return json.loads(subprocess.check_output([
            str(ROOT / 'apps/cli/notemaker'), '--proxy-file', str(paths[index]),
            '--document', document, '--actor', 'network-test-' + str(index), *args],
            input=json.dumps(value).encode() if value is not None else None, timeout=30))
    operations = set()
    for i in range(len(peers)):
        operations.add(cli(i, 'object', 'put', '-', value={
            'object': 'peer-' + str(i), 'shape': 'rect', 'x': i * 25,
            'y': 10, 'w': 20, 'h': 20})['operation'])
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        states = [cli(i, 'log') for i in range(len(peers))]
        if all(not s['pending'] for s in states):
            canonical = states[0]['events']
            if (all(s['events'] == canonical for s in states) and len(canonical) == len(operations)
                    and {e['id'] for e in canonical} == operations):
                committed, cursor = [], 0
                while True:
                    page = http(server + f'/api/replica?document={document}&after={cursor}&pendingAfter=0')
                    committed.extend(page['events'])
                    cursor = page['after']
                    if not page['more']:
                        break
                if committed != canonical:
                    raise AssertionError('Proxy histories differ from main-server final order')
                models = [cli(i, 'inspect') for i in range(len(peers))]
                if not all(m == models[0] for m in models):
                    raise AssertionError('Materialized models differ')
                if len(models[0]['objects']) != len(peers):
                    raise AssertionError('Not all proxy edits survived')
                report.update(status='pass', committed_actions=len(committed))
                return report
        time.sleep(.5)
    raise AssertionError('External topology did not converge before timeout')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--topology', required=True, type=Path)
    parser.add_argument('--timeout', type=float, default=90)
    args = parser.parse_args()
    if not os.environ.get('IN_NIX_SHELL'):
        parser.error('Run through nix-shell')
    out = ROOT / '.test-artifacts/network' / datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    out.mkdir(parents=True, mode=0o700)
    try:
        report = check(args.topology.resolve(), out, args.timeout)
    except Exception as error:
        report = json.loads((out / 'report.json').read_text()) if (out / 'report.json').exists() else {}
        report.update(status='fail', error=str(error))
    (out / 'report.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    print('Artifacts: ' + str(out))
    return int(report['status'] != 'pass')


if __name__ == '__main__':
    sys.exit(main())

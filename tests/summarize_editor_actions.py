#!/usr/bin/env python3
"""Export note-free physical editor action timings and per-mode process counters."""
import argparse
import json
from pathlib import Path


def counters(before, after):
    return {
        'process_cpu_ms': after['process_cpu_ms'] - before['process_cpu_ms'],
        'pss_kib_before_after': [before['pss_kib'], after['pss_kib']],
        'java_used_bytes_before_after': [before['java_used_bytes'], after['java_used_bytes']],
        'io_delta': {key: after['io'][key] - before['io'][key]
                     for key in ('rchar', 'wchar', 'read_bytes', 'write_bytes')
                     if key in before['io'] and key in after['io']},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('runs', nargs='+', type=Path)
    args = parser.parse_args()
    summary = {'workload': '60 synthetic pans, 40 zoom redraws, 40 tool switches, 40 empty-layer switches, 12 warm same-note reopens; 5 warmups except 2 reopens',
               'timing': 'enqueue to end of app Canvas draw; not E Ink scanout or physical finger sampling',
               'runs': []}
    for run in args.runs:
        metrics = json.loads((run / 'metrics.json').read_text())
        metadata = json.loads((run / 'metadata.json').read_text())
        modes = {}
        for name, values in metrics['modes'].items():
            resources = metrics['mode_resources'][name]
            modes[name] = {**values, 'resources': counters(resources['before'], resources['after'])}
        result = {'source_revision': metadata['revision'],
                  'source_apk_sha256': metadata['source_apk_sha256'],
                  'device_model': metadata['ro.product.model'],
                  'android_api': metadata['ro.build.version.sdk'],
                  'modes': modes}
        if 'post_gc_resources' in metrics:
            result['post_gc'] = counters(metrics['end_resources'], metrics['post_gc_resources'])
        summary['runs'].append(result)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2) + '\n')


if __name__ == '__main__':
    main()

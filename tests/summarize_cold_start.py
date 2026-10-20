#!/usr/bin/env python3
"""Export note-free Android process-cold launch and resource measurements."""
import argparse
import json
import statistics
from pathlib import Path


def spread(values):
    return {'min': min(values), 'p50': statistics.median(values), 'max': max(values)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('runs', nargs='+', type=Path)
    args = parser.parse_args()
    report = {'measurement': 'Android am start -W after force-stop, with a copied note in an isolated package; Android TotalTime excludes physical E Ink scanout',
              'runs': []}
    for path in args.runs:
        metrics = json.loads((path / 'metrics.json').read_text())
        metadata = json.loads((path / 'metadata.json').read_text())
        measured = metrics['samples'][metrics['warmup']:]
        assert len(measured) >= 10
        values = {key: spread([sample[key] for sample in measured])
                  for key in ('android_total_ms', 'android_wait_ms', 'host_start_wall_ms',
                              'process_cpu_ms', 'pss_kib')}
        values['io'] = {key: spread([sample['io'][key] for sample in measured])
                        for key in ('rchar', 'wchar', 'read_bytes', 'write_bytes')}
        report['runs'].append({'compile_speed': metadata['compile_speed'],
                               'source_revision': metadata['revision'],
                               'source_apk_sha256': metadata['source_apk_sha256'],
                               'isolated_apk_sha256': metadata['isolated_apk_sha256'],
                               'device_model': metadata['ro.product.model'],
                               'android_api': metadata['ro.build.version.sdk'],
                               'warmup': metrics['warmup'],
                               'sample_count': len(measured),
                               'stats': values,
                               'samples': measured})
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()

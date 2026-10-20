#!/usr/bin/env python3
"""Export a reviewable, note-free summary of a performance harness run."""
import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run', type=Path, help='Artifact directory from tests/performance.py or tablet_erase_latency.py')
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    metrics = json.loads((args.run / 'metrics.json').read_text())
    resources = json.loads((args.run / 'resources.json').read_text())['samples']
    metadata_path = args.run / 'metadata.json'
    metadata = json.loads(metadata_path.read_text()) if metadata_path.exists() else {}
    first, last = resources[0], resources[-1]
    io_keys = ('rchar', 'wchar', 'syscr', 'syscw', 'read_bytes', 'write_bytes', 'cancelled_write_bytes')
    io_delta = {key: last['io'][key] - first['io'][key]
                for key in io_keys if key in first['io'] and key in last['io']}
    summary = {
        'source_revision': metadata.get('revision'),
        'source_apk_sha256': metadata.get('source_apk_sha256'),
        'device_model': metadata.get('ro.product.model'),
        'android_api': metadata.get('ro.build.version.sdk'),
        'workload': {'strokes': metrics['strokes'], 'short_stroke_points': metrics['short_stroke_points'],
                     'figure_eight_points': metrics['figure_eight_points'],
                     'viewport_wrapped': True},
        'latency_ms': {key: metrics[key] for key in
                       ('stroke_latency', 'down_latency', 'move_latency', 'up_latency', 'figure_eight_latency')},
        'windows': metrics['windows'],
        'dispatch_stalls': len(metrics['dispatch_stalls']),
        'elapsed_seconds': metrics['elapsed_seconds'],
        'drain_ms': metrics['drain_ms'],
        'max_save_queue': metrics['max_save_queue'],
        'incremental_matches_replay': metrics['incremental_matches_replay'],
        'tile_cache': metrics['tile_cache'],
        'resources': [{**{key: sample[key] for key in
                          ('strokes', 'pss_kib', 'private_dirty_kib', 'java_used_bytes',
                           'native_allocated_bytes', 'process_cpu_ms', 'sampling_ms')},
                       'read_bytes': sample['io'].get('read_bytes'),
                       'write_bytes': sample['io'].get('write_bytes'),
                       'memory_parts_kib': sample.get('memory_parts')}
                      for sample in resources],
        'io_counters_delta_before_final_drain': io_delta,
        'io_note': 'Linux per-process counters. write_bytes is charged I/O, not measured storage latency.',
        'restart_verified': (args.run / 'reopened.txt').exists() and
                            'PASS state' in (args.run / 'reopened.txt').read_text(),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2) + '\n')


if __name__ == '__main__':
    main()

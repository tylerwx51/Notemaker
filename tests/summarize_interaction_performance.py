#!/usr/bin/env python3
"""Export note-free timing summaries from paired tablet interaction runs."""
import argparse
import json
from pathlib import Path


def summarize(run):
    metrics = json.loads((run / 'metrics.json').read_text())
    metadata = json.loads((run / 'metadata.json').read_text())
    before, after = metrics['start_resources'], metrics['end_resources']
    return {
        'source_revision': metadata['revision'],
        'source_apk_sha256': metadata['source_apk_sha256'],
        'device_model': metadata['ro.product.model'],
        'android_api': metadata['ro.build.version.sdk'],
        'modes': metrics['modes'],
        'process_pss_kib_before_after': [before['pss_kib'], after['pss_kib']],
        'process_cpu_ms_delta': after['process_cpu_ms'] - before['process_cpu_ms'],
        'process_io_delta': {key: after['io'][key] - before['io'][key]
                             for key in ('read_bytes', 'write_bytes', 'rchar', 'wchar')
                             if key in before['io'] and key in after['io']},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('baseline_first', type=Path)
    parser.add_argument('candidate_first', type=Path)
    parser.add_argument('candidate_repeat', type=Path)
    parser.add_argument('baseline_repeat', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = {'workload': '60 pan, 40 zoom redraw, 40 tool switches; 5 warmup each; disposable note',
              'latency_definition': 'enqueue to end of Canvas draw; synthetic input, not E Ink scanout',
              'runs': {name: summarize(getattr(args, name)) for name in
                       ('baseline_first', 'candidate_first', 'candidate_repeat', 'baseline_repeat')}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()

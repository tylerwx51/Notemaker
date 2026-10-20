#!/usr/bin/env python3
"""Export note-free summaries from paired physical eraser UI runs."""
import argparse
import json
import math
import statistics
from pathlib import Path


def summary(path):
    metrics = json.loads((path / 'metrics.json').read_text())
    metadata = json.loads((path / 'metadata.json').read_text())
    samples = metrics['samples']
    result = {
        'samples': len(samples),
        'geometry_restored': metrics['geometry_restored'],
        'source_apk_sha256': metadata.get('source_apk_sha256'),
        'device_model': metadata.get('ro.product.model'),
    }
    for key in ('input_to_feedback_ms', 'input_to_updated_draw_ms'):
        values = sorted(float(row[key]) for row in samples)
        result[key] = {
            'p50': statistics.median(values),
            'p95': values[math.ceil(.95 * len(values)) - 1],
            'max': values[-1],
        }
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('baseline_partial', 'current_partial', 'baseline_item', 'current_item'):
        parser.add_argument(name, type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = {name: summary(getattr(args, name)) for name in
              ('baseline_partial', 'current_partial', 'baseline_item', 'current_item')}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()

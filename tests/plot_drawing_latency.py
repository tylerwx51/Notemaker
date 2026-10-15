#!/usr/bin/env python3
"""Plot sanitized latency measurements. Run in nix-shell; optionally import raw runs.

No document content is copied into the report. --raw points to the private
.test-artifacts/latency directory; without it, use the checked-in measurements.
Percentiles use the nearest-rank convention (ceil(p*n)-1).
"""
import argparse
import json
import math
from pathlib import Path
import statistics

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs/design/latency'


def summary(values):
    ordered = sorted(values)
    return dict(n=len(values), median=statistics.median(values),
                p95=ordered[math.ceil(.95*len(values))-1], max=max(values),
                under30=sum(v < 30 for v in values), values=values)


def read_lines(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.startswith('{')]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--raw', type=Path)
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    data_path = OUT/'measurements.json'
    if args.raw:
        data = {'ui': {}, 'cache': {}, 'backend': {}}
        for name in ['ui-compact', 'ui-preview', 'ui-pictures', 'ui-layer-cache',
                     'ui-native-snapshot', 'ui-final-partial', 'ui-final-item', 'ui-item-verified']:
            run = json.loads((args.raw/name/'metrics.json').read_text())
            data['ui'][name] = {key: summary([s[key] for s in run['samples']])
                               for key in ['input_to_feedback_ms', 'input_to_updated_draw_ms']
                               if key in run['samples'][0]}
            data['ui'][name]['display_refresh_hz'] = run.get('display_refresh_hz')
        for name in ['unions-memory', 'parts-memory', 'compact-memory']:
            rows = read_lines(args.raw/name/'timings.jsonl')
            base = rows[0]
            data['cache'][name] = [{'items': int(row['phase'].split('_')[1]),
                'heap_mib': (row['heap_bytes']-base['heap_bytes'])/2**20,
                'native_mib': (row['native_bytes']-base['native_bytes'])/2**20}
                for row in rows if row.get('phase', '').startswith('cache_') and row['phase'] != 'cache_empty']
        for name in ['tablet-final-profile', 'cache10000', 'compact']:
            rows = read_lines(args.raw/name/'timings.jsonl')
            data['backend'][name] = {'erase_ms': [r['ms'] for r in rows if r.get('phase')=='proxy_erase'],
                                    'burst_final_ms': [r['total_ms'] for r in rows if r.get('phase')=='queued_erase'][-1]}
        data_path.write_text(json.dumps(data, indent=2)+'\n')
    data = json.loads(data_path.read_text())
    plt.rcParams.update({'font.size': 11, 'axes.spines.top': False, 'axes.spines.right': False})
    fig, axes = plt.subplots(1, 2, figsize=(12, 4.7), layout='constrained')
    names = ['ui-compact', 'ui-preview', 'ui-final-partial', 'ui-item-verified']
    labels = ['Geometry only', '+ Partial preview', 'Final partial', 'Final whole item']
    for ax, key, title in zip(axes,
            ['input_to_feedback_ms', 'input_to_updated_draw_ms'],
            ['First visible feedback', 'Authoritative updated draw']):
        stats = [data['ui'][n].get(key, data['ui'][n]['input_to_updated_draw_ms']) for n in names]
        med = np.array([s['median'] for s in stats]); p95 = np.array([s['p95'] for s in stats])
        ax.bar(range(4), med, color=['#8193a5', '#65a3ac', '#267c75', '#8e6aab'], width=.65)
        ax.scatter(range(4), p95, color='#242424', marker='_', s=180, label='95th percentile', zorder=3)
        ax.axhline(30, color='#b64a39', linestyle='--', label='30 ms target')
        for i, s in enumerate(stats): ax.text(i, s['median']+3, f"{s['median']:.1f}", ha='center', fontsize=10)
        ax.set_xticks(range(4), labels, rotation=15, ha='right')
        ax.set(title=title, ylabel='Milliseconds', ylim=(0, max(p95)*1.2))
        ax.grid(axis='y', alpha=.15); ax.set_axisbelow(True)
    axes[0].legend(loc='upper right', fontsize=9)
    fig.suptitle('BOOX tablet: 29 erase taps per run, offline disposable note\nBars = median; dash markers = p95. App draw completion, not E Ink scanout.', fontsize=12)
    for suffix in ['png', 'svg']: fig.savefig(OUT/f'latency.{suffix}', dpi=170)
    plt.close(fig)
    fig, axes = plt.subplots(1, 2, figsize=(12, 4.6), layout='constrained')
    labels = ['Union paths\n10k cache', 'Primitive paths\nRAM experiment', 'Packed ink\nRecommended']
    runs = [data['cache'][n][-1] for n in ['unions-memory', 'parts-memory', 'compact-memory']]
    heap = [r['heap_mib'] for r in runs]; native = [r['native_mib'] for r in runs]
    axes[0].bar(range(3), heap, label='Java heap', color='#267c75')
    axes[0].bar(range(3), native, bottom=heap, label='Native allocations', color='#b68847')
    for i in range(3): axes[0].text(i, heap[i]+native[i]+4, f'{heap[i]+native[i]:.1f} MiB', ha='center')
    axes[0].set_xticks(range(3), labels); axes[0].set(ylabel='Retained allocation increase (MiB)', ylim=(0,260))
    axes[0].legend(loc='upper left',fontsize=9);axes[0].set_title('10,000 distinct 20-point strokes')
    values = [data['backend'][n]['burst_final_ms'] for n in ['tablet-final-profile','cache10000','compact']]
    axes[1].bar(range(3), values, color=['#8193a5','#b68847','#267c75'])
    for i,v in enumerate(values): axes[1].text(i,v*1.1,f'{v:,.0f} ms',ha='center')
    axes[1].set_xticks(range(3), ['Original\n16 entries', 'Union cache\n10k entries', 'Packed + retained\nmaterialization'])
    axes[1].set(yscale='log', ylabel='Completion of fifth queued request (ms, log scale)', ylim=(10,15000))
    axes[1].set_title('Five-request erase burst')
    fig.suptitle('Tradeoffs: cache allocations exclude document, prepared snapshots, images and display buffers', fontsize=12)
    for ax in axes: ax.grid(axis='y',alpha=.15);ax.set_axisbelow(True)
    for suffix in ['png','svg']:fig.savefig(OUT/f'tradeoffs.{suffix}',dpi=170)
    plt.close(fig)
    print(data_path)


if __name__ == '__main__':
    main()

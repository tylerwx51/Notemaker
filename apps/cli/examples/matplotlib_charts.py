"""Generate self-contained Matplotlib SVG examples for NoteMaker.

Run inside the repository's nix-shell: python3 apps/cli/examples/matplotlib_charts.py DIR
"""
import io
from pathlib import Path
import re
import sys

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np


def compatible_svg(figure):
    """Remove Matplotlib's non-rendering XML wrapper and RDF metadata.

    This only processes SVG produced by the supplied Figure. Matplotlib's paths,
    definitions, clipping and local references remain vector data.
    """
    output = io.StringIO()
    figure.savefig(output, format='svg')
    source = output.getvalue()
    source = source[source.index('<svg '):]
    source, count = re.subn(r'\s*<metadata>.*?</metadata>', '', source, count=1, flags=re.S)
    if count != 1:
        raise ValueError('Matplotlib SVG metadata wrapper changed')
    return source


def figures():
    x = np.linspace(0, 2 * np.pi, 80)
    fig, ax = plt.subplots(figsize=(6, 3))
    ax.plot(x, np.sin(x), label='sin(x)')
    ax.plot(x, np.cos(x), '--', label='cos(x)')
    ax.legend(); ax.set_title('Line plot'); ax.set_xlabel('Radians')
    fig.tight_layout()
    yield 'line', fig

    fig, ax = plt.subplots(figsize=(6, 3))
    ax.scatter(x[::3], np.sin(x[::3]), c=x[::3], cmap='viridis', s=35)
    ax.set_title('Scatter plot')
    yield 'scatter', fig

    fig, ax = plt.subplots(figsize=(6, 3))
    ax.bar(['A', 'B', 'C'], [5, 3, 7], hatch='//')
    ax.set_title('Hatched bars')
    yield 'bar', fig

    fig, ax = plt.subplots(figsize=(6, 3))
    ax.hist(np.random.default_rng(4).normal(size=160), bins=12)
    ax.set_title('Histogram')
    yield 'histogram', fig

    fig, ax = plt.subplots(figsize=(6, 3))
    ax.boxplot([np.arange(9), np.arange(9) ** .5])
    ax.set_title('Box plot')
    yield 'box', fig

    fig, ax = plt.subplots(figsize=(6, 3))
    ax.violinplot([np.sin(x), np.cos(x)])
    ax.set_title('Violin plot')
    yield 'violin', fig

    fig, ax = plt.subplots(figsize=(6, 3))
    ax.pcolormesh(np.arange(7), np.arange(6), np.arange(30).reshape(5, 6), cmap='viridis')
    ax.set_title('Vector heatmap')
    yield 'heatmap', fig

    fig, ax = plt.subplots(figsize=(4, 4))
    ax.pie([2, 3, 4], labels=['A', 'B', 'C'], autopct='%1.0f%%')
    ax.set_title('Pie chart')
    yield 'pie', fig

    fig, ax = plt.subplots(figsize=(6, 3))
    xx, yy = np.meshgrid(np.linspace(-2, 2, 18), np.linspace(-2, 2, 18))
    ax.contour(xx, yy, xx**2 + yy**2, levels=5)
    ax.set_title('Contour plot')
    yield 'contour', fig

    fig, ax = plt.subplots(figsize=(6, 3))
    ax.errorbar([1, 2, 3], [2, 3, 2], yerr=[.2, .4, .3], fmt='o-')
    ax.set_title('Error bars')
    yield 'errorbar', fig


def generate(directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    result = {}
    with plt.rc_context({'svg.fonttype': 'path', 'svg.hashsalt': 'notemaker',
                         'svg.image_inline': True}):
        for name, figure in figures():
            path = directory / (name + '.svg')
            path.write_text(compatible_svg(figure), encoding='utf-8')
            result[name] = path
            plt.close(figure)
    return result


if __name__ == '__main__':
    for name, path in generate(sys.argv[1]).items():
        print(name, path)

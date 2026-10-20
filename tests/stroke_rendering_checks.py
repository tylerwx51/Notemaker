"""Raster regression tests through actual offline CLI actions and exports."""
from pathlib import Path
import tempfile
import math
import subprocess
import unittest
import numpy as np
from matplotlib.image import imread
from topology_support import Proxy, ROOT


class StrokeRenderingChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.compiled = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.compiled.cleanup)
        cls.classpath = cls.compiled.name + ':' + str(ROOT / 'apps/cli/build/notemaker.jar')
        subprocess.run(['javac', '-cp', cls.classpath, '-d', cls.compiled.name,
                        str(ROOT / 'tests/StrokeSvgRaster.java')], check=True, capture_output=True)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.proxy = Proxy(self.root / 'replica', 'stroke-rendering')
        self.render_count = 0

    def tearDown(self):
        self.proxy.stop()
        self.temp.cleanup()

    def render(self, name, paths, brush, color, svg=False):
        if self.render_count:
            self.proxy.cli('erase', '-', value=dict(mode='page', page=0))
        self.render_count += 1
        for index, points in enumerate(paths):
            self.proxy.cli('stroke', 'put', '-', value=dict(
                stroke=name + str(self.render_count) + '-' + str(index), layer='base', page=0, width=12,
                brush=brush, material=50, color=color,
                points=[[x, y, 1] for x, y in points]))
        output = self.root / (name + '.png')
        if svg:
            source = self.root / (name + '.svg')
            self.proxy.cli('export', str(source), '0', '0', '0', '200', '200')
            subprocess.run(['java', '-Djava.awt.headless=true', '-cp', self.classpath,
                            'StrokeSvgRaster', str(source), str(output)], check=True, capture_output=True)
        else:
            self.proxy.cli('export', str(output), '0', '0', '0', '200', '200')
        return np.rint(imread(output) * 255).astype(int)

    def test_self_crossing_deposits_same_ink_as_two_crossing_lines(self):
        # The connecting turn is outside the crop. Both arms and the entire
        # crossing (including antialiased edges) are compared, not just one pixel.
        fixtures = [
            ([(30, 30), (170, 170), (170, 30), (30, 170)],
             [[(30, 30), (170, 170)], [(170, 30), (30, 170)]]),
            ([(30, 100), (170, 100), (170, 30), (100, 30), (100, 170)],
             [[(30, 100), (170, 100)], [(100, 30), (100, 170)]]),
            ([(30, 100), (170, 100), (30, 100)],
             [[(30, 100), (170, 100)], [(170, 100), (30, 100)]]),
        ]
        for svg in (False, True):
            for brush, color in [('pen', -16777216), ('pen', -2147483648), ('marker', -16777216)]:
                for fixture, (loop, separate) in enumerate(fixtures):
                    for reverse in (False, True):
                        with self.subTest(svg=svg, brush=brush, color=color, fixture=fixture, reverse=reverse):
                            if reverse:
                                loop = list(reversed(loop))
                            actual = self.render('loop', [loop], brush, color, svg)[70:130, 70:130]
                            expected = self.render('lines', separate, brush, color, svg)[70:130, 70:130]
                            delta = np.abs(actual - expected)
                            # Union coverage and sequential source-over differ at a few
                            # antialiased crossing corners. Flat ink/paper must be exact.
                            flat = np.ones((58, 58), dtype=bool)
                            for dy in range(3):
                                for dx in range(3):
                                    flat &= np.all(expected[dy:dy+58, dx:dx+58] == expected[1:59, 1:59], axis=2)
                            self.assertTrue(np.all(delta[1:59, 1:59][flat] == 0), 'crossing interior changed')
                            self.assertLessEqual(delta.max(), 64, 'crossing edge coverage changed')
                            self.assertLess(delta.mean(), .25)
                            self.assertTrue(np.array_equal(actual[30, 30], expected[30, 30]),
                                            'crossing center must receive both passes of ink')

    def test_translucent_curves_have_no_seams_between_ribbon_passes(self):
        circle = [(100 + 60 * math.cos(math.radians(angle)),
                   100 + 60 * math.sin(math.radians(angle))) for angle in range(0, 361, 2)]
        for svg in (False, True):
            for brush, color in [('pen', -2147483648), ('marker', -16777216)]:
                with self.subTest(svg=svg, brush=brush):
                    curve = self.render('curve', [circle], brush, color, svg)
                    straight = self.render('straight', [[(30, 100), (170, 100)]], brush, color, svg)
                    ink = straight[100, 100]
                    # The renderer partitions this smooth circle near each quarter
                    # turn. Probe a band across every artificial join, away from
                    # the actual stroke endpoints and antialiased outer boundary.
                    for join in (90, 180, 270):
                        for angle in range(join - 15, join + 16):
                            x = round(100 + 60 * math.cos(math.radians(angle)))
                            y = round(100 + 60 * math.sin(math.radians(angle)))
                            self.assertTrue(np.all(curve[y-1:y+2, x-1:x+2] == ink),
                                            f'opacity seam at {angle} degrees')


if __name__ == '__main__':
    unittest.main()

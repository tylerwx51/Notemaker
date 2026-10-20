#!/usr/bin/env python3
"""Run the real CanvasView viewport cache and culling regression on API 36."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
env = dict(os.environ)
env.setdefault('NOTEMAKER_DRAWING_ANDROID_PORT', '5592')
env.setdefault('NOTEMAKER_DRAWING_ANDROID_OUT', '.test-artifacts/drawing-android-viewport')
subprocess.run([sys.executable, str(ROOT / 'tests' / 'drawing_android.py'), '--viewport-only'],
               cwd=ROOT, env=env, check=True)

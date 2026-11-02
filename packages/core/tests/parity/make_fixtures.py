#!/usr/bin/env python3
"""Deterministic fixture generator for the Java/Rust core parity harness.

Writes packages/core/tests/parity/fixtures/{json,scenes,actions,assets}.
Run from anywhere: python3 make_fixtures.py [FIXTURES_DIR]
Requires Pillow (supplied by the root shell.nix) for the raster assets.
"""
import base64
import io
import json
import math
import os
import random
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "fixtures")
rng = random.Random(20261102)


def write(path, text, binary=False):
    full = os.path.join(OUT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "wb" if binary else "w", encoding=None if binary else "utf-8", newline=None if binary else "\n") as f:
        f.write(text)


def dump(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


# ---------------------------------------------------------------- JSON cases
json_cases = [
    '1', '1.0', '0.1', '1e-5', '1E-5', '1e21', '1e+21', '-0.0', '-0', '0', '-1', '9223372036854775807',
    '-9223372036854775808', '9223372036854775808', '1.7976931348623157e308', '1e309', '4.9e-324', '1e-400',
    '123456789012345678901234567890.5', '0.30000000000000004', '100.0', '1e7', '1.0e-3', '12345678.9', '1e23',
    '2e23', '0.001', '0.0001', '-1.5e-10', '3.141592653589793', '2.718281828459045', '1.1', '1.10',
    '"plain"', '"\\u0001\\u001f\\u0020"', '"\\"\\\\\\/\\b\\f\\n\\r\\t"', '"\\u00e9\\u4e2d\\ud83d\\ude00"',
    '"é中😀"', '"\\ud800"', '"\\udc00x"', '"\\ud83dx"', '"\\u-123"', '"\\u+0041"', '"\\uzzzz"', '"\\u12"',
    '"a\\qb"', '"tab\there"', '"unterminated', '"\\', 'true', 'false', 'null', ' [ 1 , 2 ] ', '[]', '{}',
    '[1,]', '[,1]', '{"a":1,}', '{"a":1,"a":2}', '{"a" 1}', '{1:2}', '{"b":1,"a":2,"c":{"z":[1,{"y":null}]}}',
    '[1 2]', '01', '1.', '.5', '-', '-a', '+1', '1e', '1e+', 'tru', 'nul', 'NaN', 'Infinity', '[1]x', '',
    '   ', '{"k":"v"} ', '[' * 65 + ']' * 65, '[' * 64 + ']' * 64, '{"é":"ü","😀":1}', '"  "',
    '[1.0E-5,1.0E21,-0.0,1e-7,123e-2,5e-1]', '{"nested":{"list":[[[[]]]],"n":-12.5e3}}',
]
write("json/cases.txt", "\n".join(c.replace("\n", "\\n") for c in json_cases) + "\n")
equivalent_pairs = [
    ('1', '1.0'), ('1', '1.00001'), ('{"a":1}', '{"a":1.0}'), ('[1,2]', '[1.0,2.0]'), ('[1,2]', '[2,1]'),
    ('{"a":1,"b":2}', '{"b":2,"a":1}'), ('{"a":1}', '{"a":1,"b":2}'), ('1e21', '1000000000000000000000'),
    ('-0.0', '0'), ('"1"', '1'), ('null', 'null'), ('true', 'true'), ('0.1', '0.10000000000000001'),
    ('9007199254740993', '9007199254740992'), ('123456789012345678', '1.23456789012345678E17'),
]
write("json/equivalent.txt", "\n".join(a + "\t" + b for a, b in equivalent_pairs) + "\n")


# ---------------------------------------------------------------- strokes
def stroke(sid, points, brush=None, width=3, color=-16777216, material=None, layer="base", page=0, erasures=None):
    op = {"kind": "stroke", "stroke": sid, "layer": layer, "page": page, "width": width, "color": color, "points": points}
    if brush is not None:
        op["brush"] = brush
    if material is not None:
        op["material"] = material
    if erasures is not None:
        op["erasures"] = erasures
    return op


def r(v, digits=2):
    return round(v, digits)


def wave_points(x0, y0, n, amp, step, phase=0.0, pressure_mode="ramp"):
    pts = []
    for i in range(n):
        x = x0 + i * step
        y = y0 + amp * math.sin(i * 0.35 + phase)
        if pressure_mode == "ramp":
            p = 0.2 + 0.8 * i / max(1, n - 1)
        elif pressure_mode == "pulse":
            p = 0.5 + 0.5 * math.sin(i * 0.5)
        else:
            p = 1.0
        pts.append([r(x), r(y), r(max(0.0, min(1.0, p)), 3)])
    return pts


def loop_points(cx, cy, n, radius):
    pts = []
    for i in range(n):
        a = i * 2 * math.pi * 1.6 / n
        pts.append([r(cx + radius * math.cos(a) + i * 0.7), r(cy + radius * math.sin(a)), r(0.4 + 0.5 * (i % 7) / 6, 3)])
    return pts


def zigzag(x0, y0, n, size):
    return [[x0 + (i % 2) * size, y0 + i * size * 0.6, 0.6] for i in range(n)]


brushes = ["pen", "pencil", "marker", "brush", "oily", "fountain", "charcoal", "neoBrush", "latinCalligraphy", "asiaCalligraphy", "square"]
ops = []
y = 40
for bi, brush in enumerate(brushes):
    for mi, material in enumerate([None, 0, 50, 100]):
        x0 = 40 + mi * 240
        ops.append(stroke(f"s-{brush}-{mi}", wave_points(x0, y, 24, 18, 8.5, phase=bi, pressure_mode=["ramp", "pulse", "flat", "ramp"][mi]),
                          brush=brush, width=[2, 5, 9, 14][mi], color=[-16777216, 0x7f3355cc - 2**32 if 0x7f3355cc > 2**31 else 0x7f3355cc, -65536, 0x40000000][mi], material=material))
    y += 70
write("scenes/brushes.json", dump({"type": "fixed", "ops": ops, "renders": [{"page": 0, "viewport": [0, 0, 1000, 820]}]}))

ops = [
    stroke("loop-pen", loop_points(200, 200, 90, 80), brush="pen", width=6, color=-16777216),
    stroke("loop-marker", loop_points(500, 200, 90, 80), brush="marker", width=12, color=0x80ff8800 - 2**32),
    stroke("loop-oily", loop_points(800, 200, 90, 80), brush="oily", width=10, color=-16777216, material=70),
    stroke("zig-pencil", zigzag(100, 400, 30, 30), brush="pencil", width=4),
    stroke("zig-charcoal", zigzag(300, 400, 30, 30), brush="charcoal", width=8, material=30),
    stroke("zig-brush", zigzag(500, 400, 30, 30), brush="brush", width=12, material=80),
    stroke("zig-fountain", zigzag(700, 400, 30, 30), brush="fountain", width=6, material=20),
    stroke("dot", [[600, 1000, 0.5]], brush="pen", width=8),
    stroke("dot-marker", [[650, 1000, 1.0]], brush="marker", width=8),
    stroke("dup-points", [[100, 1000, 0.5], [100, 1000, 0.5], [100.000001, 1000, 0.5], [140, 1010, 0.8]], width=5),
    stroke("retrace", [[100, 1100, 0.5], [300, 1100, 0.5], [100, 1100, 0.5], [300, 1100, 0.5]], brush="marker", width=10, color=0x66000000),
    stroke("long", [[50 + i * 0.9, 1250 + 30 * math.sin(i / 9), 0.5] for i in range(600)], brush="neoBrush", width=7, material=60),
    stroke("erased", wave_points(500, 1150, 40, 25, 10), width=10, erasures=[
        [[60, -40], [120, -40], [120, 40], [60, 40]],
        [[200, -30], [260, 0], [200, 30]],
    ]),
]
write("scenes/strokes.json", dump({"type": "fixed", "ops": ops, "renders": [{"page": 0, "viewport": [0, 0, 1000, 1400]}]}))

# ---------------------------------------------------------------- shapes
shape_ids = ["rect", "ellipse", "text", "line", "triangle", "diamond", "pentagon", "hexagon", "star", "arrow",
             "trapezoid", "triangle45", "triangle60", "triangle90", "wave", "curve", "polyline", "regularPolygon",
             "cube", "cylinder", "cone", "sphere", "pyramid", "circle", "rectangle", "rhombus", "arrowLine", "waveLine", "regularHexagon"]
ops = []
for i, shape in enumerate(shape_ids):
    col, row = i % 5, i // 5
    o = {"kind": "object", "object": f"o-{shape}", "layer": "base", "page": 0, "shape": shape,
         "x": 30 + col * 190, "y": 30 + row * 210, "w": 150, "h": 120 if shape not in ("line", "arrow", "arrowLine") else 90,
         "color": [-16777216, -16776961, 0x80008000, -65536][i % 4], "width": 1 + i % 6, "fontSize": 18 + i % 3 * 4,
         "text": f"{shape}\nline two & <three>" if i % 3 == 0 else ""}
    if i % 2 == 0 and shape not in ("text", "line", "arrow", "arrowLine", "wave", "curve", "polyline", "waveLine"):
        o["fill"] = [0x40ff0000, -256, 0x2000ffff][i % 3]
    if i % 4 == 1:
        o["rotation"] = [15, -30, 90, 135.5][i % 4]
    if i % 5 == 2:
        o["flipX"] = True
    if i % 7 == 3:
        o["flipY"] = True
    if i % 3 == 1:
        o["fontFamily"] = ["serif", "monospace", "sans-serif"][i % 3]
        o["alignment"] = ["center", "right", "left"][i % 3]
        o["bold"] = True
        o["italic"] = i % 2 == 0
        o["underline"] = True
    ops.append(o)
ops.append({"kind": "object", "object": "erased-rect", "layer": "base", "page": 0, "shape": "rect", "x": 600, "y": 1300, "w": 200, "h": 80,
            "color": -16777216, "width": 4, "fontSize": 24, "text": "", "erasures": [[[0.1, 0.1], [0.4, 0.1], [0.4, 0.9], [0.1, 0.9]]], "rotation": 20})
write("scenes/shapes.json", dump({"type": "fixed", "ops": ops, "renders": [{"page": 0, "viewport": [0, 0, 1000, 1400]}, {"page": 0, "viewport": [-50.5, 100.25, 512, 333.75]}]}))

# ---------------------------------------------------------------- connectors, text, layers
diagram = json.load(open(os.path.join(HERE, "..", "..", "..", "..", "apps", "cli", "examples", "diagram.json")))
ops = [{"kind": "layer", "layer": "architecture", "name": "Architecture", "force": False}]
for o in diagram["objects"]:
    full = {"kind": "object", "layer": "architecture", "page": 0, "color": -16777216, "width": 3, "fontSize": 24, "text": "",
            "x": 0, "y": 0, "w": 160, "h": 80}
    full.update(o)
    ops.append(full)
ops += [
    {"kind": "object", "object": "rot-node", "layer": "base", "page": 0, "shape": "rect", "x": 600, "y": 900, "w": 200, "h": 100,
     "rotation": 30, "flipX": True, "color": -16777216, "width": 2, "fontSize": 20, "text": "rotated"},
    {"kind": "object", "object": "circ-node", "layer": "base", "page": 0, "shape": "circle", "x": 100, "y": 1100, "w": 120, "h": 120,
     "color": -16777216, "width": 2, "fontSize": 20, "text": ""},
    {"kind": "object", "object": "rot-link", "layer": "base", "page": 0, "shape": "arrowLine", "from": "rot-node", "to": "circ-node",
     "x": 0, "y": 0, "w": 10, "h": 10, "color": -16776961, "width": 3, "fontSize": 18, "text": "link"},
    {"kind": "object", "object": "half-link", "layer": "base", "page": 0, "shape": "arrow", "to": "rot-node",
     "x": 900, "y": 1300, "w": -50, "h": -200, "color": -16777216, "width": 2, "fontSize": 18, "text": ""},
    {"kind": "layer", "layer": "hidden-layer", "name": "Hidden", "force": False, "order": 5},
    {"kind": "layer", "layer": "forced", "name": "Forced", "force": True, "order": -2},
    {"kind": "layer", "layer": "inactive", "name": "Inactive", "force": False, "order": 1.5},
    {"kind": "layer-active", "layer": "inactive", "active": False},
    stroke("hidden-stroke", wave_points(100, 1300, 20, 10, 10), layer="hidden-layer", width=4),
    stroke("forced-stroke", wave_points(300, 1300, 20, 10, 10), layer="forced", width=4, color=-65536),
    stroke("inactive-stroke", wave_points(500, 1300, 20, 10, 10), layer="inactive", width=4),
    {"kind": "object", "object": "caption-2", "layer": "forced", "page": 0, "shape": "text", "x": 50, "y": 50, "w": 400, "h": 80,
     "text": "", "fontSize": 30, "color": -16777216, "width": 1, "alignment": "center"},
    {"kind": "active", "stroke": "hidden-stroke", "active": True},
    {"kind": "object-active", "object": "server", "active": False},
]
write("scenes/diagram.json", dump({"type": "fixed", "ops": ops, "renders": [{"page": 0, "viewport": [0, 0, 1000, 1400], "hidden": ["hidden-layer", "forced"]}]}))

# ---------------------------------------------------------------- assets (raster)
def png_bytes(w, h, mode="RGBA", color=(255, 0, 0, 128)):
    from PIL import Image
    img = Image.new(mode, (w, h), color)
    for i in range(min(w, h)):
        img.putpixel((i, i), (0, 0, 255, 255) if mode == "RGBA" else (0, 0, 255))
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def jpeg_bytes(w, h, progressive=False):
    from PIL import Image
    img = Image.new("RGB", (w, h), (10, 200, 30))
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=70, progressive=progressive)
    return buf.getvalue()


def gif_bytes(frames, w, h, durations, loop=0):
    from PIL import Image
    imgs = [Image.new("P", (w, h), i * 40 % 256) for i in range(frames)]
    buf = io.BytesIO()
    imgs[0].save(buf, format="GIF", save_all=True, append_images=imgs[1:], duration=durations, loop=loop)
    return buf.getvalue()


try:
    png = png_bytes(16, 12)
    write("assets/good-small.png", png, binary=True)
    write("assets/good-rgb.png", png_bytes(40, 30, "RGB", (1, 2, 3)), binary=True)
    write("assets/bad-crc.png", png[:20] + bytes([png[20] ^ 0xff]) + png[21:], binary=True)
    write("assets/bad-truncated.png", png[:-5], binary=True)
    write("assets/bad-trailing.png", png + b"xx", binary=True)
    big = bytearray(png)
    struct.pack_into(">I", big, 16, 9000)
    write("assets/bad-dimensions.png", bytes(big), binary=True)
    jpg = jpeg_bytes(33, 21)
    write("assets/good-small.jpg", jpg, binary=True)
    write("assets/good-progressive.jpg", jpeg_bytes(64, 48, True), binary=True)
    write("assets/bad-truncated.jpg", jpg[:-2], binary=True)
    write("assets/bad-header.jpg", b"\xff\xd9" + jpg[2:], binary=True)
    gif = gif_bytes(3, 20, 10, [50, 0, 120])
    write("assets/good-anim.gif", gif, binary=True)
    write("assets/good-still.gif", gif_bytes(1, 8, 8, [100]), binary=True)
    write("assets/good-slow.gif", gif_bytes(2, 8, 8, [20000, 30]), binary=True)
    write("assets/bad-truncated.gif", gif[:-3], binary=True)
    write("assets/bad-header.gif", b"GIF88a" + gif[6:], binary=True)
    write("assets/bad-noframes.gif", gif[:13 + 3 * 256] + b"\x3b" if len(gif) > 800 else gif[:13] + b"\x3b", binary=True)
    write("assets/bad-short.gif", b"GIF89a\x01\x00", binary=True)
except ImportError:
    print("Pillow unavailable; raster assets not regenerated", file=sys.stderr)
uris = [
    "data:image/png;base64,", "data:image/png;base64,@@@@", "data:image/png;base64,iVBORw0KGgo=", "data:image/png;base64,a",
    "data:image/gif;base64,R0lGODlh", "data:image/gif;base64,R0lGODlhAQABAAAAACw=x", "data:image/gif;base64,R0lGé",
    "data:image/gif;base64,R0lGODlhAQABA===", "data:image/webp;base64,AAAA", "data:image/jpeg;base64,/9j/2Q==",
    "http://example.com/a.png",
]
write("assets/uris.txt", "\n".join(uris) + "\n")

# ---------------------------------------------------------------- images and svg objects in a scene
asset_dir = os.path.join(OUT, "assets")
svg_dir = os.path.join(OUT, "svg")
ops = []
if os.path.exists(os.path.join(asset_dir, "good-small.png")):
    data = open(os.path.join(asset_dir, "good-small.png"), "rb").read()
    ops.append({"kind": "object", "object": "img", "layer": "base", "page": 0, "shape": "image", "x": 10, "y": 10, "w": 160, "h": 120,
                "image": "data:image/png;base64," + base64.b64encode(data).decode(), "color": -16777216, "width": 1, "fontSize": 24, "text": ""})
    gifdata = open(os.path.join(asset_dir, "good-anim.gif"), "rb").read()
    ops.append({"kind": "object", "object": "gif", "layer": "base", "page": 0, "shape": "gif", "x": 200, "y": 10, "w": 100, "h": 50,
                "gif": "data:image/gif;base64," + base64.b64encode(gifdata).decode(), "color": -16777216, "width": 1, "fontSize": 24, "text": "", "rotation": 45})
for i, name in enumerate(["good-placed.svg", "good-matplotlib-line.svg", "good-dots-generated.svg", "good-prefixed.svg"]):
    p = os.path.join(svg_dir, name)
    if os.path.exists(p):
        ops.append({"kind": "object", "object": f"svg-{i}", "layer": "base", "page": 0, "shape": "svg", "x": 20 + i * 240, "y": 300, "w": 220, "h": 180,
                    "svg": open(p, encoding="utf-8").read(), "color": -16777216, "width": 1, "fontSize": 24, "text": "", "flipY": i == 1,
                    "erasures": [[[0.2, 0.2], [0.5, 0.2], [0.5, 0.5]]] if i == 2 else []})
write("scenes/media.json", dump({"type": "fixed", "ops": ops, "renders": [{"page": 0, "viewport": [0, 0, 1000, 600]}]}))

# ---------------------------------------------------------------- conflicts and replay rules
conflict_ops = [
    {"kind": "document", "type": "pages", "id": "op-doc"},
    {"kind": "document", "type": "infinite", "id": "op-doc2"},
    {"kind": "batch", "id": "op-1", "actor": "alice", "actions": [stroke("c-1", [[0, 0, 0.5], [10, 10, 0.5]]), {"kind": "active", "stroke": "c-1", "active": True}],
     "bases": {"stroke:c-1": ""}, "visibility": {"stroke:c-1": True}},
    {"kind": "batch", "id": "op-2", "actor": "bob", "actions": [stroke("c-1", [[0, 0, 0.5], [20, 20, 0.5]])], "bases": {"stroke:c-1": "op-1"}},
    {"kind": "batch", "id": "op-3", "actor": "carol", "actions": [stroke("c-1", [[0, 0, 0.5], [30, 30, 0.5]])], "bases": {"stroke:c-1": "op-1"}},
    {"kind": "batch", "id": "op-4", "actor": "alice", "actions": [stroke("c-1", [[0, 0, 0.5], [40, 40, 0.5]])], "bases": {"stroke:c-1": "op-3"}},
    {"kind": "stroke", "id": "op-5", "actor": "dave", **{k: v for k, v in stroke("c-2", [[5, 5, 1], [6, 6, 1]], page=2).items()}, "bases": {"stroke:c-2": "zzz"}},
    {"kind": "active", "id": "op-6", "actor": "erin", "stroke": "c-1", "active": False, "bases": {"stroke:c-1": "op-2"}, "visibility": {"stroke:c-1": False}},
    {"kind": "conflict-status", "id": "op-7", "conflict": "none", "active": False},
    {"kind": "object", "id": "op-8", "object": "o-1", "layer": "base", "page": 1, "shape": "rect", "x": 1, "y": 2, "w": 3, "h": 4, "color": 0, "width": 1},
    {"kind": "object", "id": "op-9", "object": "o-1", "layer": "base", "page": 1, "shape": "rect", "x": 5, "y": 2, "w": 3, "h": 4, "color": 0, "width": 1, "bases": {"object:o-1": "op-x"}},
    {"kind": "layer", "layer": "L2", "name": "Two", "force": False},
    {"kind": "object", "object": "o-2", "layer": "L2", "page": 1, "shape": "ellipse", "x": 1, "y": 1, "w": 5, "h": 5, "color": 0, "width": 1, "text": "hi"},
    {"kind": "object", "object": "o-2", "layer": "base", "page": 0, "shape": "ellipse", "x": 1, "y": 1, "w": 5, "h": 5, "color": 0, "width": 1},
    {"kind": "bogus", "id": "op-bad"},
]
write("scenes/conflicts.json", dump({"type": "fixed", "ops": conflict_ops, "renders": [{"page": 0, "viewport": [0, 0, 100, 100]}, {"page": 1, "viewport": [0, 0, 100, 100]}], "raw": True}))

deleted_ops = conflict_ops[:4] + [{"kind": "document-delete", "id": "del"}, stroke("after-delete", [[1, 1, 1]])]
write("scenes/deleted.json", dump({"type": "fixed", "ops": deleted_ops, "renders": [], "raw": True}))

# Validation-only cases: each line is one operation, printed as OK or the error.
invalid = [
    {"kind": "stroke"}, stroke("", [[0, 0, 0]]), stroke("x" * 201, [[0, 0, 0]]), stroke("ok", []), stroke("ok", [[0, 0]]),
    stroke("ok", [[0, 0, 2]]), stroke("ok", [[0, 0, "1"]]), stroke("ok", [[2e6, 0, 0.5]]), stroke("ok", [[0, 0, 0.5]], width=0.5),
    stroke("ok", [[0, 0, 0.5]], width=101), stroke("ok", [[0, 0, 0.5]], color=2**31), stroke("ok", [[0, 0, 0.5]], color=1.5),
    stroke("ok", [[0, 0, 0.5]], brush="crayon"), stroke("ok", [[0, 0, 0.5]], material=50.5), stroke("ok", [[0, 0, 0.5]], page=1.5),
    stroke("ok", [[0, 0, 0.5]], page=10001), stroke("ok", [[0, 0, 0.5]], erasures=[[[0, 0], [1, 1]]]),
    stroke("ok", [[0, 0, 0.5]], erasures=[[[0, 0], [1, 1], ["a", 1]]]), stroke("ok", [[0, 0, 0.5]], erasures=[[[0, 0], [1, 1], [2e9, 1]]]),
    stroke("ok", [[0, 0, 0.5]], erasures=[[[0, 0, 0], [1, 1], [1, 0]]]), stroke("ok", [[0, 0, 0.5]], layer=""),
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "blob", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "rect", "x": 0, "y": 0, "w": 0, "h": 1, "color": 0, "width": 1},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "line", "x": 0, "y": 0, "w": 0, "h": -5, "color": 0, "width": 1},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "rect", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1, "text": "t", "fontSize": 0},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "rect", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1, "rotation": 361},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "rect", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1, "fontFamily": "cursive"},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "rect", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1, "alignment": "justify"},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "rect", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1, "bold": "yes"},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "rect", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1, "from": ""},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "template", "template": "lines", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "template", "template": "dots", "spacing": 4, "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "svg", "svg": "<svg/>", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "image", "image": "data:image/png;base64,AAAA", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "gif", "gif": "nope", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1},
    {"kind": "object", "object": "o", "layer": "base", "page": 0, "shape": "rect", "x": 0, "y": 0, "w": 1, "h": 1, "color": 0, "width": 1, "text": "x" * 10001, "fontSize": 10},
    {"kind": "layer", "layer": "l", "name": "n", "force": "no"}, {"kind": "layer", "layer": "l", "name": "n", "force": False, "order": 2e6},
    {"kind": "layer-active", "layer": "base", "active": False}, {"kind": "active", "stroke": "s", "active": 1},
    {"kind": "document", "type": "scroll"}, {"kind": "conflict-status", "conflict": "c"},
    {"kind": "batch", "actions": []}, {"kind": "batch", "actions": [{"kind": "batch", "actions": []}]},
    {"kind": "batch", "actions": [{"kind": "document-delete"}]}, {"kind": "batch", "actions": [{"kind": "nope"}]},
    {"kind": "batch", "actions": [{"kind": "active", "stroke": "s", "active": True}], "bases": {"k": 1}},
    {"kind": "batch", "actions": [{"kind": "active", "stroke": "s", "active": True}], "visibility": {"k": "yes"}},
    {"kind": "active", "stroke": "s", "active": True, "actor": ""}, {"kind": "active", "stroke": "s", "active": True, "session": "x" * 300},
    {"kind": "teleport"}, {"kind": 5}, {"kind": "document-delete"},
    {"kind": "stroke", "stroke": "é" * 200, "layer": "base", "page": 0, "width": 1, "color": 0, "points": [[0, 0, 0]]},
    {"kind": "stroke", "stroke": "😀" * 101, "layer": "base", "page": 0, "width": 1, "color": 0, "points": [[0, 0, 0]]},
]
write("validate.jsonl", "\n".join(dump(v) for v in invalid) + "\n")

# ---------------------------------------------------------------- drawing actions
calls = [
    {"scene": "diagram", "fn": "copySelection", "entities": [{"kind": "object", "id": "app"}, {"kind": "object", "id": "app-state"}, {"kind": "object", "id": "rot-link"}, {"kind": "object", "id": "circ-node"}, {"kind": "object", "id": "rot-node"}]},
    {"scene": "diagram", "fn": "copySelection", "entities": [{"kind": "stroke", "id": "nope"}]},
    {"scene": "diagram", "fn": "copySelection", "entities": [{"kind": "layer", "id": "base"}]},
    {"scene": "diagram", "fn": "paste", "input": {"entities": "COPY", "dx": 10.5, "dy": -3, "layer": "forced", "page": 0}},
    {"scene": "strokes", "fn": "paste", "input": {"entities": [stroke("p1", [[1, 2, 0.5], [3, 4, 0.5]])]}},
    {"scene": "strokes", "fn": "paste", "input": {"entities": [{"kind": "object", "object": "n1", "layer": "base", "page": 0, "shape": "arrow", "to": "missing", "x": 0, "y": 0, "w": 5, "h": 5, "color": 0, "width": 1}]}},
    {"scene": "strokes", "fn": "paste", "input": {"entities": [stroke("p1", [[1, 2, 0.5]]), stroke("p1", [[1, 2, 0.5]])]}},
    {"scene": "diagram", "fn": "duplicateLayer", "id": "architecture"},
    {"scene": "diagram", "fn": "duplicateLayer", "id": "missing"},
    {"scene": "diagram", "fn": "mergeLayer", "source": "architecture", "target": "base"},
    {"scene": "diagram", "fn": "mergeLayer", "source": "base", "target": "architecture"},
    {"scene": "diagram", "fn": "mergeLayer", "source": "forced", "target": "inactive"},
    {"scene": "diagram", "fn": "reorderLayer", "id": "forced", "index": 4},
    {"scene": "diagram", "fn": "reorderLayer", "id": "base", "index": 0},
    {"scene": "diagram", "fn": "reorderLayer", "id": "base", "index": 9},
    {"scene": "shapes", "fn": "template", "input": {"template": "ruled"}},
    {"scene": "shapes", "fn": "template", "input": {"template": "grid", "w": 3000, "h": 2000, "spacing": 120, "color": -16777216, "page": 0}},
    {"scene": "shapes", "fn": "template", "input": {"template": "dots", "spacing": 50, "x": 5, "y": 6, "object": "dots-1", "background": False}},
    {"scene": "shapes", "fn": "template", "input": {"template": "planner", "w": 1000, "h": 700}},
    {"scene": "shapes", "fn": "template", "input": {"template": "blank"}},
    {"scene": "shapes", "fn": "template", "input": {"template": "dots", "spacing": 9}},
    {"scene": "shapes", "fn": "template", "input": {"template": "weird"}},
    {"scene": "infinite", "fn": "template", "input": {"template": "grid", "spacing": 40, "page": 0}},
    {"scene": "infinite", "fn": "template", "input": {"template": "dots", "background": False, "object": "g", "layer": "base"}},
    {"scene": "shapes", "fn": "fill", "input": {"object": "o-rect", "color": 0x40ff0000}},
    {"scene": "shapes", "fn": "fill", "input": {"object": "o-line", "color": -1}},
    {"scene": "fill", "fn": "fill", "input": {"stroke": "closed", "color": -16711936}},
    {"scene": "fill", "fn": "fill", "input": {"stroke": "closed-erased", "color": 0x7f00ff00}},
    {"scene": "fill", "fn": "fill", "input": {"stroke": "open", "color": -16711936}},
    {"scene": "fill", "fn": "fill", "input": {"x": 250, "y": 250, "color": 0x7f0000ff}},
    {"scene": "fill", "fn": "fill", "input": {"x": 640, "y": 260, "color": -16776961}},
    {"scene": "fill", "fn": "fill", "input": {"x": 5, "y": 5, "color": -16776961}},
    {"scene": "fill", "fn": "fill", "input": {"x": 200, "y": 150, "color": -16776961}},
    {"scene": "strokes", "fn": "erase", "input": {"mode": "stroke", "points": [[90, 400, 1], [400, 420, 1]], "radius": 10}},
    {"scene": "strokes", "fn": "erase", "input": {"mode": "partial", "points": [[90, 400], [120, 430], [150, 460], [150, 460], [180, 490], [400, 1100]], "radius": 6}},
    {"scene": "shapes", "fn": "erase", "input": {"mode": "partial", "points": [[100, 100], [500, 300]], "radius": 15}},
    {"scene": "shapes", "fn": "erase", "input": {"mode": "item", "points": [[100, 100]], "radius": 3}},
    {"scene": "shapes", "fn": "erase", "input": {"mode": "page"}},
    {"scene": "shapes", "fn": "erase", "input": {"mode": "sweep", "points": [[0, 0]]}},
    {"scene": "shapes", "fn": "erase", "input": {"mode": "area", "points": [[0, 0]], "radius": 0.5}},
    {"scene": "shapes", "fn": "eraseRegions", "input": {"points": [[0, 0], [10, 0], [20, 0], [20, 10], [20, 10], [5, 5]], "radius": 4}},
    {"scene": "shapes", "fn": "eraseRegions", "input": {"points": [[3, 4]], "radius": 0.05}},
    {"scene": "shapes", "fn": "eraseRegions", "input": {"points": [[3, 4]], "radius": 1}},
    {"scene": "diagram", "fn": "transform", "entities": [{"kind": "object", "id": "app"}, {"kind": "object", "id": "state"}, {"kind": "object", "id": "app-state"}, {"kind": "object", "id": "half-link"}],
     "spec": {"scale": 1.5, "dx": 10, "dy": -20, "rotation": 30, "flipX": True}},
    {"scene": "strokes", "fn": "transform", "entities": [{"kind": "stroke", "id": "erased"}, {"kind": "stroke", "id": "dot"}], "spec": {"scale": 0.5, "rotation": -45, "flipY": True, "pivotX": 10, "pivotY": 20}},
    {"scene": "strokes", "fn": "transform", "entities": [{"kind": "stroke", "id": "dot"}], "spec": {"scale": 0}},
    {"scene": "strokes", "fn": "transform", "entities": [{"kind": "stroke", "id": "dot"}, {"kind": "stroke", "id": "dot"}], "spec": {}},
    {"scene": "strokes", "fn": "transform", "entities": [{"kind": "layer", "id": "dot"}], "spec": {}},
    {"scene": "strokes", "fn": "transform", "entities": [{"kind": "stroke", "id": "dot"}], "spec": {"dx": "1"}},
    {"scene": "strokes", "fn": "transformSimple", "entities": [{"kind": "stroke", "id": "zig-brush"}], "scale": 2, "dx": 3, "dy": 4},
    {"scene": "empty", "fn": "diagram", "input": diagram},
    {"scene": "diagram", "fn": "diagram", "input": diagram},
    {"scene": "empty", "fn": "diagram", "input": {"layer": "d2", "objects": [{"object": "a", "shape": "rect"}, {"object": "b", "shape": "arrow", "from": "a", "to": "a"}]}},
    {"scene": "empty", "fn": "diagram", "input": {"layer": "d3", "objects": [{"object": "a", "shape": "rect"}, {"object": "a", "shape": "rect"}]}},
    {"scene": "empty", "fn": "diagram", "input": {"layer": "d4", "objects": []}},
    {"scene": "diagram", "fn": "inverse", "ops": [{"kind": "conflict-status", "conflict": "x", "active": False}, {"kind": "object", "object": "app"}, {"kind": "object", "object": "new"},
                                                    {"kind": "stroke", "stroke": "new"}, {"kind": "layer", "layer": "forced"}, {"kind": "layer-active", "layer": "inactive", "active": True},
                                                    {"kind": "object-active", "object": "server", "active": True}, {"kind": "active", "stroke": "x", "active": False}, {"kind": "document", "type": "fixed"}]},
    {"scene": "diagram", "fn": "validateReferences", "ops": [stroke("v", [[0, 0, 0]], layer="nope")]},
    {"scene": "diagram", "fn": "validateReferences", "ops": [stroke("v", [[0, 0, 0]], page=1)]},
    {"scene": "diagram", "fn": "validateReferences", "ops": [stroke("v", [[0, 0, 0]])]},
    {"scene": "diagram", "fn": "select", "layer": "architecture", "page": 0, "region": [[100, 140], [400, 140], [400, 600], [100, 600]], "topmost": False},
    {"scene": "diagram", "fn": "select", "layer": "architecture", "page": 0, "region": None, "topmost": True},
    {"scene": "strokes", "fn": "select", "layer": "base", "page": 0, "region": [[0, 380], [1000, 380], [1000, 520], [0, 520]], "topmost": False},
    {"scene": "strokes", "fn": "paintBounds", "layer": "base", "page": 0},
    {"scene": "shapes", "fn": "paintBounds", "layer": "base", "page": 0},
    {"scene": "diagram", "fn": "requireReferences", "layer": "architecture", "page": 0, "refs": [{"kind": "object", "id": "app"}, {"kind": "object", "id": "server"}]},
    {"scene": "diagram", "fn": "locks", "state": {"leases": {"0:object:app": {"actor": "other", "session": "s2", "expires": 99999999999999, "layer": "architecture", "page": 0},
                                                           "0:stroke:hidden-stroke": {"actor": "me", "session": "s1", "expires": 99999999999999},
                                                           "1:object:state": {"actor": "x", "session": "y", "expires": 99999999999999},
                                                           "0:object:old": {"actor": "x", "session": "y", "expires": 5}},
                                                "activeSessions": {"me": "s1"}},
     "ops": [{"kind": "object", "object": "app", "layer": "architecture", "page": 0}, {"kind": "batch", "actions": [{"kind": "active", "stroke": "hidden-stroke", "active": False}, {"kind": "layer-active", "layer": "architecture", "active": False}]}],
     "incoming": {"kind": "batch", "actor": "me", "session": "s1", "actions": [{"kind": "object-active", "object": "state", "active": False}, {"kind": "layer", "layer": "architecture", "name": "A", "force": False, "order": 3}]}},
]
fill_ops = [
    stroke("closed", [[100 + 100 * math.cos(a / 20 * 2 * math.pi), 250 + 100 * math.sin(a / 20 * 2 * math.pi), 0.5] for a in range(21)], width=4),
    stroke("closed-erased", [[400, 150, 0.5], [500, 150, 0.5], [500, 250, 0.5], [400, 250, 0.5], [402, 152, 0.5]], width=6, erasures=[[[40, -20], [60, -20], [60, 20], [40, 20]]]),
    stroke("open", [[0, 0, 0.5], [100, 0, 0.5], [100, 100, 0.5]], width=4),
    {"kind": "object", "object": "box", "layer": "base", "page": 0, "shape": "rect", "x": 550, "y": 150, "w": 200, "h": 200, "color": -16777216, "width": 4, "rotation": 10},
    {"kind": "object", "object": "tri", "layer": "base", "page": 0, "shape": "triangle", "x": 600, "y": 200, "w": 80, "h": 80, "color": -16777216, "width": 3, "flipY": True},
    {"kind": "object", "object": "ell", "layer": "base", "page": 0, "shape": "ellipse", "x": 150, "y": 100, "w": 120, "h": 60, "color": -16777216, "width": 2},
]
write("scenes/fill.json", dump({"type": "fixed", "ops": fill_ops, "renders": [{"page": 0, "viewport": [0, 0, 800, 400]}]}))
write("scenes/infinite.json", dump({"type": "infinite", "ops": [{"kind": "document", "type": "infinite"}, {"kind": "layer", "layer": "template-0", "name": "x", "force": False, "order": -3},
                                                               {"kind": "layer-active", "layer": "template-0", "active": False}], "renders": []}))
write("scenes/empty.json", dump({"type": "fixed", "ops": [], "renders": [{"page": 0, "viewport": [0, 0, 10, 10]}]}))
write("actions.jsonl", "\n".join(dump(c) for c in calls) + "\n")
print("fixtures written to", OUT)

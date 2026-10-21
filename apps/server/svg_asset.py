"""Static, self-contained SVG profile; kept in sync with core/SvgAsset.java."""
import math
import re
import xml.etree.ElementTree as ET

TAGS = set('svg g defs title desc path rect circle ellipse line polyline polygon text tspan linearGradient radialGradient stop clipPath marker style use pattern'.split())
ATTRS = set('id class style version x y x1 y1 x2 y2 dx dy width height viewBox preserveAspectRatio transform d points rx ry r cx cy fx fy fr fill fill-opacity fill-rule stroke stroke-width stroke-opacity stroke-linecap stroke-linejoin stroke-miterlimit stroke-dasharray stroke-dashoffset opacity color font-family font-size font-style font-weight text-anchor dominant-baseline alignment-baseline baseline-shift textLength lengthAdjust display visibility clip-path clip-rule gradientUnits gradientTransform spreadMethod offset stop-color stop-opacity href marker-start marker-mid marker-end markerWidth markerHeight markerUnits refX refY orient type space patternUnits patternContentUnits patternTransform'.split())
SVG = '{http://www.w3.org/2000/svg}'

def css(value):
    value = re.sub(r'/\*.*?\*/', '', value, flags=re.S).lower()
    if any(s in value for s in ('\\', '@', '/*')):
        raise ValueError('SVG CSS escapes/imports are unsupported')
    value = re.sub(r'''url\(\s*['"]?\#[a-z_][a-z0-9_.:-]*['"]?\s*\)''', '', value)
    if 'url' in value or 'expression(' in value:
        raise ValueError('SVG resources must be local fragments')

def validate_svg(source):
    if not isinstance(source, str) or len(source.encode('utf-8')) > 262144:
        raise ValueError('SVG must be text, at most 256 KiB')
    if '<!DOCTYPE' in source.upper() or '<!ENTITY' in source.upper():
        raise ValueError('SVG DTDs and entities are unsupported')
    class StaticTree(ET.TreeBuilder):
        def pi(self, target, text=None):
            raise ValueError('SVG processing instructions are unsupported')
    try:
        root = ET.fromstring(source, parser=ET.XMLParser(target=StaticTree()))
    except ET.ParseError as e:
        raise ValueError('Invalid SVG XML') from e
    if root.tag != SVG + 'svg':
        raise ValueError('Expected SVG root')
    count = 0
    def visit(node, depth):
        nonlocal count
        count += 1
        if count > 10000 or depth > 64:
            raise ValueError('SVG is too complex')
        if not node.tag.startswith(SVG) or node.tag[len(SVG):] not in TAGS:
            raise ValueError('Unsupported SVG element: ' + node.tag)
        for name, value in node.attrib.items():
            if name == '{http://www.w3.org/1999/xlink}href':
                name = 'href'
            elif name == '{http://www.w3.org/XML/1998/namespace}space':
                name = 'space'
            if name not in ATTRS:
                raise ValueError('Unsupported SVG attribute: ' + name)
            if name == 'href' and not re.fullmatch(r'#[A-Za-z_][A-Za-z0-9_.:-]*', value):
                raise ValueError('SVG references must be local fragments')
            if name in ('style', 'fill', 'stroke', 'clip-path') or name.startswith('marker-'):
                css(value)
        if node.tag == SVG + 'style':
            css(''.join(node.itertext()))
        for child in node:
            visit(child, depth + 1)
    visit(root, 1)
    paths = {node.get('id') for node in root.iter(SVG + 'path') if node.get('id')}
    for node in root.iter(SVG + 'use'):
        href = node.get('href') or node.get('{http://www.w3.org/1999/xlink}href', '')
        if not href.startswith('#') or href[1:] not in paths:
            raise ValueError('SVG use must reference a local path')
    try:
        if 'viewBox' in root.attrib:
            box = [float(v) for v in re.split(r'[\s,]+', root.attrib['viewBox'].strip())]
            if len(box) != 4 or any(not math.isfinite(v) or abs(v) > 1000000 for v in box):
                raise ValueError('Invalid SVG viewBox')
            w, h = box[2:]
        else:
            w, h = [float(re.sub(r'px$', '', root.get(k, '').strip())) for k in ('width', 'height')]
        if not (0 < w <= 1000000 and 0 < h <= 1000000):
            raise ValueError('Invalid SVG dimensions')
    except (ValueError, OverflowError) as e:
        raise ValueError('SVG needs positive viewBox or pixel width/height') from e

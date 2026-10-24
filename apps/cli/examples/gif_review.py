"""Build three small monochrome GIFs and an atomic review diagram.

Usage: python3 apps/cli/examples/gif_review.py OUTPUT_DIR
Then: notemaker --document 'GIF Player Review' diagram add OUTPUT_DIR/diagram.json
"""
import base64
import json
import math
import sys
from pathlib import Path
from PIL import Image, ImageDraw

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
items = []
examples = [
    ('moving-dot', '1. Moving dot - position changes', 400),
    ('blinking-grid', '2. Blinking grid - contrast changes', 600),
    ('clock-hand', '3. Clock hand - angle changes', 900),
]
for index, (name, caption, delay) in enumerate(examples):
    frames = []
    for frame in range(6):
        im = Image.new('RGB', (240, 180), 'white')
        draw = ImageDraw.Draw(im)
        draw.rectangle((1, 1, 238, 178), outline='black', width=3)
        if index == 0:
            x = 35 + frame * 34
            draw.ellipse((x-17, 73, x+17, 107), fill='black')
        elif index == 1:
            for row in range(3):
                for col in range(4):
                    if (row + col + frame) % 2 == 0:
                        x, y = 24 + col*50, 25 + row*45
                        draw.rectangle((x, y, x+30, y+30), fill='black')
        else:
            angle = math.pi * (frame/3 - .5)
            draw.ellipse((100, 70, 140, 110), outline='black', width=3)
            draw.line((120, 90, 120+65*math.cos(angle), 90+65*math.sin(angle)), fill='black', width=8)
        frames.append(im)
    path = out / f'{name}.gif'
    frames[0].save(path, save_all=True, append_images=frames[1:], duration=delay, loop=0, optimize=False)
    uri = 'data:image/gif;base64,' + base64.b64encode(path.read_bytes()).decode()
    x = 55 + index * 310
    items.append(dict(object=f'gif-review-{name}', shape='gif', gif=uri, x=x, y=190, w=270, h=203, text=name))
    items.append(dict(object=f'gif-review-caption-{name}', shape='text', x=x, y=510, w=280, h=50,
                      text=caption, fontSize=18))
items.insert(0, dict(object='gif-review-title', shape='text', x=55, y=95, w=900, h=60,
                     text='GIF Player Review', fontSize=36))
items.append(dict(object='gif-review-instructions', shape='text', x=55, y=625, w=900, h=50,
                  text='Select Interact, then tap the controls beneath an animation.', fontSize=22))
(out / 'diagram.json').write_text(json.dumps(dict(name='GIF playback review', objects=items)))
print(json.dumps(dict(files=[str(out / (name+'.gif')) for name, _, _ in examples], diagram=str(out/'diagram.json'))))

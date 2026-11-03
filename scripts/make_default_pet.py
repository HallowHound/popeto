# Draws Popeto's default pet "Popi" (a round sprout creature) and writes a Shimeji-ee pack:
#   assets/pets/Popi/s*.png, preview.png, conf/actions.xml, conf/behaviors.xml
# Frames face left, 128x128, anchor at the bottom center. Needs Pillow:  python scripts/make_default_pet.py
import os
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'assets', 'pets', 'Popi')
S = 4                      # supersampling
W = H = 128
LINE = (47, 72, 78, 255)
BODY = (132, 214, 178, 255)
BELLY = (196, 240, 214, 255)
CHEEK = (246, 160, 170, 255)
LEAF = (98, 178, 92, 255)
WHITE = (255, 255, 255, 255)
LW = 3                     # outline width


def e(d, cx, cy, rx, ry, fill, outline=True):
    box = [(cx - rx) * S, (cy - ry) * S, (cx + rx) * S, (cy + ry) * S]
    d.ellipse(box, fill=fill, outline=LINE if outline else None, width=LW * S if outline else 0)


def limb(d, x0, y0, x1, y1, w=7):
    d.line([x0 * S, y0 * S, x1 * S, y1 * S], fill=LINE, width=(w + 2 * LW) * S)
    for x, y in ((x0, y0), (x1, y1)):
        r = w / 2 + LW
        d.ellipse([(x - r) * S, (y - r) * S, (x + r) * S, (y + r) * S], fill=LINE)
    d.line([x0 * S, y0 * S, x1 * S, y1 * S], fill=BODY, width=w * S)
    for x, y in ((x0, y0), (x1, y1)):
        r = w / 2
        d.ellipse([(x - r) * S, (y - r) * S, (x + r) * S, (y + r) * S], fill=BODY)


def pet(cx=64, bottom=118, rx=30, ry=34, feet=((52, 122), (76, 122)), arms=(), back_arms=(),
        eyes='open', look=(0, 0), mouth='smile', sprout=True):
    """One frame. arms: ((x0,y0,x1,y1), ...) drawn over the body, back_arms behind it."""
    im = Image.new('RGBA', (W * S, H * S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    cy = bottom - ry
    top = bottom - 2 * ry
    if sprout:
        d.line([(cx + 2) * S, (top + 3) * S, (cx + 5) * S, (top - 9) * S], fill=LINE, width=(3 + 2) * S)
        e(d, cx + 12, top - 11, 8, 4, LEAF)
        e(d, cx - 2, top - 10, 6, 3, LEAF)
    for a in back_arms: limb(d, *a)
    for fx, fy in feet: e(d, fx, fy, 9, 5, BODY)
    e(d, cx, cy, rx, ry, BODY)
    e(d, cx - 3, cy + ry * 0.35, rx * 0.62, ry * 0.5, BELLY, outline=False)
    for a in arms: limb(d, *a)
    # face, looking left
    ex, ey = cx - 10 + look[0], cy - ry * 0.25 + look[1]
    for k, x in enumerate((ex - 8, ex + 8)):
        if eyes == 'closed' or (eyes == 'wink' and k == 0):
            d.arc([(x - 5) * S, (ey - 3) * S, (x + 5) * S, (ey + 4) * S], 200, 340, fill=LINE, width=3 * S)
        elif eyes == 'x':
            for s in (-1, 1):
                d.line([(x - 4) * S, (ey - 4 * s) * S, (x + 4) * S, (ey + 4 * s) * S], fill=LINE, width=3 * S)
        else:
            r = 6 if eyes == 'wide' else 5
            e(d, x, ey, r - 1, r, LINE, outline=False)
            e(d, x - 1.5, ey - 2, 1.8, 1.8, WHITE, outline=False)
    for x in (ex - 15, ex + 15):
        e(d, x, ey + 8, 4.5, 2.5, CHEEK, outline=False)
    mx, my = ex, ey + 9
    if mouth == 'smile':
        d.arc([(mx - 4) * S, (my - 3) * S, (mx + 4) * S, (my + 3) * S], 20, 160, fill=LINE, width=3 * S)
    elif mouth == 'o':
        e(d, mx, my + 1, 3, 3.5, LINE, outline=False)
    elif mouth == 'flat':
        d.line([(mx - 3) * S, (my + 1) * S, (mx + 3) * S, (my + 1) * S], fill=LINE, width=3 * S)
    return im.resize((W, H), Image.LANCZOS)


# name -> frame. Feet sit on y~127 so the anchor (64,128) is the floor.
F = {}
F['stand'] = pet()
F['blink'] = pet(eyes='closed')
F['walk1'] = pet(bottom=116, feet=((48, 118), (78, 122)))
F['walk2'] = pet(bottom=118)
F['walk3'] = pet(bottom=116, feet=((50, 122), (80, 118)))
F['wink'] = pet(eyes='wink')
F['sit'] = pet(bottom=124, rx=33, ry=28, feet=((38, 123), (54, 124)))
F['sit_up'] = pet(bottom=124, rx=33, ry=28, feet=((38, 123), (54, 124)), look=(0, -4), mouth='o')
F['dangle1'] = pet(bottom=124, rx=33, ry=28, feet=((40, 121), (56, 124)))
F['dangle2'] = pet(bottom=124, rx=33, ry=28, feet=((38, 124), (54, 121)))
F['sprawl'] = pet(bottom=127, rx=42, ry=17, feet=(), eyes='closed', mouth='flat',
                  arms=((26, 116, 14, 122),))
F['creep1'] = pet(bottom=126, rx=38, ry=21, feet=((80, 124),), arms=((34, 114, 18, 121),))
F['creep2'] = pet(bottom=126, rx=38, ry=21, feet=((84, 124),), arms=((36, 116, 24, 122),))
F['trip'] = pet(bottom=127, rx=42, ry=17, feet=((96, 118),), eyes='x', mouth='o',
                arms=((26, 118, 12, 124),))
F['bounce'] = pet(bottom=126, rx=36, ry=27)
F['fall'] = pet(bottom=112, rx=28, ry=36, feet=((54, 116), (74, 116)), eyes='wide', mouth='o',
                back_arms=((40, 66, 30, 44), (88, 66, 98, 44)))
F['jump'] = pet(bottom=118, rx=26, ry=38, feet=((56, 122), (72, 122)), eyes='wide',
                back_arms=((42, 62, 32, 40), (86, 62, 96, 40)))
F['drag1'] = pet(bottom=114, rx=28, ry=34, feet=((54, 118), (74, 120)), eyes='wide', mouth='o',
                 back_arms=((48, 52, 58, 30), (80, 52, 70, 30)))
F['drag2'] = pet(cx=60, bottom=114, rx=28, ry=34, feet=((48, 119), (70, 117)), eyes='wide', mouth='o',
                 back_arms=((44, 52, 56, 30), (76, 52, 68, 30)))
F['drag3'] = pet(cx=68, bottom=114, rx=28, ry=34, feet=((58, 117), (80, 119)), eyes='wide', mouth='o',
                 back_arms=((52, 52, 60, 30), (84, 52, 72, 30)))
# on a wall to the left: hands and feet touch x~10
F['climb1'] = pet(cx=44, bottom=116, rx=28, ry=34, feet=((18, 120),), look=(-4, -2),
                  arms=((26, 70, 10, 60), (26, 96, 12, 100)))
F['climb2'] = pet(cx=44, bottom=114, rx=28, ry=34, feet=((18, 116),), look=(-4, -2),
                  arms=((26, 66, 10, 70), (26, 92, 12, 88)))
# under a ceiling: hands touch y~6
F['ceil1'] = pet(bottom=104, rx=28, ry=32, feet=((54, 108), (74, 108)), look=(0, -3), sprout=False,
                 back_arms=((46, 48, 40, 8), (82, 48, 88, 8)))
F['ceil2'] = pet(bottom=104, rx=28, ry=32, feet=((54, 108), (74, 108)), look=(0, -3), sprout=False,
                 back_arms=((46, 48, 34, 10), (82, 48, 82, 8)))

names = list(F)
os.makedirs(os.path.join(OUT, 'conf'), exist_ok=True)
for f in os.listdir(OUT):
    if f.endswith('.png'): os.remove(os.path.join(OUT, f))
for i, n in enumerate(names):
    F[n].save(os.path.join(OUT, f's{i}.png'))
F['stand'].crop((24, 30, 104, 110)).resize((128, 128), Image.LANCZOS).save(os.path.join(OUT, 'preview.png'))


def bounds(n):
    return F[n].getchannel('A').point(lambda a: 255 if a > 40 else 0).getbbox()


def poses(frames, anchor, still=False):
    """frames: [(name, duration, vx, vy)]"""
    return ''.join(
        f'\n\t\t\t\t<Pose Image="/s{names.index(n)}.png" ImageAnchor="{anchor[0]},{anchor[1]}" '
        f'Velocity="{0 if still else vx},{0 if still else vy}" Duration="{dur}" />'
        for n, dur, vx, vy in frames) + '\n\t\t\t'


floor = (64, 128)
wall = (min(bounds('climb1')[0], bounds('climb2')[0]), 128)
ceil = (64, min(bounds('ceil1')[1], bounds('ceil2')[1]))
walk = [('walk1', 6, -2, 0), ('walk2', 6, -2, 0), ('walk3', 6, -2, 0), ('walk2', 6, -2, 0)]
drag = [('drag1', 8, 0, 0), ('drag2', 8, 0, 0), ('drag1', 8, 0, 0), ('drag3', 8, 0, 0)]
fill = {
    'stand': poses([('stand', 120, 0, 0), ('blink', 4, 0, 0), ('stand', 80, 0, 0), ('blink', 4, 0, 0)], floor),
    'walk': poses(walk, floor),
    'run': poses([(n, 3, -4, 0) for n, *_ in walk], floor),
    'dash': poses([(n, 2, -8, 0) for n, *_ in walk], floor),
    'sit': poses([('sit', 250, 0, 0)], floor),
    'sit_down': poses([('sit', 250, 0, 0)], floor),
    'sit_look_up': poses([('sit_up', 250, 0, 0)], floor),
    'sit_dangle': poses([('dangle1', 10, 0, 0), ('dangle2', 10, 0, 0)], floor),
    'sprawl': poses([('sprawl', 250, 0, 0)], floor),
    'creep': poses([('creep1', 10, -1, 0), ('creep2', 10, -1, 0)], floor),
    'wink': poses([('stand', 10, 0, 0), ('wink', 20, 0, 0), ('stand', 10, 0, 0)], floor),
    'tripping': poses([('trip', 40, 0, 0), ('sit', 20, 0, 0)], floor),
    'bounce': poses([('bounce', 4, 0, 0), ('stand', 4, 0, 0)], floor),
    'grab_wall': poses([('climb1', 250, 0, 0)], wall, still=True),
    'climb_up': poses([('climb1', 12, 0, 0), ('climb2', 8, 0, -1)], wall),
    'climb_down': poses([('climb2', 12, 0, 0), ('climb1', 8, 0, 1)], wall),
    'grab_ceiling': poses([('ceil1', 250, 0, 0)], ceil, still=True),
    'ceiling': poses([('ceil1', 12, 0, 0), ('ceil2', 8, -1, 0)], ceil),
    'fall': poses([('fall', 250, 0, 0)], floor, still=True),
    'jump': poses([('jump', 250, 0, 0)], floor, still=True),
    'jump_off': poses([('bounce', 4, 0, 0), ('jump', 4, 0, 0)], floor),
    'drag': poses(drag, floor, still=True),
    'resist': poses([(n, 3, 0, 0) for n, *_ in drag * 3], floor, still=True),
    'drag_offset': '100',
}
with open(os.path.join(ROOT, 'assets', 'template', 'actions.xml'), encoding='utf-8') as f:
    actions = f.read()
actions = actions.replace('Generated by Popeto from a converted pet. {{name}} placeholders are filled with the pet\'s frames.',
                          'Popi, Popeto\'s default pet. Generated by scripts/make_default_pet.py.')
for k, v in fill.items():
    actions = actions.replace('{{' + k + '}}', v)
assert '{{' not in actions, 'unfilled template placeholder'
with open(os.path.join(OUT, 'conf', 'actions.xml'), 'w', encoding='utf-8', newline='\n') as f:
    f.write(actions)
with open(os.path.join(ROOT, 'assets', 'template', 'behaviors.xml'), encoding='utf-8') as f:
    behaviors = f.read().replace('Generated by Popeto for a converted pet', 'Popi, Popeto\'s default pet')
with open(os.path.join(OUT, 'conf', 'behaviors.xml'), 'w', encoding='utf-8', newline='\n') as f:
    f.write(behaviors)
print(f'{len(names)} frames -> {OUT}')

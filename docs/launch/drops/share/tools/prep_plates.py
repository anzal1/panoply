"""Derive the plates the Share drop uses from assets/src-captures (real captures of the real app).
Run from docs/launch/drops/share:  python3 tools/prep_plates.py
Nothing is invented: the result plate has only its footer button row removed (it is rebuilt in HTML,
see facts.md); every Share screen is an unmodified crop or a plain downscale."""
from PIL import Image, ImageDraw
S, O = 'assets/src-captures/', 'assets/plates/'

# result card: erase the old button row (x 560 to 1400), keep the hint text on the left
im = Image.open(S + 'bar-result-light.png').convert('RGBA')
bg = im.getpixel((580, 885))
d = ImageDraw.Draw(im)
d.rectangle([520, 822, 1400, 906], fill=bg)
im.save(O + 'result.png', optimize=True)
print('result footer bg', bg)

def crop(name, out, box):
    im = Image.open(S + name).convert('RGB').crop(box)
    im.save(O + out, optimize=True)
    print(out, im.size)

# 1810 wide, native pixels, of the 2320x1440 captures (light); the right 30 px (the scrollbar) is left out
crop('share-05-ready-to-send.png', 'devices.png', (480, 700, 2290, 1270))
crop('share-07-sending.png', 'sending.png', (480, 340, 2290, 910))
crop('share-08-send-done.png', 'done.png', (480, 340, 2290, 910))
crop('share-12-incoming-prompt.png', 'incoming.png', (480, 140, 2290, 850))
crop('share-16-qr.png', 'qr.png', (490, 860, 2290, 1410))

# the done frame shows a sliver of the next section (the top of its refresh button) in the last rows: repeat clean page rows over it
im = Image.open(O + 'done.png')
row = im.crop((0, 545, im.width, 546)).resize((im.width, im.height - 546))
im.paste(row, (0, 546))
im.save(O + 'done.png', optimize=True)

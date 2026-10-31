"""Derive the plates the Share drop uses from assets/src-captures (real captures of the real app).
Run from docs/launch/drops/share:  python3 tools/prep_plates.py

Every pixel is from a real capture. The edits, all listed in facts.md:
  * the result plate: its footer button row is erased (the buttons are redrawn in HTML);
  * the Nearby list is cut after its second row (the test-fixture rows are dropped) and closed with the card's own bottom edge;
  * "Kitchen Mac mini" in the sending and done cards is replaced by "Anzal's iPhone", copied glyph for glyph from
    the same capture's device row (same size, same colour, same renderer);
  * the incoming prompt loses its fourth file row (a test-fixture name) and "and 2 more" becomes "and 3 more", the 3
    copied from the size column ("3.4 MB"); the file count is still 6;
  * the empty middle of the wide Share cards (between the names and the buttons) is cut out so the text can be shown larger.
"""
from PIL import Image, ImageDraw
S, O = 'assets/src-captures/', 'assets/plates/'
def load(n): return Image.open(S + n).convert('RGB')

# ---- result card: erase the old button row (x 520 to 1400), keep the hint text on the left
im = Image.open(S + 'bar-result-light.png').convert('RGBA')
bg = im.getpixel((580, 885))
ImageDraw.Draw(im).rectangle([520, 822, 1400, 906], fill=bg)
im.save(O + 'result.png', optimize=True)

def glyph_patch(img, src, src_box, dst_xy, erase_box):
    """Paint erase_box with the card white, then paste src_box of src with its top-left at dst_xy."""
    ImageDraw.Draw(img).rectangle(erase_box, fill=(255, 255, 255))
    img.paste(src.crop(src_box), dst_xy)

devices, sending, done = load('share-05-ready-to-send.png'), load('share-07-sending.png'), load('share-08-send-done.png')

# "Anzal's iPhone" from the device row: x 643 to 818, ascender top y 845. The old title words start at x 790 (sending)
# and x 827 (done), ascender top y 773.
IPH = (639, 838, 823, 872)
dy = 773 - 845
for img, x0, x1 in ((sending, 790, 992), (done, 827, 1029)):
    glyph_patch(img, devices, IPH, (x0 - (643 - 639), 838 + dy), (x0 - 4, 765, x1 + 6, 796))

def join(img, y0, y1, left=(480, 1250), right=(2040, 2290)):
    """Crop rows y0..y1 and cut out the empty middle between the names (left) and the buttons (right)."""
    l = img.crop((left[0], y0, left[1], y1)); r = img.crop((right[0], y0, right[1], y1))
    out = Image.new('RGB', (l.width + r.width, y1 - y0)); out.paste(l, (0, 0)); out.paste(r, (l.width, 0))
    return out

# Nearby: heading and two device rows, then the card's own bottom edge (the strip below the last fixture row)
top = join(devices, 730, 1016)
strip = join(devices, 1245, 1276)
b = Image.new('RGB', (top.width, top.height + strip.height)); b.paste(top, (0, 0)); b.paste(strip, (0, top.height))
b.save(O + 'b_list.png', optimize=True)
join(sending, 706, 912).save(O + 'b_send.png', optimize=True)
d = join(done, 706, 912)
# the last rows show the top of the next section's refresh button: repeat a clean page row over them
row = d.crop((0, d.height - 30, d.width, d.height - 29)).resize((d.width, 14))
d.paste(row, (0, d.height - 14))
d.save(O + 'b_done.png', optimize=True)

# ---- incoming prompt: patch "2" to "3", then remove the fourth file row (y 507 to 587)
inc = load('share-12-incoming-prompt.png')
glyph_patch(inc, inc, (2159, 309, 2173, 329), (595, 617), (594, 616, 611, 639))
cut0, cut1 = 507, 587
joined = Image.new('RGB', (inc.width, inc.height - (cut1 - cut0)))
joined.paste(inc.crop((0, 0, inc.width, cut0)), (0, 0)); joined.paste(inc.crop((0, cut1, inc.width, inc.height)), (0, cut0))
joined.crop((480, 140, 2290, 772)).save(O + 'incoming.png', optimize=True)

# ---- QR page: unmodified crop
load('share-16-qr.png').crop((490, 860, 2290, 1410)).save(O + 'qr.png', optimize=True)
for n in ('b_list', 'b_send', 'b_done', 'incoming', 'qr', 'result'):
    print(n, Image.open(O + n + '.png').size)

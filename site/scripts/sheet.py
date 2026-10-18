# Contact sheets from scripts/shots.mjs output: one image per scroll moment, every width side by side, nothing cropped.
#   python3 scripts/sheet.py <shotsDir>
import sys, glob, os
from PIL import Image
d = sys.argv[1]
sizes = ['375x812', '390x844', '768x1024', '1024x768', '1280x800', '1440x900', '1920x1080', '2560x1440']
for m in ['top', 'heroEnd', 'how', 'set', 'privacy', 'end']:
    ims = [Image.open(f'{d}/{z}-{m}.png').convert('RGB') for z in sizes if os.path.exists(f'{d}/{z}-{m}.png')]
    H = 340; rows = [ims[:5], ims[5:]]
    rows = [[i.resize((round(i.width * H / i.height), H)) for i in r] for r in rows if r]
    W = max(sum(i.width for i in r) + 10 * (len(r) - 1) for r in rows)
    sheet = Image.new('RGB', (W, len(rows) * (H + 10)), (70, 70, 70))
    for y, r in enumerate(rows):
        x = 0
        for i in r: sheet.paste(i, (x, y * (H + 10))); x += i.width + 10
    sheet.save(f'{d}/sheet-{m}.jpg', quality=85)
print('sheets in', d)

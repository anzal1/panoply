# Contact sheets of the index plus six key tools, empty and result, for review. Usage: python3 scripts/sheets.py <shots dir> <out dir> <suffix>
import sys, os
from PIL import Image, ImageDraw
S, out, suf = sys.argv[1], os.path.expanduser(sys.argv[2]), sys.argv[3]
pages=[('tools index','dark-{s}-tools-result.png',None),('resize-to-kb','dark-{s}-tools/resize-to-kb-%s.png',1),('passport photo','dark-{s}-tools/passport-photo/us-passport-%s.png',1),('compress PDF','dark-{s}-tools/pdf-compress-%s.png',1),('remove background','dark-{s}-tools/remove-background-%s.png',1),('video convert (media)','dark-{s}-tools/media-%s.png',1),('OCR','dark-{s}-tools/ocr-%s.png',1)]
for w,h in [(375,812),(1440,900)]:
    s=f'{w}x{h}'; cols=[]; ch=1000 if w<1000 else 900
    for name,pat,st in pages:
        col=[]
        for state in (['result'] if st is None else ['empty','result']):
            f=S+'/'+(pat.format(s=s)%state if st else pat.format(s=s)); im=Image.open(f).convert('RGB'); sc=min(1,ch/im.height)
            col.append((name+' '+state,im.resize((int(im.width*sc),int(im.height*sc)))))
        cols.append(col)
    pad=16;lab=22; cw=[max(i.width for _,i in c) for c in cols]
    sheet=Image.new('RGB',(sum(cw)+pad*(len(cols)+1),2*(ch+lab)+pad*3),(30,30,34)); d=ImageDraw.Draw(sheet); x=pad
    for col,wd in zip(cols,cw):
        y=pad
        for label,im in col: d.text((x,y),label,fill=(220,220,225)); y+=lab; sheet.paste(im,(x,y)); y+=ch+pad
        x+=wd+pad
    sheet.save(f'{out}/sheet-dark-{w}-{suf}.png')

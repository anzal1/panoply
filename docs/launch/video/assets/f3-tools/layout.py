import json, math
from PIL import ImageFont
d=json.load(open('/Users/anzalhussainabidi/personal/panoply-video/docs/launch/video/assets/data/tools.json'))
T=d['tools']
f6=ImageFont.truetype('Geist-600.ttf',38)
def tw(s): return f6.getlength(s)-0.76*len(s)
FIX=112; GAP=14; W=1730
ROWS=[5,4,4,5,4,5,4]
nat=[FIX+tw(t['name']) for t in T]
for t,n in zip(T,nat): print(t['name'],round(n))
class L:
    def __init__(s,seed): s.s=seed
    def r(s):
        s.s=(s.s*1664525+1013904223)%4294967296; return s.s/4294967296

import itertools
def rows_of(idx,ROWS):
    p=0;rows=[]
    for n in ROWS:
        rows.append(idx[p:p+n]);p+=n
    return rows
def cost(idx,ROWS):
    rows=rows_of(idx,ROWS);c=0;ex=[]
    for r in rows:
        tot=sum(nat[i] for i in r)+GAP*(len(r)-1)
        e=(W-tot)/len(r); ex.append(e)
        if e<3: c+=(3-e)*50
        c+= max(0,e-4)*1.0
    for r in rows:
        for a,b in zip(r,r[1:]):
            if T[a]['category']==T[b]['category']: c+=8
    for a,b in zip(rows,rows[1:]):
        for k in range(min(len(a),len(b))):
            pass
    first=set(T[i]['category'] for r in rows[:2] for i in r)
    if len(first)<4: c+=200
    return c
best=None
for ROWS in ([4,4,5,4,5,4,5],[4,4,4,5,5,5,4],[4,4,5,5,4,5,4],[4,4,5,4,4,5,5],[4,4,4,5,5,4,5]):
  for seed in range(1,120):
    g=L(seed); idx=list(range(31))
    for i in range(30,0,-1):
        j=int(g.r()*(i+1)); idx[i],idx[j]=idx[j],idx[i]
    c=cost(idx,ROWS)
    for it in range(6000):
        a=int(g.r()*31);b=int(g.r()*31)
        if a==b: continue
        idx[a],idx[b]=idx[b],idx[a]
        c2=cost(idx,ROWS)
        if c2<=c: c=c2
        else: idx[a],idx[b]=idx[b],idx[a]
    if best is None or c<best[0]: best=(c,list(idx),ROWS,seed)
c,idx,ROWS,seed=best
print('cost',c,ROWS,seed)
rows=[(r,(W-(sum(nat[i] for i in r)+GAP*(len(r)-1)))/len(r)) for r in rows_of(idx,ROWS)]
score=max(e for _,e in rows);pen=0
print(seed,score,pen)
out=[]
y=0
for ri,(r,extra) in enumerate(rows):
    x=0
    for i in r:
        w=nat[i]+extra
        out.append([T[i]['id'],T[i]['name'],T[i]['category'],round(x,1),y,round(w,1),ri])
        x+=w+GAP
    y+=82+12
json.dump(out,open('slots.json','w'))
for o in out: print(o)

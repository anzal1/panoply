# Flatten the passport rule TOMLs (top-level keys only) into a JSON list for the site's rule pages.
import glob, json, os, re
d = os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../modules/builtin/id-passport-photo/rules/*.toml')
out = []
for f in sorted(glob.glob(d)):
    p = {}
    for line in open(f):
        if line.startswith('['): break
        m = re.match(r'^(\w+)\s*=\s*(.+?)\s*$', line)
        if not m: continue
        k, v = m.groups()
        if v.startswith('"') or v.startswith('['): p[k] = json.loads(v)
        elif v in ('true', 'false'): p[k] = v == 'true'
        else: p[k] = float(v) if '.' in v else int(v)
    keep = ['id','country','country_name','aliases','document','name','authority','width_mm','height_mm','dpi','width_px','height_px','head_min_pct','head_max_pct','head_min_mm','head_max_mm','background','min_kb','max_kb','size_preset','status','source_url','glasses']
    out.append({k: p[k] for k in keep if k in p})
json.dump(out, open(os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../site/src/data/passport-rules.json'), 'w'), indent=1)
print(len(out), 'rules')

# Flatten the preset TOMLs (top-level keys only) from the Panoply checkout into presets.json.
import glob, json, re, os
d = os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../modules/builtin/image-resize-to-kb/presets/*.toml')
out = []
for f in sorted(glob.glob(d)):
    p = {}
    for line in open(f):
        if line.startswith('['): break
        m = re.match(r'^(\w+)\s*=\s*(.+?)\s*$', line)
        if not m: continue
        k, v = m.groups()
        if v.startswith('"'): p[k] = json.loads(v)
        elif v.startswith('['): p[k] = json.loads(v)
        elif v in ('true', 'false'): p[k] = v == 'true'
        else: p[k] = float(v) if '.' in v else int(v)
    keep = ['id','authority','form','field','width_px','height_px','physical_mm','dpi','min_bytes_claimed','max_bytes_claimed','unit_claimed','kb_basis','baseline_only','format','status','source_url']
    out.append({k: p[k] for k in keep if k in p})
json.dump(out, open('src/presets.json', 'w'), indent=1)
print(len(out), 'presets')

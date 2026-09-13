#!/usr/bin/env python3
"""Compare measured benchmark JSON from the same kind of host. No synthetic baseline."""
import argparse
import json
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('before',type=Path);p.add_argument('after',type=Path)
p.add_argument('--max-regression',type=float,default=0.15)
a=p.parse_args();old=json.loads(a.before.read_text());new=json.loads(a.after.read_text())
for key in ['arch','os','records','index_bytes','rounds','rss_includes_index_build']:
    if old.get(key)!=new.get(key):p.error(f'incomparable {key}; rerun with matching inputs and setup')
rows=[]
for key in ['warm_p50_us','warm_p95_us','warm_p99_us','mean_packet_bytes','peak_rss_kib']:
    if old.get(key) and new.get(key) is not None:
        rows.append({'metric':key,'before':old[key],'after':new[key],'relative_change':new[key]/old[key]-1})
print(json.dumps({'comparison':rows,'warning':'Matching JSON metadata does not prove identical hardware or load. Inspect host artifacts.'},indent=2))
raise SystemExit(1 if any(r['relative_change']>a.max_regression for r in rows if r['metric']=='warm_p95_us') else 0)

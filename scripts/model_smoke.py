#!/usr/bin/env python3
"""Opt-in local inference observations. Generated shell is never executed."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary',type=Path,required=True)
p.add_argument('--model',type=Path,required=True)
p.add_argument('--index',type=Path,required=True)
a = p.parse_args()
def sha256(path):
    digest=hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda:stream.read(1024*1024),b''):digest.update(chunk)
    return digest.hexdigest()
binary=a.binary.resolve();model=a.model.resolve();index=a.index.resolve()
cases = ["show git working tree status", "stage src and commit with message 'release'", "rebuild the compose backend in the background and then follow its logs"]
rows=[]
for query in cases:
    start=time.perf_counter()
    result=subprocess.run([str(binary),'plan','--json','--index',str(index),'--model',str(model),'--allow-uninstalled',query],capture_output=True,text=True,timeout=300)
    row={'query':query,'exit':result.returncode,'elapsed_ms':(time.perf_counter()-start)*1000,'stderr':result.stderr}
    try:row['response']=json.loads(result.stdout)
    except json.JSONDecodeError:row['stdout']=result.stdout
    rows.append(row)
out={'generated_at_utc':datetime.now(timezone.utc).isoformat().replace('+00:00','Z'),
     'provenance':{'model_sha256':sha256(model),'binary_sha256':sha256(binary),
                   'index_sha256':sha256(index)},
     'observations':rows,'semantic_accuracy_verified':False,'proposals_executed':False}
(ROOT/'reports').mkdir(exist_ok=True)
(ROOT/'reports/model-smoke.json').write_text(json.dumps(out,indent=2)+'\n')
print(json.dumps(out,indent=2))
# Require at least one completed and surface-accepted generation, not task accuracy.
raise SystemExit(0 if any(row.get('response',{}).get('accepted') for row in rows) else 1)

#!/usr/bin/env python3
"""End-to-end checks of an actual built watf binary. Never execute its proposals."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time
ROOT = Path(__file__).resolve().parents[1]

def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    args = p.parse_args(); binary = args.binary.resolve()
    if not binary.is_file(): p.error('a built watf binary is required')
    observations = []
    with tempfile.TemporaryDirectory(prefix='watf-smoke-') as tmp:
        index = str(Path(tmp) / 'index.widx')
        def call(arguments: list[str], valid=(0,)):
            start = time.perf_counter()
            r = subprocess.run([str(binary), *arguments], cwd=ROOT, capture_output=True, timeout=180)
            assert r.returncode in valid, (arguments, r.returncode, r.stderr.decode(errors='replace'))
            observations.append({'operation': arguments[0], 'elapsed_ms': (time.perf_counter()-start)*1000, 'exit': r.returncode})
            return r.stdout
        call(['index', '--index', index, '--catalog', 'data/catalog.jsonl.gz', '--no-system'])
        doctor = json.loads(call(['doctor', '--index', index, '--verify', '--json']))
        assert doctor['records'] > 100_000 and doctor['integrity_verified']
        for query, budget in [('stage changes and create a commit', 4096), ('rebuild the compose backend then follow logs', 2048), ('aws ec2 describe instances filter', 4096)]:
            output = call(['search','--index', index,'--json','--max-bytes',str(budget),query], valid=(0,3))
            assert len(output) <= budget
            packet = json.loads(output); assert packet['schema_version'] == 1
            for item in packet['evidence']: assert item['source'] < len(packet['sources'])
        report = json.loads(call(['validate','--index',index,'--json','--allow-uninstalled','--plan-file','examples/plan.json']))
        assert report['accepted'] and not report['executed'] and report['approval_required']
        invalid = json.loads((ROOT/'examples/plan.json').read_text())
        invalid['steps'][0]['args'] = ['--watf-invented-flag']
        bad = Path(tmp)/'invalid.json';bad.write_text(json.dumps(invalid))
        report = json.loads(call(['validate','--index',index,'--json','--allow-uninstalled','--plan-file',str(bad)],valid=(3,)))
        assert not report['accepted'] and report['shell'] is None
        request = b'{}\n'+(ROOT/'examples/request.jsonl').read_bytes()
        result = subprocess.run([str(binary),'serve','--index',index],input=request,capture_output=True,timeout=120)
        assert result.returncode == 0, result.stderr
        lines=result.stdout.splitlines(keepends=True)
        assert len(lines)==4 and json.loads(lines[0])['status']=='error'
        for line in lines[1:]: assert len(line)<=4096 and 'id' in json.loads(line)
    output={'suite':'actual_rust_binary_cli_smoke','binary':str(binary),'checks':observations,'proposed_commands_executed':False}
    (ROOT/'reports').mkdir(exist_ok=True)
    (ROOT/'reports/runtime-smoke.json').write_text(json.dumps(output,indent=2)+'\n')
    print(json.dumps(output,indent=2))

if __name__=='__main__':main()

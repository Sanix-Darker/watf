#!/usr/bin/env python3
"""End-to-end checks of an actual built watf binary and controlled fixture."""
from __future__ import annotations
import argparse
import json
import os
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
        fixture_bin = Path(tmp) / 'bin'; fixture_bin.mkdir()
        fixture = fixture_bin / 'git'; marker = Path(tmp) / 'spawned'
        fixture.write_text(
            '#!/bin/sh\n'
            'printf "spawn\\n" >> "$WATF_SMOKE_MARKER"\n'
            '[ "$#" -eq 2 ] && [ "$1" = status ] && [ "$2" = --short ] || exit 64\n'
            'printf "fixture-status\\n"\n'
            'printf "fixture-note\\n" >&2\n'
        )
        fixture.chmod(0o700)
        env = os.environ.copy(); env.pop('WATF_MODEL', None)
        env['PATH'] = f'{fixture_bin}{os.pathsep}{env.get("PATH", "")}'
        env['WATF_SMOKE_MARKER'] = str(marker)
        abstain = {'schema_version':1,'id':'smoke-abstain','op':'resolve_exec',
                   'query':'show git status short banana','cwd':tmp,
                   'timeout_ms':10000,'max_output_bytes':256,'max_bytes':2048}
        resolve = {'schema_version':1,'id':'smoke-resolve','op':'resolve_exec',
                   'query':'show git status short','cwd':tmp,
                   'timeout_ms':10000,'max_output_bytes':256,'max_bytes':4096}
        request = (b'{}\n'+(ROOT/'examples/request.jsonl').read_bytes()
                   +json.dumps(abstain,separators=(',',':')).encode()+b'\n'
                   +json.dumps(resolve,separators=(',',':')).encode()+b'\n')
        start = time.perf_counter()
        result = subprocess.run([str(binary),'serve','--index',index],input=request,
                                capture_output=True,timeout=120,env=env)
        elapsed_ms = (time.perf_counter()-start)*1000
        assert result.returncode == 0, result.stderr
        lines=result.stdout.splitlines(keepends=True)
        assert len(lines)==6 and json.loads(lines[0])['status']=='error'
        for line in lines[1:4]: assert len(line)<=4096 and 'id' in json.loads(line)
        abstained = json.loads(lines[4]); resolved = json.loads(lines[5])
        assert len(lines[4]) <= abstain['max_bytes'] and len(lines[5]) <= resolve['max_bytes']
        assert abstained['schema_version']==1 and abstained['id']==abstain['id']
        assert abstained['status']=='abstained' and abstained['resolution']=='deterministic'
        assert abstained['model_calls']==0 and abstained['abstain_reason']=='planning_required'
        assert abstained['route_reason']=='retrieval_margin'
        candidates=abstained['candidates']
        assert [candidate['command'] for candidate in candidates] == [
            'git status','git rev-parse','git show','git branch']
        for candidate in candidates:
            assert set(candidate)=={'command','score','program_available','clause_coverage'}
            assert type(candidate['command']) is str and type(candidate['score']) in (int,float)
            assert type(candidate['program_available']) is bool
            assert type(candidate['clause_coverage']) is int
        for absent in ('validation_scope','resolved_argv','execution','command','selected_command'):
            assert absent not in abstained
        assert resolved['schema_version']==1 and resolved['id']==resolve['id']
        assert resolved['status']=='ok' and resolved['resolution']=='deterministic'
        assert resolved['model_calls']==0 and resolved['resolved_argv']==[['git','status','--short']]
        step=resolved['execution']['steps'][0]
        assert step['exit_code']==0
        assert step['stdout']=={'text':'fixture-status\n','bytes':15,'truncated':False}
        assert step['stderr']=={'text':'fixture-note\n','bytes':13,'truncated':False}
        assert marker.read_text().splitlines()==['spawn']
        observations.append({'operation':'serve_resolve_exec','elapsed_ms':elapsed_ms,
                             'exit':result.returncode,'response_bytes':len(lines[5])})
    output={'suite':'actual_rust_binary_cli_smoke','binary':'supplied-release-binary','checks':observations,
            'resolve_exec':{'authorized_fixture_commands':1,'abstentions':1,
                            'model_calls':0,'resolved_argv':['git','status','--short'],
                            'abstention_route_reason':abstained['route_reason'],
                            'abstention_candidates':[candidate['command'] for candidate in candidates],
                            'response_bytes':len(lines[5]),
                            'abstention_response_bytes':len(lines[4]),
                            'stdout_bytes':step['stdout']['bytes'],
                            'stderr_bytes':step['stderr']['bytes'],'marker_spawns':1,
                            'server_shutdown_clean':True},
            'proposed_commands_executed':True,
            'execution_scope':'authorized controlled local fixture with marker-only side effect; no generated shell'}
    (ROOT/'reports').mkdir(exist_ok=True)
    (ROOT/'reports/runtime-smoke.json').write_text(json.dumps(output,indent=2)+'\n')
    print(json.dumps(output,indent=2))

if __name__=='__main__':main()

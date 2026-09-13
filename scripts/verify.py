#!/usr/bin/env python3
"""Offline source and corpus integrity checks. This is not a Rust compilation test."""
from __future__ import annotations
import ast
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path
import re
import sys
import tomllib

ROOT=Path(__file__).resolve().parents[1]
HEX=re.compile(r'[0-9a-f]{64}')

def require(condition: bool, message: str) -> None:
    if not condition: raise AssertionError(message)

def record_check(record: dict) -> None:
    require(record['kind'] in {'command','option','input_field','example'},'unknown record kind')
    require(1<=len(record['command'])<=12,'command depth')
    for part in record['command']:
        require(0<len(part.encode())<=128 and not any(c.isspace() or ord(c)<32 for c in part),'bad command component')
    for key,limit in [('id',2048),('name',512),('summary',8192)]:
        require(len(record[key].encode())<=limit,f'{key} limit')
    require(record.get('arity','unknown') in {'none','one','two','optional','many','unknown'},'unknown arity')
    require(len(record.get('aliases',[]))<=64,'alias limit')
    require(len(record.get('choices',[]))<=8192,'choice limit')
    for value in [record['id'],record['name'],*record.get('aliases',[]),*record.get('choices',[])]:
        require(not any(ord(c)<32 or 127<=ord(c)<160 for c in value),'control character')
    source=record['source']
    require(bool(source['reference']) and len(source['reference'].encode())<=4096,'source reference')
    require(source['kind'] in {'builtin','man','help','completion','local_doc','catalog'},'source kind')
    if 'sha256' in source:require(bool(HEX.fullmatch(source['sha256'])),'source hash')
    if record['kind']=='input_field':require(not record['name'].startswith('-'),'input fields are not CLI flags')
    if record['kind']=='option':require(record['name'].startswith('-'),'option is not a flag')

def main() -> None:
    manifest=json.loads((ROOT/'data/catalog.manifest.json').read_text())
    catalog=ROOT/'data/catalog.jsonl.gz'
    counts=Counter();ids=set();keys=set();commands=set();decoded=hashlib.sha256()
    with gzip.open(catalog,'rb') as stream:
        for number,line in enumerate(stream,1):
            decoded.update(line);record=json.loads(line);record_check(record)
            key=(' '.join(record['command']),record['kind'],record['name'])
            require(record['id'] not in ids and key not in keys,f'duplicate record {number}')
            ids.add(record['id']);keys.add(key);counts[record['kind']]+=1
            require(record['source']['kind']=='catalog','non-catalog source in bulk corpus')
            if record['kind']=='command':commands.add(' '.join(record['command']))
    require(sum(counts.values())==manifest['records']>100000,'catalog count mismatch')
    require(dict(counts)==manifest['counts'],'catalog kind counts')
    compressed=hashlib.sha256(catalog.read_bytes()).hexdigest()
    # Manifest naming is owned by the builder; require both hashes occur in it.
    serialized=json.dumps(manifest)
    require(compressed in serialized and decoded.hexdigest() in serialized,'catalog hashes do not match manifest')
    require(len(manifest['inputs'])==manifest['services']==425,'source model count')
    for item in manifest['inputs']:
        require(HEX.fullmatch(item['decoded_sha256']) and HEX.fullmatch(item['compressed_sha256']),'input provenance hashes')
    core=[json.loads(line) for line in (ROOT/'data/core.jsonl').read_text().splitlines()]
    for record in core:
        record_check(record)
        if record['kind']=='command':commands.add(' '.join(record['command']))
    require(len({(tuple(r['command']),r['kind'],r['name']) for r in core})==len(core),'duplicate core record')
    samples=[json.loads(line) for line in (ROOT/'examples/complex.jsonl').read_text().splitlines()]
    require(len(samples)==len({s['id'] for s in samples})==100,'need exactly 100 unique complex samples')
    for sample in samples:
        require(sample['execution_permitted'] is False and sample['intent'] and sample['review_hazards'],'sample lacks safety or intent metadata')
        for command in sample['expected_commands']:require(command in commands,f'missing reference command {command}')
    require((ROOT/'skills/watf/SKILL.md').read_bytes()==(ROOT/'watf-skills/skills/watf/SKILL.md').read_bytes(),'skill copies diverge')
    model=json.loads((ROOT/'data/models.json').read_text())['models'][0]
    for script in ['scripts/install.sh','scripts/fetch-model.sh']:
        text=(ROOT/script).read_text();require(model['sha256'] in text and model['download_url'] in text,'model pins drifted')
    rust_files=list((ROOT/'src').rglob('*.rs'))
    for path in rust_files:
        source=path.read_text()
        for forbidden in ['std::process::Command','Command::new(', 'TcpStream::connect', 'reqwest::','unimplemented!(', 'todo!(']:
            require(forbidden not in source,f'forbidden runtime primitive {forbidden} in {path}')
    python_files=[];json_files=[];text_files=0
    for path in sorted(ROOT.rglob('*')):
        if not path.is_file() or any(part in {'target','.git','.watf','.venv','venv','__pycache__','.pytest_cache','node_modules','dist'} for part in path.relative_to(ROOT).parts):continue
        if path.suffix in {'.gz','.zip','.png','.gguf'}:continue
        try:text=path.read_text()
        except UnicodeDecodeError:continue
        text_files+=1
        require('\u2014' not in text,f'forbidden em dash in {path.relative_to(ROOT)}')
        if path.suffix=='.py':ast.parse(text,filename=str(path));python_files.append(str(path.relative_to(ROOT)))
        if path.suffix=='.json':json.loads(text);json_files.append(str(path.relative_to(ROOT)))
        if path.suffix=='.toml':tomllib.loads(text)
    schema_status='not available: optional jsonschema package'
    try:
        from jsonschema import Draft202012Validator
        schemas={path.stem:json.loads(path.read_text()) for path in (ROOT/'schemas').glob('*.json')}
        for schema in schemas.values():Draft202012Validator.check_schema(schema)
        validator=Draft202012Validator(schemas['capability.schema'])
        for record in core:validator.validate(record)
        Draft202012Validator(schemas['plan.schema']).validate(json.loads((ROOT/'examples/plan.json').read_text()))
        schema_status='four schemas checked, core records and example plan validated'
    except ImportError:pass
    tests=sum(path.read_text().count('#[test]') for folder in ['src','tests'] for path in (ROOT/folder).rglob('*.rs'))
    result={'suite':'offline_source_and_corpus_integrity','catalog_records':sum(counts.values()),'catalog_kinds':dict(counts),
            'catalog_compressed_bytes':catalog.stat().st_size,'catalog_sha256':compressed,'source_models':manifest['services'],
            'core_records':len(core),'core_command_scopes':sum(r['kind']=='command' for r in core),
            'complex_samples':len(samples),'sample_categories':dict(Counter(s['category'] for s in samples)),
            'rust_test_functions_present':tests,'rust_tests_executed':False,'rust_compilation_verified':False,
            'model_inference_verified':False,'performance_measured':False,'python_sources_parsed':len(python_files),
            'json_documents_parsed':len(json_files),'text_files_without_em_dash':text_files,'json_schema_checks':schema_status}
    (ROOT/'reports').mkdir(exist_ok=True)
    (ROOT/'reports/artifact-verification.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))

if __name__=='__main__':
    try:main()
    except (AssertionError,ValueError,KeyError) as error:
        print(f'verification failed: {error}',file=sys.stderr);raise SystemExit(1)

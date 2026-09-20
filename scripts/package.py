#!/usr/bin/env python3
"""Package an already built binary. Does not claim the binary has been tested."""
from __future__ import annotations
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--flavor', choices=['full', 'lite'], required=True)
    parser.add_argument('--target', required=True)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9_-]+', args.target):
        parser.error('invalid target')
    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error('build an executable first')
    lock = ROOT / 'Cargo.lock'
    if not lock.is_file():
        parser.error('generate and review Cargo.lock before distributing binaries')
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    info = {'version': version, 'flavor': args.flavor, 'target': args.target,
            'build_os': platform.platform(), 'source_revision': os.environ.get('GITHUB_SHA', 'unknown'),
            'rustc': subprocess.check_output(['rustc', '-Vv'], text=True).strip(),
            'binary_bytes': binary.stat().st_size,
            'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'cargo_lock_sha256': hashlib.sha256(lock.read_bytes()).hexdigest(),
            'model_bundled': False, 'test_status': 'consult associated CI artifacts'}
    files = {'watf': binary, 'Cargo.lock': lock, 'LICENSE': ROOT / 'LICENSE',
             'catalog.jsonl.gz': ROOT / 'data/catalog.jsonl.gz',
             'catalog.manifest.json': ROOT / 'data/catalog.manifest.json',
             'models.json': ROOT / 'data/models.json',
             'SKILL.md': ROOT / 'skills/watf/SKILL.md',
             'botocore-LICENSE.txt': ROOT / 'third_party/botocore-LICENSE.txt'}
    args.output.mkdir(parents=True, exist_ok=True)
    output = args.output / f'watf-v{version}-{args.target}-{args.flavor}.tar.gz'
    epoch = int(os.environ.get('SOURCE_DATE_EPOCH', '0'))
    with output.open('wb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=epoch) as gz:
        with tarfile.open(mode='w', fileobj=gz, format=tarfile.USTAR_FORMAT) as tar:
            for name in sorted([*files, 'BUILD-INFO.json']):
                data = (json.dumps(info, indent=2) + '\n').encode() if name == 'BUILD-INFO.json' else files[name].read_bytes()
                member = tarfile.TarInfo(name)
                member.size = len(data); member.mtime = epoch
                member.mode = 0o755 if name == 'watf' else 0o644
                tar.addfile(member, io.BytesIO(data))
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_name(output.name + '.sha256').write_text(f'{digest}  {output.name}\n')
    print(output)

if __name__ == '__main__':
    main()

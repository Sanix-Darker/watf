#!/usr/bin/env python3
"""Installer contract tests using an inert fake binary, never a Rust build test."""
from __future__ import annotations
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main() -> None:
    checks = []
    with tempfile.TemporaryDirectory(prefix='watf-installer-test-') as directory:
        temp = Path(directory)
        archive = temp / 'release.tar.gz'
        members = {'watf': b'#!/bin/sh\nexit 77\n', 'Cargo.lock': b'# fixture only\n',
                   'catalog.jsonl.gz': gzip.compress(b'', mtime=0), 'catalog.manifest.json': b'{}\n',
                   'SKILL.md': b'# fixture\n', 'LICENSE': b'MIT\n', 'botocore-LICENSE.txt': b'Apache-2.0\n',
                   'models.json': b'{}\n', 'BUILD-INFO.json': b'{}\n', '../../escaped': b'bad'}
        with tarfile.open(archive, 'w:gz') as tar:
            for name, data in members.items():
                info = tarfile.TarInfo(name); info.size = len(data); info.mode = 0o644
                tar.addfile(info, io.BytesIO(data))
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        base = ['sh', str(ROOT / 'scripts/install.sh'), '--local-archive', str(archive), '--sha256', digest,
                '--bin-dir', str(temp / 'bin'), '--data-dir', str(temp / 'data'), '--no-index']
        result = subprocess.run(base, capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr
        assert (temp / 'bin/watf').read_bytes() == members['watf']
        assert (temp / 'data/skills/watf/SKILL.md').is_file()
        assert not (temp / 'escaped').exists()
        checks.extend(['valid flat release installs', 'unknown archive paths not extracted', 'no-index avoids running binary'])
        bad = base.copy(); bad[bad.index('--sha256') + 1] = '0' * 64
        result = subprocess.run(bad, capture_output=True, text=True, timeout=30)
        assert result.returncode != 0 and 'mismatch' in result.stderr
        checks.append('bad checksum rejected')
        result = subprocess.run(['sh', str(ROOT/'scripts/install.sh'), '--repo', 'bad/../repo', '--no-index'], capture_output=True, text=True, timeout=30)
        assert result.returncode != 0 and 'exactly one slash' in result.stderr
        checks.append('invalid repository rejected before networking')
        result = subprocess.run(['sh', str(ROOT/'scripts/install.sh'), '--version', 'v1;bad'], capture_output=True, text=True, timeout=30)
        assert result.returncode != 0
        checks.append('invalid version rejected')
    print(json.dumps({'suite': 'shell_installer_with_inert_fixture', 'passed': len(checks), 'checks': checks}, indent=2))

if __name__ == '__main__':
    main()

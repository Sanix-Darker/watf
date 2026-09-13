#!/usr/bin/env python3
"""Create deterministic source and skill ZIPs, excluding models and build products."""
from __future__ import annotations
import argparse
import hashlib
from pathlib import Path
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
EXCLUDE = {'target', '.git', '.watf', '.venv', 'venv', '__pycache__', '.pytest_cache', 'dist', 'node_modules'}

def archive(root: Path, prefix: str, output: Path) -> int:
    count = 0
    with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as writer:
        for path in sorted(root.rglob('*')):
            relative = path.relative_to(root)
            if any(part in EXCLUDE for part in relative.parts) or path.suffix in {'.gguf', '.zip', '.pyc', '.widx'}:
                continue
            if path.is_symlink():
                raise ValueError(f'refusing source archive symlink: {relative}')
            if not path.is_file():
                continue
            name = prefix + '/' + relative.as_posix()
            info = zipfile.ZipInfo(name, (2026, 9, 11, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            mode = 0o755 if path.suffix in {'.sh', '.py'} else 0o644
            info.external_attr = (0o100000 | mode) << 16
            writer.writestr(info, path.read_bytes())
            count += 1
    with zipfile.ZipFile(output) as reader:
        assert reader.testzip() is None, 'ZIP integrity check failed'
        assert all('..' not in Path(name).parts and not name.startswith('/') for name in reader.namelist())
    return count

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'dist')
    args = parser.parse_args(); args.output.mkdir(parents=True, exist_ok=True)
    version = tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
    outputs = []
    for root, name in [(ROOT, 'watf'), (ROOT/'watf-skills', 'watf-skills')]:
        path = args.output / f'{name}-v{version}.zip'
        count = archive(root, name, path)
        print(f'{path}: {count} files, {path.stat().st_size} bytes')
        outputs.append(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n')
    (args.output/f'watf-v{version}-SHA256SUMS.txt').write_text(''.join(outputs))

if __name__ == '__main__':
    main()

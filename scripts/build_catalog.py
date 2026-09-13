#!/usr/bin/env python3
"""Maintainer-only, offline corpus builder. Requires the pinned botocore package.

Runtime watf neither imports Python nor invokes this script. Input fields are
counted separately from CLI options. No aliases or repeated inherited flags
are counted as additional records.
"""
import argparse
import collections
import gzip
import hashlib
import html
import importlib.metadata
import json
from pathlib import Path
import re
import shutil

PIN = "1.43.18"


def clean(value, limit=240):
    value = html.unescape(re.sub(r"<[^>]*>", " ", value or ""))
    value = value.replace(chr(0x2014), " - ")
    value = " ".join("".join(c if c.isprintable() else " " for c in value).split())
    return value[:limit]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("data/catalog.jsonl.gz"))
    parser.add_argument("--allow-version", action="store_true")
    args = parser.parse_args()
    import botocore
    if botocore.__version__ != PIN and not args.allow_version:
        raise SystemExit(f"Expected botocore=={PIN}, got {botocore.__version__}")
    root = Path(botocore.__file__).parent / "data"
    records = []
    sources = []
    counts = collections.Counter()
    service_counts = {}
    seen = set()

    def emit(record):
        if record["id"] in seen:
            raise ValueError(f"duplicate ID: {record['id']}")
        seen.add(record["id"])
        records.append(record)
        counts[record["kind"]] += 1

    for directory in sorted(root.iterdir()):
        candidates = sorted(directory.glob("*/service-2.json*"))
        if not candidates:
            continue
        path = candidates[-1]
        raw = path.read_bytes()
        decoded = gzip.decompress(raw) if path.suffix == ".gz" else raw
        model = json.loads(decoded)
        shapes = model.get("shapes", {})
        # AWS CLI reserves `s3` for its high-level custom commands.
        service = "s3api" if directory.name == "s3" else directory.name
        source_path = str(path.relative_to(root.parent))
        digest = hashlib.sha256(decoded).hexdigest()
        source = {"kind": "catalog", "reference": f"botocore/{botocore.__version__}/{source_path}",
                  "version": botocore.__version__, "sha256": digest}
        sources.append({"path": source_path, "decoded_sha256": digest,
                        "compressed_sha256": hashlib.sha256(raw).hexdigest(),
                        "api_version": model.get("metadata", {}).get("apiVersion")})
        before = len(records)
        nested_roots = set()
        for op_name, op in sorted(model.get("operations", {}).items()):
            if op.get("internal"):
                continue
            command_name = botocore.xform_name(op_name, "-")
            command = ["aws", service, command_name]
            cid = "/".join(command)
            emit({"id": f"{cid}#command", "command": command, "kind": "command",
                  "name": command_name, "summary": clean(op.get("documentation")) or clean(op_name),
                  "arity": "unknown", "required": False, "source": source})
            request = shapes.get(op.get("input", {}).get("shape"), {})
            required = set(request.get("required", []))
            for member, spec in sorted(request.get("members", {}).items()):
                if spec.get("internal"):
                    continue
                shape = shapes.get(spec.get("shape"), {})
                nested_roots.add(spec.get("shape"))
                name = "--" + botocore.xform_name(member, "-")
                typ = shape.get("type", "unknown")
                record = {"id": f"{cid}#{name}", "command": command, "kind": "option", "name": name,
                          "summary": clean(spec.get("documentation") or shape.get("documentation")) or clean(member),
                          "arity": "none" if typ == "boolean" else "many" if typ == "list" else "one",
                          "required": member in required, "value_type": typ, "source": source}
                if typ == "boolean":
                    record["aliases"] = ["--no-" + name[2:]]
                if shape.get("enum"):
                    record["choices"] = [str(v) for v in shape["enum"]]
                emit(record)
        visited = set()

        def visit(shape_name):
            if shape_name in visited or shape_name not in shapes:
                return
            visited.add(shape_name)
            shape = shapes[shape_name]
            typ = shape.get("type")
            if typ == "list":
                visit(shape["member"]["shape"])
            elif typ == "map":
                visit(shape["value"]["shape"])
            elif typ == "structure":
                for member, spec in sorted(shape.get("members", {}).items()):
                    if spec.get("internal"):
                        continue
                    value = shapes.get(spec.get("shape"), {})
                    record = {"id": f"aws/{service}/@input/{shape_name}/{member}",
                              "command": ["aws", service], "kind": "input_field",
                              "name": f"{shape_name}.{member}",
                              "summary": "JSON input field, not a CLI flag. " +
                                         clean(spec.get("documentation") or value.get("documentation") or member, 180),
                              "arity": "unknown", "required": member in shape.get("required", []),
                              "value_type": value.get("type", "unknown"), "source": source}
                    if value.get("enum"):
                        record["choices"] = [str(v) for v in value["enum"]]
                    emit(record)
                    visit(spec.get("shape"))
        for shape_name in sorted(n for n in nested_roots if n):
            visit(shape_name)
        service_counts[service] = len(records) - before

    records.sort(key=lambda r: r["id"])
    if len(records) <= 100_000:
        raise SystemExit(f"Corpus too small: {len(records)} unique records")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    decoded_hash = hashlib.sha256()
    with args.output.open("wb") as file:
        with gzip.GzipFile(fileobj=file, mode="wb", filename="", mtime=0, compresslevel=9) as stream:
            for record in records:
                line = (json.dumps(record, ensure_ascii=True, separators=(",", ":"), sort_keys=True) + "\n").encode()
                decoded_hash.update(line)
                stream.write(line)
    manifest = {"schema_version": 1, "source_package": "botocore", "source_version": botocore.__version__,
                "license": "Apache-2.0", "records": len(records), "counts": dict(counts),
                "services": len(service_counts), "service_counts": service_counts,
                "compressed_sha256": hashlib.sha256(args.output.read_bytes()).hexdigest(),
                "jsonl_sha256": decoded_hash.hexdigest(), "compressed_bytes": args.output.stat().st_size,
                "scope": "AWS CLI modeled operations and direct options, plus separately typed nested JSON input fields",
                "limitations": ["Not 100000 executables or independent tasks", "Not an installed command inventory",
                                "AWS CLI customizations may differ from service models", "Catalog syntax is not installed-version verified"],
                "inputs": sources}
    args.output.with_name("catalog.manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    license_files = importlib.metadata.distribution("botocore").files or []
    for file in license_files:
        if str(file).endswith("LICENSE.txt"):
            license_path = importlib.metadata.distribution("botocore").locate_file(file)
            destination = args.output.parent.parent / "third_party" / "botocore-LICENSE.txt"
            destination.parent.mkdir(exist_ok=True)
            shutil.copyfile(license_path, destination)
            break
    print(json.dumps({k: manifest[k] for k in ("records", "counts", "services", "compressed_bytes", "compressed_sha256")}, indent=2))

if __name__ == "__main__":
    main()

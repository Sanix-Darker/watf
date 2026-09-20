#!/usr/bin/env python3
"""Authorized read-only resolve and execute example using foreground JSONL."""
import json
import os
from pathlib import Path
import subprocess

MAX_BYTES = 4096
REQUEST_ID = "authorized-read-only-1"
# Set this only when the caller needs full summaries and source provenance.
NEED_FULL_EVIDENCE = True


def exchange(engine, request):
    max_bytes = request["max_bytes"]
    wire = json.dumps(request, separators=(",", ":")).encode("utf-8") + b"\n"
    engine.stdin.write(wire)
    engine.stdin.flush()
    line = engine.stdout.readline(max_bytes + 1)
    if not line:
        raise RuntimeError("watf returned no response")
    if len(line) > max_bytes or not line.endswith(b"\n"):
        raise RuntimeError("watf response exceeded its byte budget")
    try:
        response = json.loads(line)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("watf returned invalid JSON") from error
    if not isinstance(response, dict):
        raise RuntimeError("watf response is not an object")
    return response


def stop(engine):
    if engine.stdin is not None and not engine.stdin.closed:
        engine.stdin.close()
    try:
        return engine.wait(timeout=15)
    except subprocess.TimeoutExpired:
        engine.terminate()
        try:
            return engine.wait(timeout=5)
        except subprocess.TimeoutExpired:
            engine.kill()
            return engine.wait()


# The caller has already authorized this read-only operation. resolve_exec may
# directly spawn the command selected from the indexed evidence.
request = {
    "schema_version": 1,
    "id": REQUEST_ID,
    "op": "resolve_exec",
    "query": "show git status short",
    "cwd": str(Path.cwd()),
    "timeout_ms": 10_000,
    "max_output_bytes": 4096,
    "max_bytes": MAX_BYTES,
}
environment = os.environ.copy()
environment["GIT_OPTIONAL_LOCKS"] = "0"
engine = subprocess.Popen(
    ["watf", "serve"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    env=environment,
)
try:
    if engine.stdin is None or engine.stdout is None:
        raise RuntimeError("watf protocol pipes unavailable")
    response = exchange(engine, request)
    if response.get("status") == "abstained":
        if (
            response.get("schema_version") != 1
            or response.get("id") != REQUEST_ID
            or response.get("abstain_reason") != "planning_required"
            or not isinstance(response.get("candidates"), list)
        ):
            raise RuntimeError("watf returned an invalid abstention")
        # Candidates are unvalidated routing possibilities, not executable argv.
        print(json.dumps(response, indent=2))
        if NEED_FULL_EVIDENCE:
            fallback_id = REQUEST_ID + "-search"
            fallback = exchange(engine, {
                "schema_version": 1,
                "id": fallback_id,
                "op": "search",
                "query": request["query"],
                "max_bytes": MAX_BYTES,
                "limit": 16,
            })
            if fallback.get("schema_version") != 1 or fallback.get("id") != fallback_id:
                raise RuntimeError("watf search response schema or id mismatch")
            print(json.dumps(fallback, indent=2))
    elif response.get("status") == "error":
        if response.get("schema_version") != 1 or not isinstance(response.get("error"), str):
            raise RuntimeError("watf returned an invalid error response")
        print(json.dumps(response, indent=2))
    else:
        if response.get("schema_version") != 1 or response.get("id") != REQUEST_ID:
            raise RuntimeError("watf response schema or id mismatch")
        print(json.dumps(response, indent=2))
finally:
    return_code = stop(engine)
if return_code != 0:
    raise RuntimeError("watf failed")

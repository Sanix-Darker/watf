#!/usr/bin/env python3
"""Example agent-side client. Python is not a dependency of the watf engine."""
import json
import subprocess

# The caller launches only watf. watf itself launches no child commands.
with subprocess.Popen(["watf", "serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                      text=True, encoding="utf-8", bufsize=1) as engine:
    request={"id":"example-1","query":"restart compose backend then follow logs","max_bytes":4096}
    assert engine.stdin is not None and engine.stdout is not None
    engine.stdin.write(json.dumps(request)+"\n")
    engine.stdin.flush()
    line=engine.stdout.readline(4097)
    if len(line.encode("utf-8"))>4096:
        raise RuntimeError("Evidence budget exceeded")
    packet=json.loads(line)
    print(json.dumps(packet,indent=2))
    engine.stdin.close()
    if engine.wait(timeout=5):
        raise RuntimeError("watf failed")

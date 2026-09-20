#!/usr/bin/env python3
"""Tiny deterministic MCP stdio fixture for proxy benchmarks."""

import json
import subprocess
import sys


def reply(request, result):
    if "id" not in request:
        return
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}, separators=(",", ":")), flush=True)


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        reply(request, {
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "watf-bench-echo", "version": "1"},
        })
    elif method == "tools/list":
        reply(request, {"tools": [{
            "name": "echo",
            "description": "Return the supplied text unchanged.",
            "inputSchema": {
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"],
                "additionalProperties": False,
            },
        }]})
    elif method == "tools/call":
        text = request.get("params", {}).get("arguments", {}).get("text", "")
        run = subprocess.run(
            ["printf", "%s\\n", text],
            check=True,
            capture_output=True,
            text=True,
        )
        reply(request, {"content": [{"type": "text", "text": run.stdout}], "isError": False})
    elif method == "ping":
        reply(request, {})
    elif "id" in request:
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": request["id"],
            "error": {"code": -32601, "message": f"unknown method: {method}"},
        }, separators=(",", ":")), flush=True)

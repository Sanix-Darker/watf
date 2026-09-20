#!/usr/bin/env python3
"""Compare watf exact exec with a warm mcp-smart-proxy MCP task loop."""

import argparse
import json
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


def percentile(values, pct):
    values = sorted(values)
    return values[min(len(values) - 1, (len(values) * pct + 99) // 100 - 1)]


class JsonlRpc:
    def __init__(self, process):
        self.process = process
        self.next_id = 1

    def notify(self, method, params=None):
        request = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            request["params"] = params
        self.process.stdin.write((json.dumps(request, separators=(",", ":")) + "\n").encode())
        self.process.stdin.flush()

    def call(self, method, params=None):
        request_id = self.next_id
        self.next_id += 1
        request = {"jsonrpc": "2.0", "id": request_id, "method": method}
        if params is not None:
            request["params"] = params
        wire = (json.dumps(request, separators=(",", ":")) + "\n").encode()
        self.process.stdin.write(wire)
        self.process.stdin.flush()
        response_wire = self.process.stdout.readline()
        if not response_wire:
            stderr = self.process.stderr.read().decode(errors="replace")
            raise RuntimeError(f"MCP process closed: {stderr}")
        response = json.loads(response_wire)
        if response.get("id") != request_id or "error" in response:
            raise RuntimeError(response_wire.decode(errors="replace"))
        return response, len(wire), len(response_wire)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--watf", type=Path, default=Path("target/release/watf"))
    parser.add_argument("--msp", type=Path, required=True)
    parser.add_argument("--msp-config", type=Path, required=True)
    parser.add_argument("--msp-name", default="echo-fixture")
    parser.add_argument("--rounds", type=int, default=100)
    args = parser.parse_args()

    watf = args.watf.resolve()
    msp = args.msp.resolve()
    config = args.msp_config.resolve()

    with tempfile.TemporaryDirectory(prefix="watf-mcp-bench-") as temp:
        root = Path(temp)
        records = root / "records.jsonl"
        records.write_text(json.dumps({
            "id": "printf#command",
            "command": ["printf"],
            "kind": "command",
            "name": "printf",
            "summary": "Print formatted text",
            "aliases": [],
            "source": {"kind": "local_doc", "reference": str(records)},
        }, separators=(",", ":")) + "\n")
        index = root / "index.widx"
        subprocess.run([
            str(watf), "index", "--index", str(index), "--no-catalog", "--no-system",
            "--import", str(records),
        ], check=True, stdout=subprocess.DEVNULL)

        watf_server = subprocess.Popen(
            [str(watf), "serve", "--index", str(index)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        msp_server = subprocess.Popen(
            [str(msp), "--config", str(config), "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        rpc = JsonlRpc(msp_server)

        try:
            rpc.call("initialize", {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "watf-benchmark", "version": "1"},
            })
            rpc.notify("notifications/initialized")
            tools, _, _ = rpc.call("tools/list", {})
            names = {tool["name"] for tool in tools["result"]["tools"]}
            expected = {
                "activate_additional_mcps",
                "activate_tools_in_additional_mcp",
                "call_tool_in_additional_mcp",
            }
            if not expected <= names:
                raise RuntimeError(f"unexpected msp tools: {sorted(names)}")

            watf_request = (json.dumps({
                "id": "bench",
                "op": "exec",
                "argv": ["printf", "%s\\n", "hello"],
                "cwd": str(root),
                "max_bytes": 4096,
                "max_output_bytes": 4096,
            }, separators=(",", ":")) + "\n").encode()

            activate_server = {
                "name": "activate_additional_mcps",
                "arguments": {"additional_mcp_names": [args.msp_name]},
            }
            activate_tool = {
                "name": "activate_tools_in_additional_mcp",
                "arguments": {"additional_mcp_name": args.msp_name, "tool_names": ["echo"]},
            }
            call_tool = {
                "name": "call_tool_in_additional_mcp",
                "arguments": {
                    "additional_mcp_name": args.msp_name,
                    "tool_name": "echo",
                    "args_in_json": '{"text":"hello"}',
                },
            }

            def watf_once():
                start = time.perf_counter_ns()
                watf_server.stdin.write(watf_request)
                watf_server.stdin.flush()
                line = watf_server.stdout.readline()
                elapsed = (time.perf_counter_ns() - start) // 1000
                response = json.loads(line)
                if response.get("status") != "ok":
                    raise RuntimeError(line.decode(errors="replace"))
                return elapsed, len(watf_request), len(line)

            def msp_full_once():
                start = time.perf_counter_ns()
                req_bytes = resp_bytes = 0
                for payload in (activate_server, activate_tool, call_tool):
                    response, sent, received = rpc.call("tools/call", payload)
                    req_bytes += sent
                    resp_bytes += received
                elapsed = (time.perf_counter_ns() - start) // 1000
                if "hello" not in json.dumps(response, separators=(",", ":")):
                    raise RuntimeError("msp echo result missing")
                return elapsed, req_bytes, resp_bytes

            def msp_call_once():
                start = time.perf_counter_ns()
                response, sent, received = rpc.call("tools/call", call_tool)
                elapsed = (time.perf_counter_ns() - start) // 1000
                if "hello" not in json.dumps(response, separators=(",", ":")):
                    raise RuntimeError("msp echo result missing")
                return elapsed, sent, received

            watf_once()
            msp_full_once()
            msp_call_once()

            watf_runs = [watf_once() for _ in range(args.rounds)]
            msp_full_runs = [msp_full_once() for _ in range(args.rounds)]
            msp_call_runs = [msp_call_once() for _ in range(args.rounds)]

            result = {
                "suite": "equivalent_local_success_mcp_proxy",
                "rounds": args.rounds,
                "task": "return literal hello from a local deterministic capability",
                "watf": {
                    "protocol_calls_per_task": 1,
                    "model_calls": 0,
                    "p50_us": percentile([run[0] for run in watf_runs], 50),
                    "p95_us": percentile([run[0] for run in watf_runs], 95),
                    "request_mean_bytes": round(statistics.mean(run[1] for run in watf_runs)),
                    "response_mean_bytes": round(statistics.mean(run[2] for run in watf_runs)),
                    "binary_bytes": watf.stat().st_size,
                },
                "msp_full_contract": {
                    "protocol_calls_per_task": 3,
                    "model_calls": 0,
                    "p50_us": percentile([run[0] for run in msp_full_runs], 50),
                    "p95_us": percentile([run[0] for run in msp_full_runs], 95),
                    "request_mean_bytes": round(statistics.mean(run[1] for run in msp_full_runs)),
                    "response_mean_bytes": round(statistics.mean(run[2] for run in msp_full_runs)),
                    "binary_bytes": msp.stat().st_size,
                },
                "msp_call_only_after_activation": {
                    "protocol_calls_per_task": 1,
                    "model_calls": 0,
                    "p50_us": percentile([run[0] for run in msp_call_runs], 50),
                    "p95_us": percentile([run[0] for run in msp_call_runs], 95),
                    "request_mean_bytes": round(statistics.mean(run[1] for run in msp_call_runs)),
                    "response_mean_bytes": round(statistics.mean(run[2] for run in msp_call_runs)),
                },
            }
            print(json.dumps(result, indent=2))
        finally:
            for process in (watf_server, msp_server):
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)


if __name__ == "__main__":
    main()

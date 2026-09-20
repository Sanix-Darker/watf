#!/usr/bin/env python3
"""Measure direct argv, persistent watf exec, and optional output reducers."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def pct(values, p):
    values.sort()
    return values[(len(values) - 1) * p // 100]


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--binary", type=Path, default=Path("target/release/watf"))
    ap.add_argument("--rtk", type=Path)
    ap.add_argument("--banish", type=Path)
    ap.add_argument("--rounds", type=int, default=30)
    ap.add_argument("--max-output-bytes", type=int, default=1024)
    a = ap.parse_args()
    binary = a.binary.resolve()
    if not binary.is_file() or not 1 <= a.rounds <= 1000:
        ap.error("need an existing binary and rounds in 1..1000")
    if not 256 <= a.max_output_bytes <= 1048576:
        ap.error("--max-output-bytes must be in 256..1048576")
    rtk = a.rtk.resolve() if a.rtk else None
    if rtk and not rtk.is_file():
        ap.error("--rtk must point to an existing binary")
    banish = a.banish.resolve() if a.banish else None
    if banish and not banish.is_file():
        ap.error("--banish must point to an existing binary")

    with tempfile.TemporaryDirectory(prefix="watf-loop-") as tmp:
        root = Path(tmp)
        noise = root / "noise"
        noise.write_bytes(b"repeated successful build line\n" * 32768)
        command = "rg" if banish else "watf-bench-noise"
        option_records = []
        if banish:
            if not shutil.which("rg"):
                ap.error("--banish fixture requires rg in PATH")
            matches = root / "matches"
            matches.mkdir()
            line = "needle repeated successful build line\n"
            for i in range(128):
                (matches / f"f{i:03}.txt").write_text(line * 256)
            bench_args = ["rg", "-n", "needle", "matches"]
            direct_argv = bench_args
            option_records.append({
                "id": "rg#option:-n",
                "command": ["rg"],
                "kind": "option",
                "name": "-n",
                "summary": "Print line numbers",
                "aliases": ["--line-number"],
                "arity": "none",
                "required": False,
                "value_type": None,
                "choices": [],
                "source": {"kind": "local_doc", "reference": str(root / "records.jsonl")},
            })
        else:
            exe = root / command
            exe.write_text("#!/bin/sh\ncat '" + str(noise) + "'\n")
            exe.chmod(0o700)
            direct_argv = [str(exe)]
            bench_args = [command]
        records = root / "records.jsonl"
        command_record = {
            "id": command + "#command",
            "command": [command],
            "kind": "command",
            "name": command,
            "summary": "Emit deterministic repeated output",
            "aliases": [],
            "arity": "none",
            "required": False,
            "value_type": None,
            "choices": [],
            "source": {"kind": "local_doc", "reference": str(records)},
        }
        records.write_text(
            "\n".join(
                json.dumps(record, separators=(",", ":"))
                for record in [command_record, *option_records]
            )
            + "\n"
        )
        index = root / "index.widx"
        env = os.environ.copy()
        env["PATH"] = str(root) + os.pathsep + env.get("PATH", "")
        env["HOME"] = str(root)
        subprocess.run([
            str(binary), "index", "--index", str(index), "--no-catalog",
            "--no-system", "--import", str(records),
        ], cwd=root, env=env, check=True, stdout=subprocess.DEVNULL)
        server = subprocess.Popen(
            [str(binary), "serve", "--index", str(index)],
            cwd=root, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        )
        request = (json.dumps({
            "id": "bench", "op": "exec", "argv": bench_args,
            "cwd": str(root), "max_bytes": 16384,
            "max_output_bytes": a.max_output_bytes,
        }, separators=(",", ":")) + "\n").encode()

        def direct():
            start = time.perf_counter_ns()
            run = subprocess.run(direct_argv, cwd=root, env=env, capture_output=True, check=True)
            return (time.perf_counter_ns() - start) // 1000, len(run.stdout) + len(run.stderr)

        def direct_discard():
            start = time.perf_counter_ns()
            run = subprocess.run(
                direct_argv,
                cwd=root,
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            if run.returncode != 0:
                raise RuntimeError(f"direct discard failed with {run.returncode}")
            return (time.perf_counter_ns() - start) // 1000

        def watf():
            start = time.perf_counter_ns()
            server.stdin.write(request)
            server.stdin.flush()
            line = server.stdout.readline()
            elapsed = (time.perf_counter_ns() - start) // 1000
            response = json.loads(line)
            if response.get("status") != "ok":
                raise RuntimeError(line.decode(errors="replace"))
            return elapsed, len(line), response

        def run_rtk():
            start = time.perf_counter_ns()
            source = subprocess.Popen(
                direct_argv, cwd=root, env=env, stdout=subprocess.PIPE,
            )
            run = subprocess.run(
                [str(rtk), "pipe", "--filter", "log"], cwd=root, env=env,
                stdin=source.stdout, capture_output=True,
            )
            source.stdout.close()
            source_rc = source.wait()
            elapsed = (time.perf_counter_ns() - start) // 1000
            if source_rc != 0 or run.returncode != 0:
                raise RuntimeError(
                    f"rtk pipeline failed with source={source_rc}, rtk={run.returncode}: "
                    + run.stderr.decode(errors="replace")
                )
            return elapsed, len(run.stdout) + len(run.stderr)

        def run_banish():
            start = time.perf_counter_ns()
            run = subprocess.run(
                [str(banish), *bench_args], cwd=root, env=env, capture_output=True,
            )
            elapsed = (time.perf_counter_ns() - start) // 1000
            if run.returncode != 0:
                raise RuntimeError(
                    f"banish failed with {run.returncode}: "
                    + run.stderr.decode(errors="replace")
                )
            return elapsed, len(run.stdout) + len(run.stderr)

        try:
            for _ in range(3):
                direct()
                direct_discard()
                watf()
                if rtk:
                    run_rtk()
                if banish:
                    run_banish()
            direct_us, direct_discard_us, watf_us, response_bytes = [], [], [], []
            rtk_us, rtk_bytes = [], []
            banish_us, banish_bytes = [], []
            for _ in range(a.rounds):
                elapsed, raw_bytes = direct()
                direct_us.append(elapsed)
                direct_discard_us.append(direct_discard())
                elapsed, size, response = watf()
                watf_us.append(elapsed)
                response_bytes.append(size)
                if rtk:
                    elapsed, size = run_rtk()
                    rtk_us.append(elapsed)
                    rtk_bytes.append(size)
                if banish:
                    elapsed, size = run_banish()
                    banish_us.append(elapsed)
                    banish_bytes.append(size)
        finally:
            server.stdin.close()
            server.terminate()
            server.wait(timeout=5)

        p50_direct, p50_watf = pct(direct_us, 50), pct(watf_us, 50)
        p50_direct_discard = pct(direct_discard_us, 50)
        mean_response = sum(response_bytes) // len(response_bytes)
        stdout = response["execution"]["steps"][0]["stdout"]
        result = {
            "suite": "one_call_exec_full_loop",
            "fixture": "rg_supported_filter" if banish else "repeated_output",
            "rounds": a.rounds,
            "max_output_bytes": a.max_output_bytes,
            "task_success": True,
            "direct_p50_us": p50_direct,
            "direct_p95_us": pct(direct_us, 95),
            "direct_discard_p50_us": p50_direct_discard,
            "direct_discard_p95_us": pct(direct_discard_us, 95),
            "watf_exec_p50_us": p50_watf,
            "watf_exec_p95_us": pct(watf_us, 95),
            "watf_overhead_p50_us": p50_watf - p50_direct,
            "watf_overhead_vs_discard_p50_us": p50_watf - p50_direct_discard,
            "raw_command_output_bytes": raw_bytes,
            "watf_response_mean_bytes": mean_response,
            "context_byte_reduction_vs_raw_output": 1 - mean_response / raw_bytes,
            "watf_stdout_truncated": stdout["truncated"],
            "watf_protocol_calls_per_task": 1,
            "watf_model_calls": response["model_calls"],
            "note": "Direct capture measures full output transport. Direct discard is a lower-bound process baseline. Byte reduction is serialized context, not tokenizer or billing savings.",
        }
        if rtk:
            mean_rtk = sum(rtk_bytes) // len(rtk_bytes)
            result.update({
                "rtk_binary": str(rtk),
                "rtk_pipe_log_p50_us": pct(rtk_us, 50),
                "rtk_pipe_log_p95_us": pct(rtk_us, 95),
                "rtk_response_mean_bytes": mean_rtk,
                "rtk_context_byte_reduction_vs_raw_output": 1 - mean_rtk / raw_bytes,
                "rtk_processes_per_task": 2,
            })
        if banish:
            mean_banish = sum(banish_bytes) // len(banish_bytes)
            result.update({
                "banish_binary": str(banish),
                "banish_p50_us": pct(banish_us, 50),
                "banish_p95_us": pct(banish_us, 95),
                "banish_response_mean_bytes": mean_banish,
                "banish_context_byte_reduction_vs_raw_output": 1 - mean_banish / raw_bytes,
                "banish_processes_per_task": 1,
            })
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

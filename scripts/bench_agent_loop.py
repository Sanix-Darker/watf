#!/usr/bin/env python3
"""Measure complete deterministic plan and run loops against direct argv."""
import argparse
import json
import os
import select
from pathlib import Path
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "examples" / "routine.jsonl"
CASE_SPECS = {
    "routine-001": {
        "query": "show git status short",
        "command": "git status",
        "argv": ["git", "status", "--short"],
        "markers": ["A  staged-marker.txt", " M tracked.txt"],
    },
    "routine-003": {
        "query": "show current git branch",
        "command": "git branch",
        "argv": ["git", "branch", "--show-current"],
        "markers": ["main"],
    },
    "routine-004": {
        "query": "show git working tree status",
        "command": "git status",
        "argv": ["git", "status"],
        "markers": [
            "Changes to be committed:",
            "Changes not staged for commit:",
            "staged-marker.txt",
            "tracked.txt",
        ],
    },
    "routine-005": {
        "query": "show git history oneline",
        "command": "git log",
        "argv": ["git", "log", "--oneline"],
        "markers": ["benchmark base"],
    },
    "routine-006": {
        "query": "show all git branches",
        "command": "git branch",
        "argv": ["git", "branch", "--all"],
        "markers": ["main"],
    },
    "routine-007": {
        "query": "show git diff staged",
        "command": "git diff",
        "argv": ["git", "diff", "--cached"],
        "markers": ["staged-marker.txt", "staged marker"],
    },
}


def percentile(values, percent):
    if not values:
        return None
    values = sorted(values)
    return values[(len(values) - 1) * percent // 100]


def timing(values):
    return {"p50": percentile(values, 50), "p95": percentile(values, 95)}


def bounded(text, limit=500):
    return text if len(text) <= limit else text[:limit] + "...[truncated]"


def unique(values):
    output = []
    for value in values:
        if value not in output:
            output.append(value)
    return output


def load_pinned_cases():
    corpus = [json.loads(line) for line in CORPUS.read_text().splitlines() if line.strip()]
    by_id = {case.get("id"): case for case in corpus}
    if len(by_id) != len(corpus):
        raise RuntimeError("routine corpus IDs must be unique")
    cases = []
    for case_id, spec in CASE_SPECS.items():
        case = by_id.get(case_id)
        expected = {
            "command": spec["command"],
            "argv": spec["argv"],
        }
        if case is None or case.get("query") != spec["query"] or case.get("expected") != expected:
            raise RuntimeError(f"{case_id} no longer matches the agent-loop fixture")
        cases.append({**case, "markers": spec["markers"]})
    return cases


def invoke(argv, cwd, env, timeout=30):
    start = time.perf_counter_ns()
    try:
        result = subprocess.run(
            [str(arg) for arg in argv],
            cwd=cwd,
            env=env,
            capture_output=True,
            timeout=timeout,
        )
        error = None
    except (OSError, subprocess.TimeoutExpired) as caught:
        result = None
        error = bounded(str(caught))
    elapsed_us = (time.perf_counter_ns() - start) // 1000
    if result is None:
        return {
            "started": False,
            "elapsed_us": elapsed_us,
            "exit_code": None,
            "stdout": b"",
            "stderr": b"",
            "error": error,
        }
    return {
        "started": True,
        "elapsed_us": elapsed_us,
        "exit_code": result.returncode,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "error": error,
    }


def decode(data):
    try:
        return data.decode("utf-8"), None
    except UnicodeDecodeError as error:
        return data.decode("utf-8", errors="replace"), bounded(str(error))


def setup_repo(path, env):
    path.mkdir()

    def git(*args):
        subprocess.run(
            ["git", *args],
            cwd=path,
            env=env,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    git("init", "-b", "main")
    git("config", "user.name", "WATF Benchmark")
    git("config", "user.email", "watf-benchmark@example.invalid")
    git("config", "commit.gpgsign", "false")
    (path / "tracked.txt").write_text("base\n")
    git("add", "tracked.txt")
    git("commit", "-m", "benchmark base")
    (path / "staged-marker.txt").write_text("staged marker\n")
    git("add", "staged-marker.txt")
    (path / "tracked.txt").write_text("base\nunstaged marker\n")


class ServeClient:
    def __init__(self, binary, index, cwd, env):
        self.process = subprocess.Popen(
            [str(binary), "serve", "--index", str(index)], cwd=cwd, env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.sequence = 0

    def request(self, body, timeout=30):
        self.sequence += 1
        request_id = f"agent-{self.sequence}"
        wire = json.dumps({**body, "id": request_id}, separators=(",", ":")).encode() + b"\n"
        start = time.perf_counter_ns()
        try:
            self.process.stdin.write(wire)
            self.process.stdin.flush()
            ready, _, _ = select.select([self.process.stdout], [], [], timeout)
            if not ready:
                raise TimeoutError("serve response timed out")
            max_bytes = body.get("max_bytes", 4096)
            line = self.process.stdout.readline(max_bytes + 1)
            if not line:
                raise RuntimeError(f"serve ended with exit {self.process.poll()}")
            if len(line) > max_bytes or not line.endswith(b"\n"):
                raise RuntimeError("serve response exceeded its byte budget")
            response = json.loads(line)
            error = None
        except (BrokenPipeError, OSError, RuntimeError, TimeoutError, json.JSONDecodeError, UnicodeDecodeError) as caught:
            line, response, error = b"", None, bounded(str(caught))
        return {
            "elapsed_us": (time.perf_counter_ns() - start) // 1000,
            "bytes": len(line), "request_bytes": len(wire),
            "request_id": request_id, "response": response, "error": error,
        }

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        return self.process.returncode, self.process.stderr.read()


def stream(value):
    if value is None:
        return {"text": "", "bytes": 0, "truncated": False}
    return value if isinstance(value, dict) else None


def inspect_response(response, request_id, expected, markers):
    if not isinstance(response, dict):
        return None, "resolve response is not an object"
    if response.get("schema_version") != 1 or response.get("id") != request_id:
        return None, "resolve response schema version or echoed id differs"
    if response.get("status") != "ok" or response.get("resolution") != "deterministic":
        return None, "resolve response is not deterministic status ok"
    if response.get("model_calls") != 0:
        return None, "resolve response used or omitted model_calls"
    if response.get("resolved_argv") != [expected["argv"]] or response.get("step_count") != 1:
        return None, "resolved argv differs from expected argv"
    execution = response.get("execution")
    steps = execution.get("steps") if isinstance(execution, dict) else None
    if not isinstance(steps, list) or len(steps) != 1 or not isinstance(steps[0], dict):
        return None, "execution must contain exactly one step"
    step = steps[0]
    stdout, stderr = stream(step.get("stdout")), stream(step.get("stderr"))
    if stdout is None or stderr is None or not isinstance(stdout.get("text"), str) or not isinstance(stderr.get("text"), str):
        return None, "execution streams are missing"
    details = {
        "exit_code": step.get("exit_code"),
        "skipped": step.get("skipped", False),
        "timed_out": step.get("timed_out", False),
        "stdout": stdout, "stderr": stderr,
        "markers_ok": all(marker in stdout["text"] for marker in markers),
        "streams_complete": stdout.get("truncated") is False and stderr.get("truncated") is False,
    }
    ok = details["exit_code"] == 0 and not details["skipped"] and not details["timed_out"] and details["markers_ok"] and details["streams_complete"]
    return details, None if ok else "execution status, marker, or stream completeness failed"


def linux_peak_rss_kib(pid):
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmHWM:"):
                fields = line.split()
                return int(fields[1]) if len(fields) == 3 and fields[2] == "kB" else None
    except (OSError, ValueError):
        pass
    return None


def run_cold_first_task(binary, index, index_source, repo, case, env):
    start = time.perf_counter_ns()
    client = ServeClient(binary, index, repo, env)
    resolved = client.request({
        "op": "resolve_exec", "query": case["query"], "cwd": str(repo),
        "timeout_ms": 10000, "max_output_bytes": 4096, "max_bytes": 4096,
        "limit": 16, "catalog": False, "fields": False, "installed_only": False,
    })
    total_us = (time.perf_counter_ns() - start) // 1000
    execution, error = inspect_response(
        resolved["response"], resolved["request_id"], case["expected"], case["markers"]
    ) if resolved["error"] is None else (None, resolved["error"])
    expected_stdout = "".join(marker + "\n" for marker in case["markers"])
    outputs_equal = (
        execution is not None
        and execution["streams_complete"]
        and execution["stdout"]["text"] == expected_stdout
        and execution["stderr"]["text"] == ""
    )
    peak_rss = linux_peak_rss_kib(client.process.pid)
    code, stderr = client.close()
    stderr_text, stderr_error = decode(stderr)
    clean = code == 0 and not stderr and stderr_error is None
    diagnostics = []
    if error:
        diagnostics.append(error)
    if execution is not None and not outputs_equal:
        diagnostics.append("cold first-task stdout or stderr differs from fixture")
    if not clean:
        diagnostics.append("cold serve process did not shut down cleanly")
    return {
        "case_id": case["id"],
        "task_success": execution is not None and outputs_equal and clean and not diagnostics,
        "exact_argv": execution is not None,
        "outputs_equal": outputs_equal,
        "calls": 1,
        "workload_process_executions": int(execution is not None),
        "model_calls": 0,
        "fallback_documentation_reads": 0,
        "retries": 0,
        "request_bytes": resolved["request_bytes"],
        "response_bytes": resolved["bytes"],
        "total_start_to_result_us": total_us,
        "server_peak_rss_kib": peak_rss,
        "rss_scope": "serve process high-water RSS only; workload child peak is excluded",
        "page_cache_state": (
            "unspecified; temporary index was freshly built before this sample, not disk-cold"
            if index_source == "temporary_bundled"
            else "unspecified; provided index, not disk-cold"
        ),
        "response_bounded": resolved["error"] is None,
        "resolved_argv": None if not isinstance(resolved["response"], dict) else resolved["response"].get("resolved_argv"),
        "stdout": None if execution is None else execution["stdout"],
        "stderr": None if execution is None else execution["stderr"],
        "server": {"exit_code": code, "stderr": bounded(stderr_text), "clean": clean},
        "diagnostics": diagnostics,
    }


def run_case(client, repo, case, env):
    expected = case["expected"]
    direct = invoke(expected["argv"], repo, env)
    direct_stdout, stdout_error = decode(direct["stdout"])
    direct_stderr, stderr_error = decode(direct["stderr"])
    direct_markers = all(marker in direct_stdout for marker in case["markers"])
    direct_ok = direct["exit_code"] == 0 and stdout_error is None and stderr_error is None and direct_markers
    resolved = client.request({
        "op": "resolve_exec", "query": case["query"], "cwd": str(repo),
        "timeout_ms": 10000, "max_output_bytes": 4096, "max_bytes": 4096,
        "limit": 16, "catalog": False, "fields": False, "installed_only": False,
    })
    execution, error = inspect_response(resolved["response"], resolved["request_id"], expected, case["markers"]) if resolved["error"] is None else (None, resolved["error"])
    equal = execution is not None and execution["streams_complete"] and execution["stdout"]["text"] == direct_stdout and execution["stderr"]["text"] == direct_stderr
    if execution is not None and not equal and error is None:
        error = "WATF stdout or stderr differs from direct output"
    failures = [value for value in (direct["error"], stdout_error, stderr_error, error) if value]
    if not direct_markers:
        failures.append("direct output marker failed")
    return {
        "direct_success": direct_ok,
        "watf_success": execution is not None and equal and error is None,
        "exact_argv": execution is not None, "outputs_equal": equal,
        "model_calls": resolved["response"].get("model_calls") if isinstance(resolved["response"], dict) else None,
        "timing_us": {"direct": direct["elapsed_us"], "resolve_exec": resolved["elapsed_us"], "watf_total": resolved["elapsed_us"]},
        "response_bytes": {"direct": len(direct["stdout"]) + len(direct["stderr"]), "resolve_exec": resolved["bytes"]},
        "request_bytes": resolved["request_bytes"],
        "calls": {"watf": 1, "direct_process_executions": int(direct["started"]), "watf_process_executions": int(execution is not None)},
        "exit_codes": {"direct": direct["exit_code"], "watf_command": execution["exit_code"] if execution else None},
        "markers": {"direct": direct_markers, "watf": execution["markers_ok"] if execution else None},
        "truncation": {"direct": "not_applicable", "watf_stdout": execution["stdout"].get("truncated") if execution else None, "watf_stderr": execution["stderr"].get("truncated") if execution else None},
        "direct_output": {"stdout": direct_stdout, "stderr": direct_stderr},
        "watf_output": {"stdout": execution["stdout"], "stderr": execution["stderr"]} if execution else None,
        "diagnostics": failures,
    }


def run_fallback(client, repo, marker):
    start = time.perf_counter_ns()
    resolved = client.request({
        "op": "resolve_exec", "query": "show logs", "cwd": str(repo),
        "timeout_ms": 10000, "max_output_bytes": 4096, "max_bytes": 1024,
        "limit": 16, "catalog": False, "fields": False, "installed_only": False,
    })
    response = resolved["response"]
    candidates = response.get("candidates") if isinstance(response, dict) else None
    scopes = sorted({
        candidate["command"]
        for candidate in candidates or []
        if isinstance(candidate, dict)
        and candidate.get("program_available") is True
        and isinstance(candidate.get("command"), str)
        and candidate["command"].split()[-1:] == ["logs"]
    })
    resolve_ok = (
        resolved["error"] is None
        and isinstance(response, dict)
        and response.get("schema_version") == 1
        and response.get("id") == resolved["request_id"]
        and response.get("status") == "abstained"
        and response.get("resolution") == "deterministic"
        and response.get("abstain_reason") == "planning_required"
        and response.get("route_reason") == "shared_leaf_ambiguity"
        and response.get("model_calls") == 0
        and isinstance(candidates, list)
        and len(scopes) >= 3
        and "execution" not in response
        and "resolved_argv" not in response
        and "validation_scope" not in response
        and "evidence" not in response
    )
    marker_absent = not marker.exists()
    diagnostics = []
    if not resolve_ok:
        diagnostics.append(resolved["error"] or "unexpected resolve abstention")
    if not marker_absent:
        diagnostics.append("rejected fallback path spawned a candidate workload")
    return {
        "task_success": resolve_ok and marker_absent,
        "calls": 1,
        "fallback_documentation_reads": 0,
        "retries": 0,
        "model_calls": 0,
        "timing_us": {
            "resolve": resolved["elapsed_us"],
            "total": (time.perf_counter_ns() - start) // 1000,
        },
        "request_bytes": resolved["request_bytes"],
        "response_bytes": resolved["bytes"],
        "candidate_count": len(candidates) if isinstance(candidates, list) else 0,
        "available_exact_leaf_scopes": scopes,
        "marker_absent": marker_absent,
        "resolve_request_id": resolved["request_id"],
        "diagnostics": diagnostics,
    }


def fallback_metrics(runs):
    count = len(runs)
    return {
        "tasks": count,
        "task_success": sum(run["task_success"] for run in runs),
        "calls": {
            "total": sum(run["calls"] for run in runs),
            "mean_per_task": sum(run["calls"] for run in runs) / count,
        },
        "fallback_documentation_reads": sum(run["fallback_documentation_reads"] for run in runs),
        "retries": sum(run["retries"] for run in runs),
        "model_calls": sum(run["model_calls"] for run in runs),
        "timing_us": {key: timing([run["timing_us"][key] for run in runs]) for key in ("resolve", "total")},
        "request_bytes": {
            "total": sum(run["request_bytes"] for run in runs),
            "mean": sum(run["request_bytes"] for run in runs) // count,
        },
        "response_bytes": {
            "total": sum(run["response_bytes"] for run in runs),
            "mean": sum(run["response_bytes"] for run in runs) // count,
        },
        "candidate_count": {
            "min": min(run["candidate_count"] for run in runs),
            "max": max(run["candidate_count"] for run in runs),
            "mean": sum(run["candidate_count"] for run in runs) / count,
        },
        "available_exact_leaf_scopes": unique(scope for run in runs for scope in run["available_exact_leaf_scopes"]),
        "marker_absent": sum(run["marker_absent"] for run in runs),
        "failures": sum(bool(run["diagnostics"]) for run in runs),
    }


def metrics(runs):
    count = len(runs)
    direct_bytes = sum(run["response_bytes"]["direct"] for run in runs)
    watf_bytes = sum(run["response_bytes"]["resolve_exec"] for run in runs)
    calls = sum(run["calls"]["watf"] for run in runs)
    return {
        "task_success": {"direct": sum(run["direct_success"] for run in runs), "watf": sum(run["watf_success"] for run in runs), "total_per_path": count},
        "exact_argv": {"watf": sum(run["exact_argv"] for run in runs), "total": count},
        "outputs_equal": {"watf": sum(run["outputs_equal"] for run in runs), "total": count},
        "timing_us": {key: timing([run["timing_us"][key] for run in runs]) for key in ("direct", "resolve_exec", "watf_total")},
        "calls": {
            "watf": calls, "watf_mean_per_task": calls / count,
            "direct_process_executions": sum(run["calls"]["direct_process_executions"] for run in runs),
            "watf_process_executions": sum(run["calls"]["watf_process_executions"] for run in runs),
            "fallback_documentation_reads": 0, "retries": 0,
        },
        "model_calls": sum(run["model_calls"] or 0 for run in runs),
        "unknown_model_call_results": sum(run["model_calls"] is None for run in runs),
        "response_bytes": {
            "direct": direct_bytes, "resolve_exec": watf_bytes,
            "direct_agent_visible": direct_bytes, "watf_agent_visible": watf_bytes,
            "total_agent_visible": direct_bytes + watf_bytes,
            "direct_mean_per_task": direct_bytes // count, "watf_mean_per_task": watf_bytes // count,
        },
        "request_bytes": {"total": sum(run["request_bytes"] for run in runs), "mean": sum(run["request_bytes"] for run in runs) // count},
        "exit_success": {"direct": sum(run["exit_codes"]["direct"] == 0 for run in runs), "watf_command": sum(run["exit_codes"]["watf_command"] == 0 for run in runs)},
        "marker_success": {"direct": sum(run["markers"]["direct"] is True for run in runs), "watf": sum(run["markers"]["watf"] is True for run in runs)},
        "truncated_streams": sum(run["truncation"][key] is True for run in runs for key in ("watf_stdout", "watf_stderr")),
        "failures": sum(bool(run["diagnostics"]) for run in runs),
    }


def summarize(cases, results, fallback_runs, cold_first_task, rounds, index, index_source, server):
    all_runs = [run for case in cases for run in results[case["id"]]]
    case_results = []
    for case in cases:
        runs = results[case["id"]]
        item = {"id": case["id"], "query": case["query"], "expected_argv": case["expected"]["argv"], "markers": case["markers"], "rounds": rounds, **metrics(runs)}
        item["exit_codes"] = {key: unique(run["exit_codes"][key] for run in runs) for key in ("direct", "watf_command")}
        item["truncation"] = {key: unique(run["truncation"][key] for run in runs) for key in ("direct", "watf_stdout", "watf_stderr")}
        item["direct_outputs"] = unique(run["direct_output"] for run in runs)
        item["watf_outputs"] = unique(run["watf_output"] for run in runs)
        item["diagnostics"] = unique(value for run in runs for value in run["diagnostics"])
        case_results.append(item)
    return {
        "suite": "deterministic_agent_loop", "corpus": str(CORPUS),
        "case_ids": list(CASE_SPECS), "index_source": index_source, "index_path": str(index),
        "cases": len(cases), "rounds": rounds, "tasks_per_path": len(all_runs),
        "setup_excluded": ["index construction", "repository construction", "warm persistent serve startup"],
        "inherited_environment_removed": ["GIT_*", "WATF_MODEL"], "tokens": "unavailable",
        "cold_first_task": cold_first_task,
        "fallback_loop": {
            "aggregate": fallback_metrics(fallback_runs),
            "round_results": fallback_runs,
        },
        "server": server,
        "aggregate": metrics(all_runs), "case_results": case_results,
        "caveat": (
            "The direct argv path is an optimistic oracle lower bound with no planning or validation. "
            "WATF uses one request to a persistent foreground serve process and one direct workload spawn. "
            "The six fixed read-only Git tasks use a synthetic temporary repository. "
            + ("The temporary index contains bundled command surfaces. " if index_source == "temporary_bundled" else "A provided index must contain compatible bundled Git surfaces. ")
            + "Index construction, repository construction, and warm server startup are excluded. "
            "The separate cold first-task sample includes its fresh server process launch and first response, but its OS page-cache state is unspecified. "
            "WATF success requires exact expected argv, zero model calls, successful markers, and complete untruncated stdout and stderr equal to direct argv. "
            "The measured fallback loop is reported separately, has no direct-oracle comparison, and does not execute a workload. "
            "Response byte counts are not token estimates."
        ),
    }


def benchmark(binary, index, index_source, rounds, cases, root, env):
    repo = root / "repo"
    setup_repo(repo, env)
    fake_bin = root / "rejection-bin"
    fake_bin.mkdir()
    rejection_marker = root / "rejected-workload-spawned"
    script = f"#!/bin/sh\nprintf spawned >> '{rejection_marker}'\n"
    for name in ("docker", "kubectl"):
        program = fake_bin / name
        program.write_text(script)
        program.chmod(0o700)
    env["PATH"] = str(fake_bin) + os.pathsep + env.get("PATH", "")
    cold_first_task = run_cold_first_task(
        binary, index, index_source, repo, cases[0], env
    )
    client = ServeClient(binary, index, repo, env)
    results = {case["id"]: [] for case in cases}
    fallback_runs = []
    for _ in range(rounds):
        fallback_runs.append(run_fallback(client, repo, rejection_marker))
        for case in cases:
            results[case["id"]].append(run_case(client, repo, case, env))
    code, stderr = client.close()
    stderr_text, stderr_error = decode(stderr)
    server = {"exit_code": code, "stderr": bounded(stderr_text), "clean": code == 0 and not stderr and stderr_error is None}
    return summarize(
        cases, results, fallback_runs, cold_first_task, rounds, index, index_source, server
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/watf"))
    parser.add_argument("--index", type=Path)
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--self-check", action="store_true", help="check only the six pinned corpus entries; do not run WATF or Git")
    args = parser.parse_args()
    try:
        cases = load_pinned_cases()
    except (OSError, ValueError, RuntimeError) as error:
        parser.error(str(error))
    if args.self_check:
        print(json.dumps({"status": "ok", "check": "corpus_pin_integrity_only", "runtime_exercised": False, "cases": list(CASE_SPECS)}))
        return
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error("--binary must point to an existing file")
    if not 1 <= args.rounds <= 1000:
        parser.error("--rounds must be in 1..1000")
    with tempfile.TemporaryDirectory(prefix="watf-agent-loop-") as directory:
        root = Path(directory)
        env = {name: value for name, value in os.environ.items() if name != "WATF_MODEL" and not name.startswith("GIT_")}
        env.update({
            "GIT_AUTHOR_DATE": "2000-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2000-01-01T00:00:00Z",
            "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", "GIT_PAGER": "cat",
            "HOME": str(root / "home"), "LC_ALL": "C", "NO_COLOR": "1", "PAGER": "cat",
            "TERM": "dumb", "TZ": "UTC", "XDG_CONFIG_HOME": str(root / "config"),
        })
        (root / "home").mkdir()
        (root / "config").mkdir()
        if args.index:
            index = args.index.resolve()
            if not index.is_file():
                parser.error("--index must point to an existing file")
            result = benchmark(binary, index, "provided", args.rounds, cases, root, env)
        else:
            index = root / "index.widx"
            subprocess.run([str(binary), "index", "--index", str(index), "--no-system", "--no-catalog"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
            result = benchmark(binary, index, "temporary_bundled", args.rounds, cases, root, env)
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

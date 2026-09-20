#!/usr/bin/env python3
"""Measure deterministic planning accuracy and one-shot process cost."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


CORPUS = Path(__file__).resolve().parents[1] / "examples" / "routine.jsonl"
OUTCOMES = {
    "exact_match",
    "wrong_accepted",
    "correct_abstention",
    "missed_plan",
    "harness_error",
    "unexpected_failure",
}
MODEL_REQUIRED = (
    "planning needs a local model unless the request is explicit indexed command syntax"
)
INFERENCE_FIELDS = (
    "prompt_tokens",
    "generated_tokens",
    "load_ms",
    "prefill_ms",
    "generation_ms",
)


def percentile(values, percent):
    values = sorted(values)
    return values[(len(values) - 1) * percent // 100]


def load_cases():
    cases = [json.loads(line) for line in CORPUS.read_text().splitlines() if line.strip()]
    if len(cases) != 26 or len({case["id"] for case in cases}) != 26:
        raise RuntimeError("routine corpus must contain exactly 26 unique cases")
    return cases


def bounded(text, limit=500):
    return text if len(text) <= limit else text[:limit] + "...[truncated]"


def inspect_response(response):
    if not isinstance(response, dict):
        return "response is not a JSON object", None, None
    details = {
        key: response[key]
        for key in ("status", "accepted", "error", "errors", "warnings", "questions")
        if key in response
    }
    diagnostic = bounded(json.dumps(details, separators=(",", ":"))) if details else None
    inference = response.get("inference")
    if not isinstance(inference, dict):
        return diagnostic, None, None
    metrics = {key: inference.get(key) for key in INFERENCE_FIELDS}
    if any(type(metrics[key]) is not int for key in INFERENCE_FIELDS):
        return diagnostic, metrics, None
    return diagnostic, metrics, int(any(metrics.values()))


def invoke(binary, index, case, env):
    start = time.perf_counter_ns()
    try:
        run = subprocess.run(
            [
                str(binary),
                "plan",
                "--json",
                "--allow-uninstalled",
                "--index",
                str(index),
                "--",
                case["query"],
            ],
            capture_output=True,
            env=env,
        )
    except OSError as error:
        return {
            "outcome": "harness_error",
            "observed": None,
            "model_calls": None,
            "response_bytes": 0,
            "elapsed_us": (time.perf_counter_ns() - start) // 1000,
            "exit_code": None,
            "stderr": "",
            "diagnostic": bounded(str(error)),
            "response_diagnostic": None,
            "inference_metrics": None,
        }
    elapsed_us = (time.perf_counter_ns() - start) // 1000
    try:
        stdout = run.stdout.decode("utf-8")
        stderr = run.stderr.decode("utf-8")
    except UnicodeDecodeError as error:
        return {
            "outcome": "harness_error",
            "observed": None,
            "model_calls": None,
            "response_bytes": len(run.stdout) + len(run.stderr),
            "elapsed_us": elapsed_us,
            "exit_code": run.returncode,
            "stderr": bounded(run.stderr.decode("utf-8", errors="replace")),
            "diagnostic": bounded(str(error)),
            "response_diagnostic": None,
            "inference_metrics": None,
        }

    result = {
        "outcome": "unexpected_failure",
        "observed": None,
        "model_calls": None,
        "response_bytes": len(run.stdout) + len(run.stderr),
        "elapsed_us": elapsed_us,
        "exit_code": run.returncode,
        "stderr": bounded(stderr),
        "diagnostic": None,
        "response_diagnostic": None,
        "inference_metrics": None,
    }
    fallback_diagnostics = {
        MODEL_REQUIRED,
        MODEL_REQUIRED + "\n",
        "watf: " + MODEL_REQUIRED,
        "watf: " + MODEL_REQUIRED + "\n",
    }
    if run.returncode == 2 and stdout == "" and stderr in fallback_diagnostics:
        result["model_calls"] = 0
        result["outcome"] = (
            "correct_abstention" if case["expected"] is None else "missed_plan"
        )
        return result
    response = None
    parse_error = None
    if stdout:
        try:
            response = json.loads(stdout)
        except json.JSONDecodeError as error:
            parse_error = error
    if run.returncode != 0:
        if parse_error:
            result["outcome"] = "harness_error"
            result["diagnostic"] = bounded(str(parse_error))
        elif response is not None:
            diagnostic, metrics, model_calls = inspect_response(response)
            result["response_diagnostic"] = diagnostic
            result["inference_metrics"] = metrics
            result["model_calls"] = model_calls
        if run.returncode < 0:
            result["outcome"] = "harness_error"
        return result
    if parse_error or response is None:
        result["outcome"] = "harness_error"
        result["diagnostic"] = bounded(str(parse_error or "empty stdout"))
        return result
    if stderr:
        return result
    if not isinstance(response, dict):
        result["diagnostic"] = "success response is not a JSON object"
        return result
    if response.get("accepted") is not True or response.get("status") != "ok":
        result["diagnostic"] = "success response is not accepted with status ok"
        return result
    inference = response.get("inference")
    if not isinstance(inference, dict) or any(
        type(inference.get(key)) is not int for key in INFERENCE_FIELDS
    ):
        result["diagnostic"] = "success response has invalid inference metrics"
        return result
    model_calls = int(any(inference[key] != 0 for key in INFERENCE_FIELDS))
    result["model_calls"] = model_calls
    result["inference_metrics"] = {key: inference[key] for key in INFERENCE_FIELDS}
    if model_calls:
        result["diagnostic"] = "accepted plan used inference"
        return result
    if len(response.get("steps", [])) != 1:
        result["diagnostic"] = "accepted plan does not contain exactly one step"
        return result

    step = response["steps"][0]
    observed = None
    if isinstance(step, dict):
        command = step.get("command")
        args = step.get("args")
        if isinstance(command, str) and isinstance(args, list) and all(
            isinstance(arg, str) for arg in args
        ):
            observed = {"command": command, "argv": command.split() + args}
    if observed is None:
        result["diagnostic"] = "accepted plan step has invalid command or argv"
        return result

    expected = case["expected"]
    if expected is None:
        result["outcome"] = "wrong_accepted"
    elif observed == expected:
        result["outcome"] = "exact_match"
    else:
        result["outcome"] = "wrong_accepted"
    result["observed"] = observed
    return result


def benchmark(binary, index, index_source, rounds, cases, env):
    results = {case["id"]: [] for case in cases}
    for _ in range(rounds):
        for case in cases:
            results[case["id"]].append(invoke(binary, index, case, env))

    elapsed = [run["elapsed_us"] for runs in results.values() for run in runs]
    response_bytes = [run["response_bytes"] for runs in results.values() for run in runs]
    model_calls = sum(
        run["model_calls"] or 0 for runs in results.values() for run in runs
    )
    unknown_model_call_results = sum(
        run["model_calls"] is None for runs in results.values() for run in runs
    )
    fallback = sum(
        run["outcome"] in {"correct_abstention", "missed_plan"}
        for runs in results.values()
        for run in runs
    )
    case_results = []
    for case in cases:
        runs = results[case["id"]]
        outcomes = sorted({run["outcome"] for run in runs})
        observed = []
        exit_codes = []
        stderr = []
        diagnostics = []
        response_diagnostics = []
        inference_metrics = []
        for run in runs:
            if run["observed"] is not None and run["observed"] not in observed:
                observed.append(run["observed"])
            if run["exit_code"] not in exit_codes:
                exit_codes.append(run["exit_code"])
            if run["stderr"] and run["stderr"] not in stderr:
                stderr.append(run["stderr"])
            if run["diagnostic"] and run["diagnostic"] not in diagnostics:
                diagnostics.append(run["diagnostic"])
            if (
                run["response_diagnostic"]
                and run["response_diagnostic"] not in response_diagnostics
            ):
                response_diagnostics.append(run["response_diagnostic"])
            if (
                run["inference_metrics"] is not None
                and run["inference_metrics"] not in inference_metrics
            ):
                inference_metrics.append(run["inference_metrics"])
        case_results.append({
            "id": case["id"],
            "category": case.get("reason"),
            "outcome": outcomes[0] if len(outcomes) == 1 else outcomes,
            "model_calls": sum(run["model_calls"] or 0 for run in runs),
            "unknown_model_call_results": sum(
                run["model_calls"] is None for run in runs
            ),
            "exit_codes": exit_codes,
            "stderr": stderr,
            "diagnostics": diagnostics,
            "response_diagnostics": response_diagnostics,
            "inference_metrics": inference_metrics,
            "stdout_stderr_bytes_mean": sum(run["response_bytes"] for run in runs) // rounds,
            "observed": observed,
        })

    total = len(cases) * rounds
    failures = {
        outcome: sum(run["outcome"] == outcome for runs in results.values() for run in runs)
        for outcome in ("harness_error", "unexpected_failure")
    }
    failures["total"] = sum(failures.values())
    return {
        "suite": "deterministic_routine_planning",
        "corpus": str(CORPUS),
        "index_source": index_source,
        "index_path": str(index),
        "cases": len(cases),
        "rounds": rounds,
        "invocations": total,
        "calls_per_case": 1,
        "model_calls": model_calls,
        "unknown_model_call_results": unknown_model_call_results,
        "stdout_stderr_bytes_mean": sum(response_bytes) // total,
        "stdout_stderr_bytes_total": sum(response_bytes),
        "one_shot_p50_us": percentile(elapsed, 50),
        "one_shot_p95_us": percentile(elapsed, 95),
        "fallback_required_rate": fallback / total,
        "outcomes": {
            outcome: sum(run["outcome"] == outcome for runs in results.values() for run in runs)
            for outcome in sorted(OUTCOMES)
        },
        "failures": failures,
        "case_results": case_results,
        "caveat": (
            "This small fixed corpus uses hand-authored expectations and category labels. "
            + (
                "The temporary index contains bundled command surfaces. "
                if index_source == "temporary_bundled"
                else "Custom-index results require surfaces compatible with the bundled corpus. "
            )
            + "It is a regression measurement, not a held-out accuracy estimate, proof of "
            "which guard caused an abstention, or a competitor comparison. Process timing "
            "includes one-shot startup and index open."
        ),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/watf"))
    parser.add_argument("--index", type=Path)
    parser.add_argument("--rounds", type=int, default=5)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error("--binary must point to an existing file")
    if not 1 <= args.rounds <= 1000:
        parser.error("--rounds must be in 1..1000")

    cases = load_cases()
    env = os.environ.copy()
    env.pop("WATF_MODEL", None)
    if args.index:
        index = args.index.resolve()
        if not index.is_file():
            parser.error("--index must point to an existing file")
        result = benchmark(binary, index, "provided", args.rounds, cases, env)
    else:
        with tempfile.TemporaryDirectory(prefix="watf-plan-") as directory:
            index = Path(directory) / "index.widx"
            subprocess.run(
                [str(binary), "index", "--index", str(index), "--no-system", "--no-catalog"],
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                env=env,
            )
            result = benchmark(binary, index, "temporary_bundled", args.rounds, cases, env)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

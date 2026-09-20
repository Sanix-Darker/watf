# Performance methodology

Performance claims are host-specific observations, not portable guarantees.
Response sizes are serialized bytes and are never converted into token or billing
claims. Only explicitly linked JSON reports are retained machine-readable
artifacts; other measurements below are recorded prose results from the named
harnesses.

## Current retrieval and deterministic planning

Measurements used rustc 1.96.0 on Linux x86_64 with an Intel i5-8365U and a
prebuilt 51,140,952-byte mmap index containing 125,137 records. Index construction
was excluded and OS page-cache state was unspecified.

Five 20-round `benches/retrieval.rs` runs resolved 70 of 114 bundled non-AWS
self-summary probes with 100% command match among resolved probes. Route p95 was
1 us in every run. Warm retrieval p50 ranged from 183 to 188 us and p95 from
1,566 to 1,615 us. These probes measure regression consistency over command
self-summaries, not held-out natural-language accuracy.

On 2026-09-20, five independent five-round `scripts/bench_plan.py` runs used the
26 fixed cases in `examples/routine.jsonl`. Each run built a temporary bundled
index with `--no-system --no-catalog` and started one model-free
`watf plan --json --allow-uninstalled` process per case and round.

| Deterministic planning metric | Measured |
|---|---:|
| Exact expected plans | 12 of 14 |
| Wrong accepted plans | 0 of 26 |
| Correct abstentions | 12 of 12 |
| Missed expected plans | 2 of 14 |
| Harness errors or unexpected failures | 0 |
| Model calls | 0 |
| Calls per case per round | 1 |
| Mean stdout plus stderr bytes | 679 |
| One-shot process p50 range | 15,951-16,413 us |
| One-shot process p95 range | 17,151-17,394 us |

The two missed expected plans were `routine-010` and `routine-012`. Expectations
and coverage labels are hand-authored and are not derived from planner output.
The corpus is a bundled-surface regression fixture, not a held-out accuracy set.
One-shot timing includes process startup and index open.

## Current end-to-end agent loop

Execution ships in 0.0.1. `scripts/bench_agent_loop.py` measures six accepted
read-only Git tasks through one persistent foreground server and one
`resolve_exec` request per task. Five independent runs used five rounds and a
fresh deterministic temporary repository. Index and repository construction and
warm server startup were excluded. The direct path used hand-authored expected
argv as an optimistic process lower bound; it is not a competitor.

WATF success required the exact expected argv, zero inference, successful direct
argv execution, untruncated streams, exact stdout and stderr equality with the
oracle, expected markers, schema version 1, and the exact request ID.

| Warm agent-loop metric over five independent runs | Measured |
|---|---:|
| Tasks per path | 150 |
| Direct task success | 150 of 150 |
| WATF task success, exact argv, and exact output | 150 of 150 |
| Direct p50 range | 2,787-3,317 us |
| Direct p95 range | 3,096-3,748 us |
| WATF resolve plus execute p50 range | 1,743-1,979 us |
| WATF resolve plus execute p95 range | 2,020-2,375 us |
| Direct agent-visible bytes per task | 91 |
| WATF agent-visible bytes per task | 473 |
| WATF calls and workload spawns per task | 1, 1 |
| Model calls, fallback reads, retries | 0, 0, 0 |
| Truncated streams or marker failures | 0 |

This comparison favors WATF on startup because the WATF server is already running
while direct argv starts a process for every task. WATF performs intent resolution,
index-backed validation, bounded execution, and structured reporting. Tokens are
reported as unavailable because the harness does not run a tokenizer.

Each script run also measured one separate first task from immediately before
starting a fresh server through the accepted response. This includes process
launch, index open, first retrieval, planning, validation, workload execution,
capture, and serialization, but excludes index and repository construction.

| First-task metric over five independent runs | Measured |
|---|---:|
| Task success, exact argv, exact output, clean shutdown | 5 of 5 |
| Calls and workload spawns per task | 1, 1 |
| Model calls, retries, fallback reads | 0, 0, 0 |
| Start-to-result time | 16,931-35,206 us |
| Request and response bytes | 237, 416 |
| Server peak RSS | 4,320-4,488 KiB |

Linux peak RSS is the foreground server's `VmHWM` after the response and excludes
the Git child. The freshly built index may already be in the OS page cache, so
this is process-cold first-task timing, not disk-cold timing.

The same harness measured fail-closed ambiguity with marker-writing fake commands
first on PATH. Five independent five-round runs sent one bounded `resolve_exec`
request for `show logs`. Every response abstained with compact route candidates,
and no workload marker appeared.

| Compact abstention metric over five independent runs | Measured |
|---|---:|
| Task success | 25 of 25 |
| Calls per task | 1 |
| Workload spawns, model calls, retries, documentation reads | 0, 0, 0, 0 |
| Resolve p50 range | 202-275 us |
| Resolve p95 range | 237-322 us |
| Total p50 range | 280-371 us |
| Total p95 range | 315-428 us |
| Request and response bytes | 225, 653 mean |
| Response budget | 1,024 bytes |

The compact result was 83.8% smaller than the historical 4,021-byte two-call
resolve-then-search fallback. The historical path used two calls, one
documentation read, and no model, retry, or workload spawn. It is retained only
as the superseded baseline for that reduction.

## Bounded-output fixture

The final 100-round repeated-output fixture emitted 1,015,808 bytes per command.
Persistent exact `exec` measured 3,299 us p50 and 4,442 us p95 with a 519-byte
mean response, one protocol call, and zero model calls. Direct full capture
measured 3,741 us p50; direct discard measured 3,248 us p50. RTK 0.49.0 returned
101 bytes at 52,292 us p50 on its `pipe --filter log` path. This fixture favors
bounded transport, and RTK compressed it further. It does not establish general
tool superiority.

## Runtime work avoided

Deterministic search, route, exact planning, exact execution, and grounded
`resolve_exec` do not load a model. The mmap reader checks headers and queried
offsets; `doctor --verify` performs the full integrity scan. Precomputed impacts,
exact-scope lookup, reusable scratch arrays, bounded heaps, late record decoding,
and hard JSON byte limits reduce query work and agent-visible output.

These paths still allocate and perform parsing, provenance checks, validation,
and bounds checks. Do not remove trust-boundary checks to improve a synthetic
microbenchmark.

## Running the benchmarks

```sh
make lite
./target/release/watf index --no-system --index .watf/index.widx \
  --catalog data/catalog.jsonl.gz
WATF_BENCH_INDEX="$PWD/.watf/index.widx" cargo bench --bench retrieval
make bench-loop
make bench-plan
make bench-agent-loop
```

Without `WATF_BENCH_INDEX`, the retrieval harness builds an index and its process
RSS includes construction. Record compiler, CPU, kernel, RAM, build flags, index
hashes, cache state, and system load. Use at least five independent process runs.
Compare warm persistent and one-shot processes separately.

The complete agent-loop metric includes task success, calls, fallback reads,
retries, cold and warm latency, bytes, RSS, and model use. Comparisons must provide
equivalent task success and output. Byte reduction alone is not a token, cost, or
quality result.

## Optional model measurement

Use `scripts/model_smoke.py` for opt-in native generations. New runs record UTC
generation time and SHA-256 values for the model, binary, and index, along with
inference metrics and semantic review outcomes. The current
[`model-smoke.json`](../reports/model-smoke.json) contains those provenance fields.
It records one deterministic request accepted with zero inference and two
model-planned complex requests rejected fail-closed for dropped literals or order.
Semantic model accuracy remains unverified, so the model is unsuitable as the
default planner.

A GGUF file's disk size is not peak RSS. Model mappings, repacking, KV cache,
compute buffers, runtime allocations, and the retrieval index all contribute.
Only the Qwen3 adapter is implemented in 0.0.1.

# Performance methodology

There are no measured Rust latency or RSS numbers in this source delivery.
The compiler was unavailable. This document specifies reproducible measurements
instead of inventing a sub-millisecond or sub-gigabyte result.

## Implemented work avoidance

The model is absent from retrieval-only builds and never loaded by `search` or
`serve`. The mmap reader checks the header and queried offsets, not the entire
index hash per query. `doctor --verify` performs the explicit full integrity scan.
Precomputed impacts avoid query-time logarithms for every posting. A separate
local-only vocabulary avoids cloud posting scans. Exact-scope binary lookup avoids
scanning all records. Epoch scratch arrays reuse storage. Bounded heaps and late
record decoding limit materialization. JSON packets have a hard serialized byte cap.

These mechanisms still allocate strings, query terms, per-clause candidates and
JSON. This is not a zero-allocation or SIMD implementation. Parsing/provenance and
safe bounds cost CPU. Do not remove safety checks merely to improve a synthetic
microbenchmark. A custom index brings maintenance costs and is not automatically
faster than SQLite, Tantivy or every other search engine.

## Run

```sh
make lite
./target/release/watf index --no-system --index .watf/index.widx \
  --catalog data/catalog.jsonl.gz
WATF_BENCH_INDEX="$PWD/.watf/index.widx" cargo bench --bench retrieval \
  > reports/bench-local.json
```

Without `WATF_BENCH_INDEX`, the harness builds the full catalog itself. In that
case process high-water RSS includes indexing and cannot be called retrieval RSS.
With an existing index, mapping/open timing is measured but page-cache state is
explicitly unspecified. Warm query timing follows a complete scenario warm-up.
Run in a separate process for cold startup and cold-page investigations. Do not
flush system caches on a shared or production machine just to make a chart.

Capture compiler output (`rustc -Vv`), CPU, OS/kernel, RAM, build flags, index/corpus
hashes, native-library versions, idle/load conditions and whether boost/turbo was
enabled. Repeat at least five process runs on a dedicated host. Compare p50/p95/p99,
throughput and RSS, not just the best sample. GH-hosted runner results are noisy.

## Fair comparisons

Retrieval baselines must use the same installed docs, return enough information
for the same held-out tasks, and count metadata and serialization. Compare the
no-catalog local fast path separately from explicit AWS catalog searches. Benchmark
one-shot CLI startup separately from a reused `serve` process. Do not compare a
warm persistent WATF engine to a cold process without labeling the difference.

The 100 supplied inputs are scenarios, not a gold set of equivalent shell programs.
Reference-scope recall is useful for regressions but cannot prove the right flags,
argument ordering, task correctness, or that a 0.6B model is sufficient. Review
false matches and the ten abstention cases manually. Add adversarial and unfamiliar
tools from held-out captured documentation before generalizing quality claims.

## Agent context savings

Measure the actual tokenizer used by the consuming agent on both complete inputs:
raw relevant documentation and WATF's complete packet. Include request tokens,
headers, metadata, retries, fallback reads, and any added skill instructions.
Only compare successful tasks with equivalent evidence quality. Report local
CPU time and memory as well as expensive-agent input/output tokens. Byte reduction
is not a dollar savings percentage and may not correspond to fewer output tokens.

## Native inference measurements

Use `model-smoke.yml` or `scripts/model_smoke.py` for opt-in real generations.
Report model SHA256, actual prompt/output tokens, model load time, prefill time,
generation time, peak RSS and semantic review outcomes. A GGUF file's 429 MB disk
size is not total RAM usage: model mapping, repacking, KV cache, compute buffers,
runtime allocations and the retrieval index all matter.

A memory-constrained target should first improve retrieval quality and limit
context/output sizes. Do not claim that shrinking a model to 360M preserves
multi-step constraints until it passes the same held-out evaluation. Only the
Qwen3 adapter is implemented in this release.

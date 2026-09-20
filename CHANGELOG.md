# Changelog

## 0.0.1 - initial release

Initial Rust CLI with bounded ingestion, an mmap capability index, deterministic
routing and planning for grounded requests, typed plan validation, direct argv
execution, bounded output, and a foreground JSONL protocol. Default features are
empty. Local Qwen3 inference and the TUI remain opt-in features.

The crates.io package embeds 608 bootstrap records. The repository also contains
a 124,530-record offline catalog, evaluation corpora, an agent skill, release
tooling, schemas, and measured performance reports.

The default and full native builds, full tests, strict Clippy, release smoke, and
offline verification pass on the current Linux host. The pinned-model smoke report
records a deterministic request accepted with zero inference and two model-planned
requests rejected fail-closed. Semantic accuracy remains unverified, and model
planning remains unsuitable as the default path. See
[`reports/model-smoke.json`](reports/model-smoke.json) for the recorded evidence.

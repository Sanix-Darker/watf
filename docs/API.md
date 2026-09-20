# CLI and JSON API

Schema version: 1. Schemas live in `schemas/`. Their character-count constraints
are supplemented by stricter UTF-8 byte and semantic limits in the Rust engine.

## Commands

| Command | Result |
|---|---|
| `watf search QUERY` | Deterministic documentation evidence |
| `watf route QUERY` | Deterministic command-scope classification or abstention |
| `watf plan QUERY` | Deterministic exact-syntax plan when possible, otherwise local model plan plus validation |
| `watf exec -- COMMAND ARGS...` | Resolve explicit indexed syntax, validate, then execute direct argv with bounded output |
| `watf run --plan-file FILE` | Validate then execute direct argv with bounded output |
| `watf QUERY` | Plan when compiled/configured; otherwise evidence |
| `watf explain --argv-json JSON` | Indexed explanation of one literal argv |
| `watf validate --plan-file FILE` | Validate an existing draft without inference |
| `watf index` | Rebuild from built-ins, installed catalog and local manuals |
| `watf doctor --verify --json` | Index structure/hash verification and feature metadata |
| `watf serve` | Persistent foreground JSONL engine for search, route, run, exact exec, and deterministic resolve plus exec |
| `watf tui` | Optional terminal interface |

Common options: `--index FILE`, `--json`, `--stats`, `--no-mmap`.
Use `--` before literal query arguments that begin with a dash.

Retrieval options: `--command 'docker compose up'`, `--catalog`, `--fields`,
`--installed-only`, `--no-project-hints`, `--limit N`, `--max-bytes N`.

Index options: `--catalog FILE`, `--no-catalog`, `--no-system`, repeatable
`--man-dir`, `--man-file`, `--help-file`, `--doc-file`, `--completion-file`,
`--import`; `--completion-shell fish|bash|zsh` and `--command SCOPE` for scoped imports.
Only `index` interprets `--catalog` as a file argument.
The crates.io package embeds core records only. Catalog search requires catalog
data installed separately and included in an index rebuild.

Model options: `--model FILE` or `WATF_MODEL`, `--context N`,
`--output-tokens N`, `--threads N`. `--allow-uninstalled` is for inspection or
fixtures, not permission to execute a command missing from the current machine.

## Evidence response

The packet includes status, evidence, source provenance, source freshness,
clause count, uncovered clauses, truncation, estimated tokens and the estimator
name. Evidence IDs refer to indexed records. Evidence has canonical scope, kind,
name, aliases, arity, summary and a source-table index. `program_available` checks
only the root executable in absolute PATH directories.

The hard byte cap covers the complete serialized response and its newline.
A small budget removes lower-priority evidence until the packet fits, or reports
that the budget cannot hold even the metadata. It never silently returns half JSON.
The byte cap does not apply to verbose plan/validation reports.

`estimated_tokens` uses `utf8_bytes_div4_not_model_tokens`. It is not a guarantee
for any consumer's tokenizer. Clause coverage is a lexical retrieval accounting
measure, not a confidence or correctness score.

## Foreground protocol

```json
{"schema_version":1,"id":"r1","op":"search","query":"commit staged changes then rebuild containers","max_bytes":4096,"limit":16,"catalog":false,"fields":false,"installed_only":false}
```

`command` is an optional exact scope. Schema version and op default to 1 and search;
ID is required. Search/route require a query. Run requires a plan. Exec prefers
literal `argv`; `query` remains a compatibility input for explicit indexed syntax.
Resolve exec requires `query` and rejects `argv` and `plan`. Unknown keys reject
the request. ID is at most 64 UTF-8 bytes; query is at most 16 KiB; a
request line is at most 64 KiB.

Successful packets echo `id` at top level. Error records do not echo untrusted IDs;
responses are sequential, so correlate errors by request order. Parsing/validation
errors do not poison the next request. A line above the transport size limit
terminates the stream. No socket or background service is created.

The process retains one index and PATH snapshot. Atomic replacement of an index
file does not change an existing mapping; restart to observe a new index.

## Agent execution contract

The machine interface can resolve explicit indexed syntax, validate it, and
execute it in one bounded request with `op:"exec"`. It can also validate and
execute an already-grounded typed plan with `op:"run"`. Exact exec never calls
inference. Caller-supplied positional arguments stay literal, while unknown flags
still fail closed.

`op:"resolve_exec"` performs deterministic resolution and execution in one
foreground request. The caller must authorize execution under its own policy
before sending the request. It accepts the normal retrieval controls plus `query`, `cwd`,
`timeout_ms`, `max_output_bytes`, `max_bytes`, and optional `raw_output_dir`.
WATF executes only an accepted, nonempty, zero-inference plan. Ambiguous requests
and requests that need planning return `status:"abstained"` before process spawn.
WATF does not add an approval step, and validation never grants authorization or
semantic safety. It executes the validated in-memory report directly and never
renders shell text.

```json
{"id":"r3","op":"resolve_exec","query":"show git status short","cwd":"/repo","timeout_ms":10000,"max_output_bytes":4096,"max_bytes":4096}
```

A successful response reports `resolution:"deterministic"`, `route_reason`,
`model_calls:0`, `validation_scope`, `resolved_argv`, and compact execution data.
Before spawning, WATF verifies that `max_bytes` can hold the worst-case compact
status record for the resolved steps and any requested raw recovery namespace.
If the full result does not fit afterward, the compact response preserves resolved
argv, exit codes, stream byte and truncation metadata, elapsed time, and recovery
paths or namespace metadata. `output_omitted:true` identifies this form.

An abstention reports `resolution:"deterministic"`,
`abstain_reason:"planning_required"`, `route_reason`, `model_calls:0`, clause
coverage metadata, and up to four compact route candidates. Each candidate has
only its canonical command, retrieval score, root-program availability, and
clause coverage. Candidates are unvalidated routing evidence, not selected argv.
An abstention never contains `resolved_argv`, execution, validation scope, or
full evidence, and it never creates a raw-output namespace. `candidates_truncated`
is true when either the fixed four-candidate cap or response byte fitting omits
candidates. If even empty-candidate metadata cannot fit, the protocol returns
its bounded generic error instead.
Malformed or prohibited requests and index, validation, or execution failures
remain `status:"error"`.

The same exact path is available without a persistent server:

```sh
watf exec --json -- git status --short
```

Conceptually:

```json
{"id":"r1","op":"exec","argv":["git","status","--short"],"cwd":"/repo","timeout_ms":10000,"max_output_bytes":4096}
```

```json
{"id":"r2","op":"run","cwd":"/repo","timeout_ms":10000,"max_output_bytes":4096,"plan":{"status":"ok","steps":[{"command":"cargo test","args":[],"after":"start","stdout":null}],"questions":[]}}
```

The exec response reports `resolution:"exact"`, `model_calls:0`, validation
scope, and execution. It does not repeat validation warnings, evidence, the full
validated plan, rendered shell, argv, step index, skipped state, or per-step timing
because exact exec is one caller-supplied command. Execution exposes exit status,
timeout state, bounded stdout/stderr, truncation, raw recovery metadata when used,
`collapsed_lines` for repetition reduction, `filtered_lines` for deterministic
successful-output filtering, and total timing. Empty stderr is omitted. `run` and
`exec` are part of schema version 1. A missing `timed_out` field means false;
timeouts are reported explicitly as `timed_out:true`.
If a valid execution result cannot fit `max_bytes`, the response falls back to a
compact status record with per-step `exit_codes`, timeout state, elapsed time, and
`output_omitted:true`. When raw recovery files exist, fallback includes exact
`raw_paths` if they fit the same response budget; otherwise it includes
`raw_prefix` for that execution's recovery directory plus `raw_files` with the
number of recoverable streams. An impossibly small status budget is rejected
before the command runs. If recovery namespace cleanup fails, bounded errors begin
with `raw_namespace=watf-<pid>-<id>` so the namespace identity survives compaction.

Set `raw_output_dir` in JSONL requests or `--raw-output-dir DIR` on the CLI only
when omitted output may need later inspection. The default path does no recovery
file I/O. When a stream exceeds `max_output_bytes`, watf writes the exact raw
stream to a mode-0600 file on Unix and returns its path as `raw_path`. Streams
that fit the bound create no file.

## Plan schema

See `examples/plan.json` for a complete four-step fixture. `status` is `ok`,
`needs_input`, or `unsupported`. An ok plan has 1-8 steps and no unresolved
questions. A non-ok plan cannot contain steps. Each step has a canonical command,
up to 64 literal args, `after` and optional stdout redirection. Strings are bounded
and control characters are rejected by validation.

`after` is `start` for the first step, then `success`, `always` or `pipe`.
Redirection mode is `truncate` or `append`. A pipeline producer cannot also redirect
stdout to a file. The renderer emits Bash, not portable POSIX sh, when pipefail is
required. Fish and PowerShell renderers are not implemented.

`accepted` means documented surface checks passed. Validation reports keep
`executed` false because execution is represented separately by a run result.
`approval_required` remains true. `shell` is absent on rejected plans. Always
read warnings and verify intent independently, even for an accepted plan.

## Exit codes and errors

0: successful operation or evidence returned. 3: no evidence, rejected plan or
non-ok plan result. 2: argument, file, index or native inference error. CLI errors
are concise stderr messages even with `--json`; do not assume every failed command
emits JSON on stdout. `serve` uses JSON error records until a fatal transport error.

## Rust embedding

```rust
use std::{io::Write, path::Path};
use watf::packet::{Engine, Options};

fn main() -> watf::Result<()> {
    let mut engine = Engine::open(Path::new("/absolute/path/index.widx"), true)?;
    let options = Options { max_bytes: 4096, ..Options::default() };
    let packet = engine.lookup("stage changes then rebuild containers", &options)?;
    std::io::stdout().write_all(&packet.json()?)?;
    Ok(())
}
```

Use one mutable engine per sequential worker. The engine owns reusable query
scratch space. Optional native model initialization is serialized because backend
and log configuration have process-global state. Multi-tenant inference hosting
is outside this project's current contract.

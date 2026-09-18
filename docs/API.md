# CLI and JSON API

Schema version: 1. Schemas live in `schemas/`. Their character-count constraints
are supplemented by stricter UTF-8 byte and semantic limits in the Rust engine.

## Commands

| Command | Result |
|---|---|
| `watf search QUERY` | Deterministic documentation evidence |
| `watf plan QUERY` | Local model plan plus surface validation |
| `watf QUERY` | Plan when compiled/configured; otherwise evidence |
| `watf explain --argv-json JSON` | Indexed explanation of one literal argv |
| `watf validate --plan-file FILE` | Validate an existing draft without inference |
| `watf index` | Rebuild from built-ins, installed catalog and local manuals |
| `watf doctor --verify --json` | Index structure/hash verification and feature metadata |
| `watf serve` | Persistent foreground JSONL agent engine, search-only in this release |
| `watf tui` | Optional terminal interface |

Common options: `--index FILE`, `--json`, `--stats`, `--no-mmap`.
Use `--` before literal query arguments that begin with a dash.

Retrieval options: `--command 'docker compose up'`, `--catalog`, `--fields`,
`--installed-only`, `--no-project-hints`, `--limit N`, `--max-bytes N`.

Index options: `--catalog FILE`, `--no-catalog`, `--no-system`, repeatable
`--man-dir`, `--man-file`, `--help-file`, `--doc-file`, `--completion-file`,
`--import`; `--completion-shell fish|bash|zsh` and `--command SCOPE` for scoped imports.
Only `index` interprets `--catalog` as a file argument.

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
ID and query are required. Unknown keys reject the request. ID is at most 64 UTF-8
bytes; query is at most 16 KiB; a request line is at most 64 KiB.

Successful packets echo `id` at top level. Error records do not echo untrusted IDs;
responses are sequential, so correlate errors by request order. Parsing/validation
errors do not poison the next request. A line above the transport size limit
terminates the stream. No socket or background service is created.

The process retains one index and PATH snapshot. Atomic replacement of an index
file does not change an existing mapping; restart to observe a new index.

## Target agent contract, not shipped yet

The final machine interface should collapse the normal agent CLI loop into one
bounded request. It should accept intent plus execution constraints, route through
the cheapest sufficient resolver, validate structured argv, execute directly, and
return only the information needed for the next agent decision.

Conceptually:

```json
{"op":"run","query":"show the last failing test","cwd":"/repo","timeout_ms":10000,"max_output_bytes":4096}
```

The response should expose the route used (`exact`, `local`, `classifier`, or
`planner`), validated argv/steps, exit status, bounded stdout/stderr, truncation,
timing, and provenance. It must not imply that validation grants authorization.

`run` is a target operation, not part of schema version 1. Do not add it to the
published schema until the executor and output reducer are implemented and tested.

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

`accepted` means documented surface checks passed. In schema version 1,
`executed` is always false and
`approval_required` is always true. `shell` is absent on rejected plans. Always
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

# Limits and verification gaps

Deterministic search, grounded planning, validation, and execution do not require
a model. The optional model handles synthesis fallback.

| Area | Current boundary |
|---|---|
| Verification | The lite and full native builds plus `make test-full` pass on the current host with `LIBCLANG_PATH=/usr/lib/llvm-10/lib`. The pinned-model report records one deterministic request accepted with zero inference and two model-planned requests rejected fail-closed for dropped literals or order. Semantic model accuracy remains unverified |
| Corpus size | The dated reproducible benchmark fixture combines 124,530 catalog records and 608 bootstrap records into 125,137 unique records after deduplication. The crates.io package embeds bootstrap records only |
| Corpus coverage | Primarily AWS; 117 common-tool bootstrap scopes are a conservative subset |
| Installed compatibility | Root executable detection only; no active version or plugin probing |
| Documentation capture | Existing files only; no automatic `tool --help` execution |
| Man parsing | Partial roff/mdoc, plain or gzip; no includes or interpreter |
| Completions | Conservative static subsets, not arbitrary shell-program understanding |
| Local docs | Bounded section/example indexing, not source-code semantic search |
| Retrieval | Lexical plus a small synonym map, not universal semantic understanding |
| Planning | Exact indexed command/option syntax is deterministic; ambiguous synthesis uses the optional Qwen3 adapter and bounded straight-line IR, with no general DAG/loop language |
| Model quality | The current pinned-model report records dropped literals or ordering in both model-planned complex requests, which validation rejected fail-closed. Semantic accuracy is not verified and this path is not suitable as the default planner |
| Validation | Documented command/flag surface plus required-literal preservation, retrieved clause-order checks, and uniquely grounded option checks for model synthesis; still not general semantic correctness or safety |
| Execution | Direct validated argv only; no shell grammar, environment mutation, or general workflow runtime; Unix timeouts kill spawned process groups and descendants that remain in them, then wait on direct children. A descendant that creates a new session with `setsid` is outside this executor contract |
| Result reduction | Bounded head/tail stdout/stderr capture with opt-in exact raw recovery for truncated streams; successful Cargo progress gets a narrow deterministic reducer with a bounded warning/error reserve, while other command families still use generic repetition reduction |
| Shells | Bash renderer; no Fish/PowerShell renderer or stateful shell interpreter |
| Output budget | Hard bytes, approximate token estimate, no model-independent token bound |
| Networking | None in the engine; installation and proposed external tools may need it |
| User interface | Basic TUI; no shell widget, clipboard utility or asynchronous model cancellation |
| Index updates | Full rebuild, imports must be repeated, no incremental watcher |
| Persistent mode | Foreground stdio only; no daemon, hot reload, MCP or network server |
| Portability | The published `x86_64-unknown-linux-musl` asset passes ELF/readelf checks. The published asset passed a release-session smoke test on a glibc 2.31 host. No repository report records that run. This does not establish broad Linux, macOS, or Windows compatibility |
| Distribution | The crates.io 0.0.1 package, GitHub release with the musl asset, and standalone skill release are published. Full local-model and additional targets remain manual; no model is bundled |

`search` and `serve` return evidence directly, and exact indexed command syntax
with documented option arity can be planned without a model. Single-clause routed natural language may also
add uniquely grounded no-value options from bounded retrieved evidence. Other
flags, value-bearing options, literals, redirects, pipelines, negation, and
ambiguous clauses still escalate. Independently decisive multi-clause intents may
become ordered zero-argument steps when every clause passes the same grounding
checks.
Natural-language positional arguments remain outside that fast path because the
current index does not prove their semantics; explicit `exec` argv keeps
caller-supplied positionals literal. Unknown syntax may require importing better
local documentation or abstaining.

Model synthesis is additionally constrained by retrieved query-clause provenance
and uniquely grounded option semantics. Generated commands may not move backward
across clause order or omit a retained retrieved clause, explicit literals must
survive into argv, and an original query term that maps to exactly one option in
the bounded synthesis context must retain that option. These checks reject several
observed small-model failures, but they do not prove arbitrary flag combinations,
positionals, or same-clause command sequences satisfy the user's intent.

The current end-to-end benchmark covers six read-only Git tasks and deterministic
abstention through one persistent server. It checks exact argv and output, calls,
spawns, retries, and response bytes, but it is not broad workload evidence. See
[`PERFORMANCE.md`](PERFORMANCE.md) for the measured scope and caveats.

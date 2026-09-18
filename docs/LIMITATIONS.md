# Deliberate limitations and verification gaps

This release implements retrieval, deterministic routing, planning, validation,
direct execution, and bounded result capture.
Important current limits are explicit:

| Area | Current boundary |
|---|---|
| Verification | Rust builds/tests/model runs were not possible in the assembly environment |
| Corpus size | 124,530 typed records, including 41,613 nested fields; only 82,917 direct commands/options |
| Corpus coverage | Primarily AWS; 117 common-tool bootstrap scopes are a conservative subset |
| Installed compatibility | Root executable detection only; no active version or plugin probing |
| Documentation capture | Existing files only; no automatic `tool --help` execution |
| Man parsing | Partial roff/mdoc, plain or gzip; no includes or interpreter |
| Completions | Conservative static subsets, not arbitrary shell-program understanding |
| Local docs | Bounded section/example indexing, not source-code semantic search |
| Retrieval | Lexical plus a small synonym map, not universal semantic understanding |
| Planning | Optional Qwen3 adapter, bounded straight-line IR, no general DAG/loop language |
| Model quality | 0.6B multi-step accuracy not verified, no automatic model-size escalation |
| Validation | Documented command/flag surface only, not semantic correctness or safety |
| Execution | Direct validated argv only; no shell grammar, environment mutation, or general workflow runtime |
| Result reduction | Bounded head/tail stdout/stderr capture; command-specific semantic reducers are not implemented |
| Shells | Bash renderer; no Fish/PowerShell renderer or stateful shell interpreter |
| Output budget | Hard bytes, approximate token estimate, no model-independent token bound |
| Networking | None in the engine; installation and proposed external tools may need it |
| User interface | Basic TUI; no shell widget, clipboard utility or asynchronous model cancellation |
| Index updates | Full rebuild, imports must be repeated, no incremental watcher |
| Persistent mode | Foreground stdio only; no daemon, hot reload, MCP or network server |
| Portability | Linux release layouts; core CI also targets macOS; no Windows contract |
| Distribution | Source ZIP; no binary/model/live release URL included |

The model is not a mandatory final step for agents. `search` and `serve` return
evidence directly. There is no deterministic shortcut that fabricates a complete
multi-step command merely because retrieval scores are high. Unknown syntax may
require importing better local documentation or abstaining.

Further work should improve end-to-end agent benchmarks: fewer tokens, tool calls,
fallback reads, and retries at equivalent task success.

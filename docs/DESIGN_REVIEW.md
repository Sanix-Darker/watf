# Product and design review

## Existing tools

Existing tools already cover parts of this workflow.

- Natural-language command generation is a standard capability of coding agents.
- RTK and banish already proxy common developer commands and reduce shell output
  before it reaches the model.
- MCP smart proxies already reduce tool-schema context by exposing a small
  discovery surface and activating tools on demand.
- Agent terminals and coding-agent CLIs already execute shell commands with
  permission controls.
- Completion specs, man pages, and projects such as tldr already provide compact
  command metadata and examples.

`watf` combines these operations behind one local agent interface:

```text
intent
  -> capability resolution
  -> grounded typed argv
  -> deterministic validation
  -> execution
  -> bounded result reduction
```

Complete-loop benchmarks must compare task success, model tokens, tool calls,
fallback reads, retries, bytes, and wall-clock time with direct shell use and
existing reducers.

### Comparison set

| Alternative | Already solves | What `watf` must prove beyond it |
|---|---|---|
| RTK | Very fast command-output filtering for common developer CLIs | Resolve unfamiliar CLI intent and validated argv before execution, then reduce the result in the same call |
| banish | Command-output compaction plus recoverable raw output | Add grounded capability discovery and typed execution rather than only wrapping known commands |
| MCP smart proxies | Reduce prompt bloat by discovering tool schemas on demand | Resolve ordinary local CLI capabilities without turning every executable into a separate MCP schema |
| GitHub Copilot CLI and similar agents | Model-driven shell execution, permissions, and multi-step tasks | Remove model work from routine command discovery and syntax resolution |
| AetherShell | Agent-native typed shell, structured output, effect gating, result handles | Work with the existing CLI ecosystem without requiring a new shell language or reimplementing every command |
| tldr and completion specs | Compact command examples, flags, and argument metadata | Join metadata to local availability, multi-command planning, validation, execution, and bounded results |

### Routing evaluation

Do not replace lexical retrieval with a model. Use the existing index as the
first classifier and measure a simple confidence signal such as top-candidate
coverage plus score margin. Exact command mentions and sufficiently separated
matches should remain entirely local.

A typed ambiguity classifier is a target direction, not a shipped 0.0.1 path. It
may choose or abstain only among a closed set of retrieved commands and must not
invent command names or flags. This keeps classification separate from synthesis.

Benchmark this routing stack against direct retrieval and direct LLM planning:

```text
exact/local classifier -> grounded argv -> validate/direct execute
ambiguous -> future typed classifier -> grounded argv -> validate/direct execute
needs synthesis -> optional local planner -> validate/direct execute
```

Track the percentage of intents resolved at each rung. The main classifier metric
is not raw accuracy alone; it is successful tasks per token and per millisecond,
including escalation cost.

The project scope is one local interface for indexed capability resolution,
validated argv, direct execution, and bounded results.

## Module contracts

| Module | Contract | Decision |
|---|---|---|
| discover | Inspect PATH and project marker metadata without executing binaries | No help probing, shell evaluation, or subprocesses |
| ingest | Convert bounded local text and compressed manuals into attributed records | Never source completions or invoke man/groff |
| record | Distinguish commands, options, nested input fields, and examples | A JSON input field is not a shell flag |
| index | Immutable portable on-disk index with cheap reads | Sorted dictionary, precomputed BM25 impacts, fixed-width postings, mmap |
| retrieval | Search multiple intent clauses across all commands | Clause coverage and command diversity, not one mandatory tool argument |
| packet | Return useful evidence within an actual serialized byte limit | Token counts are estimates, not guarantees |
| plan | Generate argv-based multi-step IR | No raw shell generation |
| validate | Check evidence for syntax and diagnose unsupported constructs | Never claim proof of intent satisfaction or calibrated confidence |
| native inference | Load one local GGUF only when planning | Optional compiled-in llama.cpp, no server or model download at runtime |
| skill | Make deterministic CLI resolution the agent's first path | Keep returned context minimal and bounded |

## Known failure modes

Lexical retrieval and the 0.6B model do not prove intent correctness. Retrieval
quality and model planning quality require measured task results. The project does
not claim zero hallucinations.

An installed executable does not prove that catalog flags match its version.
Installed availability, source origin, captured version, and source staleness
must remain separate. A catalog is not an inventory of this machine.

An option missing from a conservative man parser is unknown, not necessarily
unsupported by the real program. Validation fails closed for generated plans.
The returned evidence remains useful to agents even when planning abstains.

`rg` returns 1 when it finds no matches. A command joined with `&&` can therefore
stop a valid workflow. Pipeline success, redirections, literal filenames,
existing staged changes, and missing commit messages require review. There is
no automatic parallel execution or guessed commit message.

The deterministic fast path returns grounded validated argv when its routing and
construction checks pass, including through `resolve_exec`; otherwise it returns
evidence or abstains. BM25 is a relevance score and is never exposed as a
correctness probability.

A large cloud-derived catalog must not dominate small local tools. Generic
queries suppress catalog records unless the user asks for catalog search or
mentions the catalog tool. Nested fields stay out of normal command planning.

Input limits, atomic index publication, safe quoting, no executable completion
parsing, no network client, and an explicit capability schema are mandatory.

Rust compilation, tests, runtime smoke, and model observations have been run on
the current Linux verification host. The optional model remains non-default and
its smoke result is evidence, not semantic validation.

## Cross-module verification

Packet handling distinguishes duplicate evidence from truncation. The persistent
protocol's approximate token count includes its request-ID envelope. Owned index
reads stay bounded if a file grows after its metadata check. Regression cases
cover these boundaries, malformed source digests, and control characters on the
recorded Linux verification host.

Installer tests exercise checksum failure, rejected repository/version arguments,
restricted named-member extraction, and explicit no-index behavior using an inert
fixture executable. Repository package, install, verifier, and runtime-smoke gates
cover the checked-in source and package paths. A separate release-session test ran
the live published musl asset on a glibc 2.31 host; no repository report records
that run. The 0.0.1 crate, GitHub asset, and standalone skill are published. No
model is bundled.

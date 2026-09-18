# Product and design review

## Competitive reality

The easy versions of this project already exist.

- Natural-language command generation is a standard capability of coding agents.
- RTK and banish already proxy common developer commands and reduce shell output
  before it reaches the model.
- MCP smart proxies already reduce tool-schema context by exposing a small
  discovery surface and activating tools on demand.
- Agent terminals and coding-agent CLIs already execute shell commands with
  permission controls.
- Completion specs, man pages, and projects such as tldr already provide compact
  command metadata and examples.

Therefore `watf` should not compete as a shell copilot, documentation searcher,
or output filter in isolation. The differentiated target is one local agent-native
CLI boundary that combines:

```text
intent
  -> capability resolution
  -> grounded typed argv
  -> deterministic validation
  -> execution
  -> bounded result reduction
```

The existential benchmark is simple: if the same successful task uses no fewer
model tokens, tool calls, fallback reads, retries, or wall-clock time than direct
agent shell use plus existing reducers, `watf` is not earning its extra layer.

### Alternatives to benchmark

| Alternative | Already solves | What `watf` must prove beyond it |
|---|---|---|
| RTK | Very fast command-output filtering for common developer CLIs | Resolve unfamiliar CLI intent and validated argv before execution, then reduce the result in the same call |
| banish | Command-output compaction plus recoverable raw output | Add grounded capability discovery and typed execution rather than only wrapping known commands |
| MCP smart proxies | Reduce prompt bloat by discovering tool schemas on demand | Resolve ordinary local CLI capabilities without turning every executable into a separate MCP schema |
| GitHub Copilot CLI and similar agents | Model-driven shell execution, permissions, and multi-step tasks | Remove model work from routine command discovery and syntax resolution |
| AetherShell | Agent-native typed shell, structured output, effect gating, result handles | Work with the existing CLI ecosystem without requiring a new shell language or reimplementing every command |
| tldr and completion specs | Compact command examples, flags, and argument metadata | Join metadata to local availability, multi-command planning, validation, execution, and bounded results |

### Classifier strategy

Do not replace lexical retrieval with a model. Use the existing index as the
first classifier and measure a simple confidence signal such as top-candidate
coverage plus score margin. Exact command mentions and sufficiently separated
matches should remain entirely local.

Jev is interesting as the next rung, not the first rung. It accepts structured
questions and returns typed probabilistic decisions, which fits choosing among a
small retrieved command set, detecting ambiguity, or deciding whether to escalate
to planning. It cannot generate the missing argv by itself, which is useful here:
it keeps classification separate from synthesis.

Benchmark this routing stack against direct retrieval and direct LLM planning:

```text
exact/local classifier -> validate/execute
ambiguous -> typed classifier -> validate/execute
needs synthesis -> local planner -> validate/execute
```

Track the percentage of intents resolved at each rung. The main classifier metric
is not raw accuracy alone; it is successful tasks per token and per millisecond,
including escalation cost.

Do not chase feature parity with all of these projects. The shortest defensible
product is the bridge across their gaps: existing CLI ecosystem in, minimal
agent context out.

## Pass 1: contracts and workload

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

## Pass 2: failure modes and scope corrections

The earlier conversation overstated what lexical retrieval and a 0.6B model can
prove. This implementation treats both retrieval quality and model planning
quality as benchmark questions. A zero hallucination promise is not made.

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

The fast path returns evidence, not guessed commands based on a high BM25 score.
BM25 is a relevance score and is never exposed as a correctness probability.

A large cloud-derived catalog must not dominate small local tools. Generic
queries suppress catalog records unless the user asks for catalog search or
mentions the catalog tool. Nested fields stay out of normal command planning.

Input limits, atomic index publication, safe quoting, no executable completion
parsing, no network client, and an explicit capability schema are mandatory.

Rust compilation and model inference must be tested in a Rust-enabled build
host before release. Source inspection or a Python reference benchmark is not
reported as a successful Rust test run.

## Final cross-module pass

The review corrected duplicate evidence being mistaken for truncation, made the
persistent protocol's approximate token count include its request-ID envelope,
and bounded the owned index read even if a file grows after its metadata check.
Regression cases cover those packet behaviors and malformed source digests/control
characters. These are source changes reviewed by inspection, not claims of Rust
test execution. The actual suite now contains 102 Rust test functions.

Installer tests exercise checksum failure, rejected repository/version arguments,
restricted named-member extraction and explicit no-index behavior using an inert
fixture executable. Release generation refuses to package an absent executable or
a missing Cargo.lock. Source delivery remains distinct from a verified binary release.

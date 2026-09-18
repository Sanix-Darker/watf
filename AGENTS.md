# watf agent contract

## Mission

`watf` is a local CLI harness for AI agents. Its job is to make command-line
work cheaper for the model by collapsing discovery, command construction,
validation, execution, and result reduction into the smallest practical agent
interaction.

The target path is:

```text
agent intent
  -> local capability resolution
  -> minimal typed argv plan
  -> deterministic validation
  -> execution boundary
  -> bounded stdout/stderr capture
  -> deterministic reduction
  -> minimal actionable result
```

Human-facing UX is secondary. Do not add features because they make `watf` a
nicer shell for people unless they also improve the agent path.

## Product invariants

1. Agent first. Optimize the machine interface before interactive UI.
2. Deterministic before inference. Retrieval, parsing, validation, filtering,
   and known command transforms should not require an LLM.
3. Context is a cost. Every byte returned to an agent must justify itself.
4. One intent may span multiple commands and tools.
5. Keep plans as structured argv for as long as possible. Do not turn them into
   shell source just to execute them.
6. Generated syntax must remain tied to retrieved evidence and provenance.
7. Unknown syntax fails closed instead of being guessed.
8. Execution output is bounded before it reaches model context.
9. Token savings are measured end to end. Do not convert byte savings into fake
   billing claims.
10. A feature that adds a tool call without reducing the larger agent loop is a
    regression until benchmarks prove otherwise.

## Intent routing ladder

Most CLI work is deterministic once the command surface and arguments are known.
Do not spend generative inference on classification that the index can settle.

Route each intent through the first stage that is sufficient:

1. Exact syntax and explicit command names.
2. Local deterministic classification from indexed terms, command scopes, local
   availability, clause coverage, and score margin.
3. Optional fast typed classifier for genuine ambiguity. Jev is a candidate here:
   give it only the bounded state and a closed choice set produced by retrieval.
4. Local generative planning only when the task requires free-form synthesis or
   a multi-command plan that the earlier stages cannot construct.
5. Deterministic validation always runs before execution.

The classifier must never invent command names or flags. It may choose, reject,
score, or ask for escalation among candidates already grounded by the index.

Remote classification is not the fast path. Network latency can dominate local
lookup, so a remote classifier must prove lower end-to-end agent cost on ambiguous
queries before becoming part of a default build.

## Performance contract

The product metric is not search latency in isolation. Measure the complete
successful task loop:

- agent input tokens
- agent output tokens
- bytes returned by `watf`
- external tool calls
- fallback documentation reads
- retries and repair loops
- `watf` cold and warm latency
- peak RSS
- task success

The project only has a reason to exist if this loop is materially cheaper than
letting a capable agent use ordinary shell tools directly.

## Execution contract

The intended executor is deliberately narrow:

```text
validated structured argv
  -> direct process spawn
  -> explicit cwd
  -> timeout
  -> independently bounded stdout/stderr
  -> exit status and truncation metadata
  -> deterministic result reduction
```

Do not execute generated text through `sh -c`. Do not hide destructive or
network effects behind a claim that a command is safe. Authorization belongs to
the calling agent or harness policy. `watf` validates command structure and owns
its execution boundary, not the caller's permission model.

## Output contract

Prefer structured, compact, loss-aware results. Preserve enough information for
the agent to decide whether the command succeeded and what to do next.

When reducing output:

- preserve exit status
- preserve stderr separately when useful
- preserve failures and actionable diagnostics first
- deduplicate repeated noise
- collapse successful repetition
- bound lines and bytes
- report truncation explicitly
- support deliberate recovery of omitted raw output when that becomes necessary

Never silently turn a failed command into an apparent success.

## Architecture exclusions

Do not turn `watf` into a generic chat assistant, human shell replacement,
workflow platform, remote model gateway, vector database, speculative plugin
framework, or background daemon by default.

The existing mmap index, typed records, lexical retrieval, provenance, typed
plan IR, validator, and foreground JSONL server are the foundation. Reuse them.

The optional local model is a fallback for planning, not the center of the
architecture. If deterministic resolution can produce the same correct argv,
prefer it.

## Competitor test

Before adding a feature, ask which existing category already solves it:

- shell-output reducers such as RTK and banish
- MCP tool-schema routers and proxies
- agent terminals and coding-agent CLIs
- command metadata and completion databases
- generic shell executors and MCP shell servers

Do not clone a competitor feature unless it closes the full `watf` loop better.
The defensible target is one small local interface that combines capability
resolution, grounded argv, validation, execution, and bounded results.

## Change discipline

- Read the real call path before editing it.
- Reuse existing code before adding an abstraction or dependency.
- Prefer deletion and simplification.
- Keep trust-boundary validation and provenance checks intact.
- Keep new runtime dependencies exceptional and benchmark-justified.
- Add the smallest runnable regression check for non-trivial new logic.
- Do not use em dash characters in source or Markdown.
- Keep historical release and verification claims historical. Never rewrite old
  evidence to imply a future executor already existed.

## Documentation discipline

Documentation must distinguish three states clearly:

- implemented now
- measured now
- target direction

Do not describe target execution or compression work as shipped until code and
tests prove it. Do not keep philosophical statements that execution can never
belong in `watf`; it is now part of the product direction.

# Architecture

## Data flow

```text
local docs + optional catalog
  -> bounded native ingestion
  -> typed records and source provenance
  -> canonical deduplication
  -> immutable binary index

agent intent
  -> quoted-clause splitting and lexical normalization
  -> cross-tool retrieval
  -> candidate command scopes
  -> deterministic route
       -> exact or grounded argv when checks pass
       -> abstention when checks do not pass
  -> scoped option and documentation retrieval
  -> round-robin clause coverage and evidence deduplication
  -> bounded provenance-backed packet
       -> deterministic agent retrieval: return evidence, no model
       -> plan when needed: local GGUF + GBNF
            -> typed draft
            -> deterministic surface checks
            -> validated structured argv

validated continuation
  -> direct process execution
  -> bounded stdout/stderr capture
  -> deterministic output reduction
  -> compact actionable agent result
```

## Routing before planning

The implemented route uses explicit command terms, lexical matches, local
availability, clause coverage, and score margin. It constructs grounded argv when
the checks pass and otherwise abstains or uses the optional local GGUF synthesis
fallback. Every accepted path uses the same validator. A typed classifier is a
future option for choosing or abstaining among bounded retrieved candidates; none
ships in 0.0.1, and it must not invent command names or flags.

## Module contracts and review points

| Module | Responsibility | Important boundary |
|---|---|---|
| `record` | Typed command, option, field and example records | Nested fields never become flags |
| `text` | Terms, small synonyms, quote-aware clause splitting | No semantic planner or LLM at retrieval time |
| `discover` | Absolute PATH inventory and filename-only project hints | No executable/version probe |
| `ingest` | Bounded plain/gzip file reading and static parsers | No shell, roff interpreter, callbacks or network |
| `index::build` | Deduplicate, rank impacts, encode and publish | Exclusive writer lock, fresh inode, fsync and rename |
| `index::read` | Map bytes, checked offsets, lazy materialization | Read-only mapping is not safe against malicious truncation |
| `index::search` | Rare-first lexical ranking with bounded top-K | Scores are relevance, not calibrated probability |
| `packet` | Hierarchical retrieval, clause coverage, hard output cap | Evidence presence is not proof of satisfied intent |
| `plan` | Typed steps, flag checks, evidence IDs and shell quoting | Surface validation is not semantic verification |
| `infer` | Optional in-process Qwen3 adapter and grammar | Real model token budget, no cloud or executable fallback |
| `execute` | Direct argv spawn, pipelines, timeout, bounded capture | Never evaluates generated shell source |
| `route` | Fail-closed deterministic command classification | Relevance margin is not a correctness probability |
| `cli` | Stable operations and foreground JSONL protocol | Search, route, run, exact exec, and deterministic resolve plus exec stay bounded |
| `tui` | Secondary interactive view of the same engine | Must not drive agent architecture decisions |

## Retrieval mechanics

Build-time field weights favor command and option names over descriptions.
BM25-like length-normalized impacts are precomputed per posting with k1=1.2,
b=0.75. A small source-priority multiplier and query coverage multiplier apply
at search time. Exact flag tokens preserve case, so `-A` and `-a` differ.

The index has one global vocabulary and a prefixed local-only vocabulary. The
latter contains only non-catalog records. Scoped searches find the command's DocIDs
once, then binary-search each term posting list instead of scanning an entire
global list. This is an implemented optimization, not a benchmark claim.

Scratch arrays are reused with epoch marks; clearing a query does not zero the
whole record-space. Only top candidates become full capability objects. An
agent process can amortize index opening and PATH discovery through `serve`.

## Sources and conflicts

Priority is captured help, man page, static completion, built-in/local document,
then bulk catalog. Canonical deduplication uses scope, kind and name. The highest
priority copy wins that exact key. Alias overlap resolves to the highest priority
matching option. Distinct canonical names can coexist. This does not prove that a
local source belongs to the current executable or that a plugin is installed.

Source hashes describe decoded input. Metadata snapshots use the original file's
size and modification timestamp. The quick freshness check is metadata-only;
changed/missing selected local sources reject a plan. Preserved size and timestamps
can evade that check. Rebuild or independently hash documentation when integrity
matters. Catalog and built-in sources remain labeled snapshots.

## Deliberate scope

A straight-line, at-most-eight-step IR was chosen instead of a general workflow
language. Pipelines are represented explicitly and rendered with Bash pipefail.
Model output cannot insert arbitrary shell grammar: arguments are literal strings,
not a shell program.

The executor consumes the validated structured argv directly, with explicit cwd,
timeout, separate bounded stdout/stderr, exit status, and truncation metadata. It
does not use `sh -c` for generated text. Authorization remains the caller's policy.

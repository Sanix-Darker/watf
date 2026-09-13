# Design reviews before implementation

## Pass 1: contracts and workload

| Module | Contract | Decision |
|---|---|---|
| discover | Inspect PATH and project marker metadata without executing binaries | No help probing, shell evaluation, or subprocesses |
| ingest | Convert bounded local text and compressed manuals into attributed records | Never source completions or invoke man/groff |
| record | Distinguish commands, options, nested input fields, and examples | A JSON input field is not a shell flag |
| index | Immutable portable on-disk index with cheap reads | Sorted dictionary, precomputed BM25 impacts, fixed-width postings, mmap |
| retrieval | Search multiple intent clauses across all commands | Clause coverage and command diversity, not one mandatory tool argument |
| packet | Return useful evidence within an actual serialized byte limit | Token counts are estimates, not guarantees |
| plan | Generate argv-based multi-step IR | No raw shell generation, no implicit execution |
| validate | Check evidence for syntax and diagnose unsupported constructs | Never claim proof of intent satisfaction or calibrated confidence |
| native inference | Load one local GGUF only when planning | Optional compiled-in llama.cpp, no server or model download at runtime |
| skill | Expose deterministic retrieval to an existing agent | Retrieval-only is the default skill workflow |

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

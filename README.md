# watf

[watf.sanixdk.xyz](https://watf.sanixdk.xyz/)

The name is short for "what tf?"

`watf` is a local CLI harness for agents. It resolves intent against indexed
command evidence, validates typed argv, executes direct processes, and returns
bounded stdout and stderr.

```sh
watf plan --json "show git status short"

watf search --json --max-bytes 4096 \
  "show docker compose defined services"
```

The query can name a task rather than a program. Search returns grounded evidence.
Planning returns typed steps only when deterministic checks or the optional model
and validator accept them.

## Agent path

```text
agent intent
  -> local capability lookup
  -> deterministic classification when possible
  -> minimal typed argv plan
  -> deterministic validation
  -> execution boundary
  -> bounded stdout/stderr
  -> deterministic reduction
  -> compact agent result
```

The primary interface serves agent tool loops. Benchmarks track task success,
bytes, tool calls, documentation reads, retries, and latency. Deterministic routes
avoid inference. The optional local model handles requests that require synthesis.

## Release status

Version 0.0.1 is a public release candidate. The default model-free crate, full
native feature build, test suites, strict Clippy, package installation, runtime
smoke, and offline verification pass on the current Linux host. The tag workflow
is configured to publish a Linux x86_64 GNU lite binary plus source and skill
archives when the matching tag exists. No release asset or model is bundled yet.

The current [model smoke report](reports/model-smoke.json) records one
deterministic plan accepted with zero inference and two model-planned cases
rejected fail-closed for dropped literals or order. Semantic model accuracy remains
unverified, so model planning is unsuitable as the default path. The original
source-assembly evidence is retained only as a [historical report](reports/VERIFICATION.md).

## The boundaries

| Component | Behavior |
|---|---|
| Rust engine | Local files, metadata, deterministic retrieval, optional native inference |
| Runtime subprocesses | Validated plan execution only; indexing never executes documentation or completions |
| Runtime networking | No HTTP client, model downloader, cloud API, socket or telemetry |
| Inference | Embedded llama.cpp through pinned Rust bindings, optional at compile time |
| Agent mode | Bounded machine-readable retrieval, no model needed |
| Plan output | Typed steps, literal argv, dependency relations, provenance and warnings |
| Execution | Direct validated argv, explicit cwd, timeout, pipelines, redirection, bounded stdout/stderr |
| Installation/build | May use curl, tar, a compiler and other provisioning tools |

A proposed `aws`, `docker`, `curl` or `git push` command can itself need a network.
Command effects belong to the invoked program. `watf` owns validation, direct
process spawning, timeout handling, and bounded result capture.

## Build and try the current agent engine

Use a current stable Rust toolchain. Maintainer scripts require Python 3.11 or newer;
Python is not a runtime dependency of `watf`.

```sh
cd watf
make fmt
make lock
make lite
make test
make verify

./target/release/watf index --index .watf/index.widx \
  --catalog data/catalog.jsonl.gz --no-system

./target/release/watf search --index .watf/index.widx --json --max-bytes 4096 \
  "show docker compose defined services"

./target/release/watf doctor --index .watf/index.widx --verify --json
```

`Cargo.lock` is committed. Release and maintainer build commands use `--locked`
where dependency resolution must remain fixed. `make fmt-check` verifies that the
current Rust source matches the committed formatter output.

To index actual local manuals as well, omit `--no-system`. Only plain and gzip
manuals are read, without launching a formatter or shell. Unsupported files are
reported as skipped.

## Build the full native version

On a Debian/Ubuntu build machine with Rust already installed:

```sh
sudo apt-get install build-essential cmake clang libclang-dev
make build
make test-full
make smoke
```

`make build` enables `local-llm,tui`; `make lite` excludes both. The TUI is a
secondary interface, not the product center. The Rust application
statically embeds the inference core, but a GNU/Linux build can still depend on
system C/C++ runtime libraries. This is not a promise of a fully static musl binary.
Release packaging records build metadata for generated assets. The tag workflow
is configured for a Linux x86_64 GNU lite binary; full local-LLM builds remain a
maintainer build until release verification covers them.

Provision the model explicitly, once:

```sh
make model
export WATF_MODEL="$HOME/.local/share/watf/models/Qwen3-0.6B-Q4_0.gguf"

./target/release/watf plan --index .watf/index.widx --json \
  "show repository status, then list defined compose services"
```

The pinned Qwen3 0.6B Q4_0 file is approximately 429 MB and is downloaded separately.
Its checksum and immutable repository revision are in `data/models.json`.
Default inference settings are 4096 context tokens, up to 768 output tokens,
CPU execution and at most four threads. The actual tokenizer checks the prompt
budget before inference. A 1-2 GB runtime envelope is a target to measure, not a
verified guarantee. Multi-tool correctness with a 0.6B model is not assumed.

For a host-specific optimized build:

```sh
make native
```

This uses `target-cpu=native` and the `dist` profile. Do not distribute that binary
to other machines. Benchmark the actual native binding and compiler combination
before attributing an improvement to these flags.

## Install

From crates.io:

```sh
cargo install watf --version 0.0.1 --locked
watf index
```

The crate embeds the core records only. Install the offline cloud catalog data
separately before indexing if catalog search is needed.

From source:

```sh
make install
watf index
```

`PREFIX` and `DATA_DIR` are configurable Makefile variables. A non-default data
location must also be set with `WATF_DATA_DIR` when running the engine.

A curl installer is configured for the tagged release assets. This command works
after `v0.0.1` is published from the `main` branch:

```sh
curl -fsSL https://raw.githubusercontent.com/Sanix-Darker/watf/main/scripts/install.sh \
  | sh -s -- --version v0.0.1
```

For stronger review, download and inspect that script before running it. The
installer verifies archive and model SHA256 values, installs without sudo,
never edits shell configuration, and optionally builds the index. The tag workflow
is configured for Linux x86_64 GNU lite archives only. Manually packaged compatible
archives, including other targets or full builds, can still be installed with
`--local-archive`. Checksums detect corruption;
checksums from the same release location are not independent signatures.

Offline installation of a previously downloaded release is also supported:

```sh
sh scripts/install.sh --local-archive /absolute/path/release.tar.gz \
  --sha256 VERIFIED_SHA256 --no-index
```

## Corpus

The bundled bulk catalog contains **124,530 unique typed records** derived from
425 real AWS service models in botocore 1.43.18:

| Record type | Count | Meaning |
|---|---:|---|
| Command | 18,467 | Service operation exposed through a CLI command scope |
| Option | 64,450 | Direct input parameter mapped to a CLI option |
| Input field | 41,613 | Nested JSON request member, not a shell flag |
| Total | 124,530 | Unique records, not 124,530 installed executables |

There are 82,917 direct command/option records. The remaining records describe
nested input fields. Aliases and enum values are metadata, not inflated counts.
This is an AWS-heavy seed catalog, not 100,000 unrelated Unix tools or proven
workflows. AWS CLI customizations can differ from raw service-model conventions.

The compressed catalog is 5,334,641 bytes. Its manifest records source model paths,
API versions, compressed/decoded hashes, source package version and per-service
counts. Apache-2.0 attribution is included. An additional **608 bootstrap records
across 117 command scopes** cover Git, Docker, filesystem tools, Rust, Go and more.
They are explicitly labeled built-in snapshots, not current local documentation.

Ordinary queries use separate local-only postings so the cloud catalog does not
pollute local ranking or force long cloud posting scans. Pass `--catalog`, or use
a literal `aws` in the query, to search the bulk catalog. Add `--fields` to include
nested JSON fields. Catalog records never make `program_available` true.

## Add local documentation without executing it

```sh
watf index --no-system --no-catalog \
  --command demo --help-file tests/fixtures/demo.help

watf index --no-system --no-catalog \
  --command demo --man-file tests/fixtures/demo.1 \
  --completion-file tests/fixtures/demo.fish --completion-shell fish

watf index --command mytool --doc-file /absolute/path/local-guide.md
```

`index` is a full rebuild. Repeat the import arguments you want to retain.
Help files must already be captured by the user or another authorized process.
Static Bash/Zsh/Fish completion subsets are parsed as data. Dynamic callbacks,
`.so` includes, shell substitutions and completion code are never evaluated.
No automatic active `--help` or version probing is hidden behind indexing.

## Agent integration

The skill is provided both at `skills/watf/SKILL.md` and as a standalone
`watf-skills/` package. Its purpose is to make `watf` the agent's first interface
for CLI capability discovery instead of repeatedly reading `--help`, man pages,
and large command output. Validated plans can be executed directly through
`watf run` or the foreground JSONL `run` operation. Explicit indexed syntax can
skip retrieval and inference entirely with `watf exec` or the JSONL `exec`
operation, which plan, validate, execute, and bound the result in one request.

```sh
watf search --json --max-bytes 2048 \
  "restart the compose backend and follow its recent logs"
```

`--max-bytes` bounds the whole evidence JSON response including its newline.
`estimated_tokens` is explicitly bytes divided by four, not a real tokenizer.
A model-specific token or dollar savings claim must be measured externally.

For repeated requests, `watf serve` keeps the index mapped and accepts JSONL on
stdin. It is a foreground process, not a daemon or network server:

```sh
watf serve < examples/request.jsonl
```

For exact indexed syntax:

```sh
watf exec --json -- git status --short
```

```json
{"id":"r1","op":"exec","argv":["git","status","--short"],"cwd":"/repo","max_bytes":8192,"max_output_bytes":4096}
```

Each request has an ID; each successful response echoes that ID. Invalid requests
produce bounded error records. A line above the input limit terminates the stream.
Restart the process after rebuilding the index or changing PATH. See
[the API contract](docs/API.md) and `examples/agent_client.py`.

## Planning and validation

The native model emits a constrained JSON plan, not raw shell source. Canonical
command scopes are restricted to retrieved evidence. A deterministic validator
then checks indexed command/flag membership, arity, required options, known enum
values, source freshness, literal limits and dependency shape. The renderer owns
quoting. `watf run` executes validated structured argv directly rather than
passing rendered shell source to a shell.

```sh
watf validate --json --plan-file examples/plan.json
watf run --json --plan-file examples/plan.json --timeout-ms 10000 --max-output-bytes 4096
watf explain --json --argv-json '["git","commit","-m","release"]'
```

`accepted: true` means a documented-surface check passed. It does **not** prove task
correctness, positional arguments, resource existence, option compatibility,
permissions, state transitions or safety. Unknown flags fail closed, but a valid
flag can still be the wrong flag for the task. There is no fake confidence score
and no promise of zero hallucinations.

Plans support up to eight steps, literal argv, success/always dependencies,
explicit pipelines and stdout redirection. No loops, shell expansion, arbitrary
shell programs, automatic repair loop, environment mutation, directory changes,
parallel DAG executor or autonomous actions are implemented.

## Secondary TUI

The agent protocol is the primary interface. With the `tui` feature, `watf tui`
opens a small terminal interface for manual inspection. Enter retrieves
evidence; Ctrl+P explicitly invokes configured local planning. Arrow keys scroll;
Esc or Ctrl+C exits. It does not load a model per keystroke, execute commands or
call a clipboard utility. Native inference currently blocks the UI while running.

## Performance and tests

The hot path uses an immutable mmap index, an interned string pool, fixed-width
posting/record tables, precomputed ranking impacts, rare-first posting traversal,
exact-scope binary searches, reusable epoch-marked scratch arrays and a bounded
top-K heap. There is no vector database, embedding model, database process or
neural query rewrite. Safety bounds remain enabled.

```sh
make bench
make bench-loop
WATF_BENCH_INDEX="$PWD/.watf/index.widx" make bench
python3 scripts/compare_bench.py before.json after.json
```

The current end-to-end harness completed all 150 direct and 150 WATF read-only Git
tasks with exact expected argv and output. Each WATF task used one protocol call,
one workload spawn, and zero model calls, fallback reads, or retries. These results
are host-specific. See the [performance methodology](docs/PERFORMANCE.md) for
latency, response bytes, fixture scope, and caveats.

`examples/complex.jsonl` and [COMPLEX.md](examples/COMPLEX.md) contain exactly **100
complex inputs** across ten categories, including ten deliberate abstention cases.
They are evaluation inputs with constraints and hazards, not 100 certified
successful model outputs.

## Repository map

```text
src/                 Rust library, CLI and optional TUI
src/index/           binary index, reader, ranking and scratch storage
src/ingest/          bounded native documentation readers
src/plan/            plan IR, validation and inert Bash renderer
src/infer/           prompt, GBNF and optional embedded inference
skills/watf/         canonical agent skill
watf-skills/         separately distributable skill package
data/                real catalog, bootstrap corpus and model manifest
examples/            100 scenarios, plan fixture and agent client
schemas/             JSON Schema contracts
tests/               Rust regression tests and documentation fixtures
benches/             actual native retrieval benchmark
scripts/             build, verification, installer and release tooling
.github/             CI, release notes, security, benchmarks and Dependabot
docs/                architecture, limitations, security and release guides
reports/             verification evidence and measured reports
```

MIT for project code. Apache-2.0 for the derived botocore catalog and its source
license. The separately downloaded model has its own license. See
[CORPUS.md](docs/CORPUS.md), [SECURITY.md](SECURITY.md), and [limitations](docs/LIMITATIONS.md).

# Source delivery verification

## Executed in the assembly environment

The offline artifact verifier parsed all 124,530 catalog records, checked unique
IDs/canonical keys, record limits/types, corpus hashes, 425 source model references,
608 bootstrap records and exactly 100 distinct complex samples. Reference command
scopes in every sample exist in the supplied corpora. Schema definitions were
checked and bootstrap records plus the example plan were schema-validated.

All Python sources parsed successfully. All shell scripts passed `sh -n`.
All GitHub YAML files were syntactically parsed with PyYAML. The installer passed
six actual shell tests using an inert fixture binary: valid installation, restricted
archive extraction, no-index behavior, bad checksum rejection, invalid repository
rejection and invalid version rejection. No generated task command was executed.
Code and Markdown were scanned for the prohibited em dash character.

All nine regenerated catalog, bootstrap, sample and schema files were byte-identical
to the original generated outputs. Machine-readable details are in
`artifact-verification.json`, `installer-verification.json`, and
`regeneration-verification.json`. These checks are not Rust compilation or model tests.

## Not executed or verified

| Check | Status | Reason |
|---|---|---|
| Rust compilation | Not run | No cargo/rustc available |
| Rust test functions | 102 written, not run | No Rust toolchain |
| Native llama.cpp build/link | Not run | Rust toolchain/dependency provisioning unavailable |
| Native Qwen3 inference | Not run | No built binary or provisioned model |
| Rustfmt/Clippy | Not run | Toolchain unavailable |
| Cargo.lock resolution | Not run | No cargo or network dependency resolution |
| Retrieval latency/throughput | Not measured | No executable |
| Peak RAM with model | Not measured | No executable/model |
| Multi-step semantic accuracy | Not measured | Requires actual inference and reviewed evaluation |
| Live curl release installation | Not tested | No hosted project release exists |

Provisioning Rust was attempted through local discovery, package mechanisms and an
official toolchain download, but the environment could not obtain it. No lockfile,
binary, success log or benchmark number was fabricated to hide that limitation.
The GitHub workflows and Make targets perform these remaining checks on a suitable
build host. They are included as executable configurations, not claimed successful
remote runs.

## Two review passes

The design review first checked each module's input/output contract and failure
boundaries. The second pass checked cross-module interactions, documentation,
source priority, argv arity, token/byte limits, stale evidence, atomic publication,
model grammar semantics, installer archive handling and the scope of performance
claims. Specific issues were corrected, including case-sensitive flags, UTF-8
clause boundaries, two-value jq options, explicit catalog-only input fields, avoiding
double sampler acceptance, full JSON byte budgeting and source-priority alias
resolution. Manual review cannot replace compilation and execution.

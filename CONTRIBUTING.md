# Contributing

Keep the engine small, local-first and agent-first. Prefer removing work from the
hot path over adding a framework. Preserve checked file-format bounds, validation,
bounded outputs and honest provenance. The executor consumes validated structured
argv directly instead of shell source. Do not claim performance or correctness
from an unrun test.

```sh
make fmt-check
make lock
make verify
make test
export LIBCLANG_PATH=/usr/lib/llvm-10/lib # adjust for the installed LLVM
make test-full
make smoke
make bench
```

Add regression fixtures for parser boundaries, stale sources, invalid flags,
quoting, literal values, command-scope isolation and byte budgets. Keep model
smoke tests opt-in so retrieval CI does not download hundreds of megabytes.
Native model smoke results are not semantic evaluation results.

For new agent-path work, benchmark the complete successful task loop rather than
isolated lookup speed. Prefer deterministic routing first, optional typed
classification for ambiguity, and generative planning only when synthesis is
actually required.

Any corpus update must provide source/license hashes, separate commands/options
from nested fields, and avoid counting aliases or enums as new capabilities.
Do not use model-generated capability padding. Keep `skills/watf/SKILL.md` and its
standalone package copy identical. Do not put em dash characters in code or Markdown.

Before a public release, commit a reviewed `Cargo.lock` and formatter output, then
require passing CI. Full native tests require libclang and a matching
`LIBCLANG_PATH`. The historical source-assembly report records the limitations of
that earlier environment; current claims must come from current build and test
evidence.

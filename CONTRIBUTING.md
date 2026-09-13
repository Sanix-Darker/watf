# Contributing

Keep the engine small, offline and non-executing. Prefer removing work from the
hot path over adding a framework. Preserve checked file-format bounds and honest
provenance. Do not claim performance or correctness from an unrun test.

```sh
make fmt
make lock
make verify
make test
make test-full
make smoke
make bench
```

Add regression fixtures for parser boundaries, stale sources, invalid flags,
quoting, literal values, command-scope isolation and byte budgets. Keep model
smoke tests opt-in so retrieval CI does not download hundreds of megabytes.
Native model smoke results are not semantic evaluation results.

Any corpus update must provide source/license hashes, separate commands/options
from nested fields, and avoid counting aliases or enums as new capabilities.
Do not use model-generated capability padding. Keep `skills/watf/SKILL.md` and its
standalone package copy identical. Do not put em dash characters in code or Markdown.

Before a public release, commit a reviewed Cargo.lock and formatter output, then
require passing CI. The source assembly report explicitly records tests that
could not be run; replace gaps with real evidence, not edited success flags.

# Third-party notices

`data/catalog.jsonl.gz` is a transformed catalog derived from botocore 1.43.18
service-model data, copyright Amazon.com, Inc. or its affiliates, under Apache-2.0.
The source license is included as `botocore-LICENSE.txt`. The transformation and
source hashes are documented in `docs/CORPUS.md` and the catalog manifest.

The Qwen3 GGUF is not included. Its model license is Apache-2.0 and its immutable
revision/hash are recorded in `data/models.json`. Review the upstream model card
before redistribution. Native inference dependencies retain their own upstream
licenses. Release maintainers should inventory the complete resolved Cargo/native
dependency tree after generating Cargo.lock; no fabricated complete SBOM is shipped.

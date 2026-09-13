# Threat model and limits

## What the engine does not do

There is no executor, child-process launcher, network client, socket listener,
telemetry channel, completion evaluator, roff interpreter or dynamic plugin loader
in the Rust engine. Indexing reads documentation and executable metadata. Native
inference is linked into the process. Downloading scripts are explicit installation
tools, not hidden runtime dependencies.

## Untrusted inputs

Treat user intent, docs, completion text, catalog records, index files and model
weights as untrusted data. They can contain misleading or malicious content.
GBNF limits syntax; it does not establish truth. Source provenance establishes
where a snippet came from, not whether it should be obeyed. Terminal-facing text
is compacted to remove control characters; literal plan arguments reject control
characters. All proposed operations still require caller review.

Input policies bound query bytes, document decompression, line length, record count,
index size, plan size, step count, argv size and generation output. Unsupported
compressed formats, dynamic completion callbacks and `.so` includes are skipped
or rejected. Symlinks are not followed by the recursive man-directory scanner;
explicit import paths are an intentional user choice.

## Native and file-system risks

Read-only mmap uses one audited unsafe boundary with checked index offsets.
Another process truncating or modifying the same inode can still cause SIGBUS or
inconsistent reads. Use only indexes owned by a trusted user and atomically
published by WATF. `--no-mmap` takes an owned snapshot for untrusted files, with
size limits. Hash verification is explicit through `doctor --verify`. A same-origin
checksum is not an authenticity signature.

The freshness fast path compares file metadata, not a full hash on every query.
Same-size edits with preserved timestamps can evade it. Private docs may leak
filenames or text through evidence packets, agent logs or shell history even
though no network client exists in WATF. Keep index directories private and
review the outputs before sharing them.

GGUF loading and inference invoke a large native C/C++ dependency. Malicious model
files and native-library vulnerabilities remain a risk. Provision known weights
using pinned hashes, keep dependencies reviewed, and do not accept arbitrary
untrusted GGUF uploads into a privileged process. No complete sandbox is supplied.

## Plan correctness

Validation checks documented surface syntax, not the full semantics of the tool.
Option interactions, positional grammar, resources, credentials, permissions,
exit-code conventions, current shell state, destructive consequences and the
user's intended outcome are not proven. `git commit` can include earlier staged
changes. A grep no-match exit can break a success chain. A literal wildcard does
not expand. Pipelines run concurrently. These cases receive warnings, not a
false safety certificate.

Never pipe a generated proposal directly into a shell. No `--execute` option is
provided. An external agent must retain its own approval and execution policies.

## Installers and CI

Online installation is optional and separate. The installer uses HTTPS, validates
repository/version inputs, checks archive hashes and extracts only named flat
members by stdout, never arbitrary archive paths. It does not edit shell/agent
configuration. Review downloaded scripts before execution.

Pull-request builds have read-only repository permissions and do not use
`pull_request_target`. Release publication uses a separate write-permission job
and creates drafts for manual review. Actions currently use version tags or the
Rust toolchain branch; pin audited action SHAs for higher-assurance deployments.
No claim of a fully hermetic supply chain or verified reproducible binary is made.

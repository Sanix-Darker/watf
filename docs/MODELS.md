# Local model contract

Only a Qwen3 ChatML/non-thinking adapter is implemented. The native loader checks
`general.architecture == qwen3`; other model architectures fail rather than reuse
an untested chat template. SmolLM and arbitrary OpenAI-compatible endpoints are
not implemented. No model is required by the deterministic retrieval path.

The provided provisioning manifest pins:

- Repository: `ggml-org/Qwen3-0.6B-GGUF`
- Revision: `b5f37287796e5be0ea3dab2e7430873fb3f73e49`
- File: `Qwen3-0.6B-Q4_0.gguf`, approximately 429 MB
- SHA256: `da2572f16c06133561ce56accaa822216f2391ef4d37fba427801cd6736417d4`
- Model license: Apache-2.0

Source: https://huggingface.co/ggml-org/Qwen3-0.6B-GGUF/blob/b5f37287796e5be0ea3dab2e7430873fb3f73e49/Qwen3-0.6B-Q4_0.gguf

Disk size is not peak RSS. The current
[`model-smoke.json`](../reports/model-smoke.json) records one deterministic request
accepted with zero inference and two model-planned complex requests rejected
fail-closed for dropped literals or order. Semantic accuracy remains unverified,
and the model is unsuitable as the default planner. New runs of
`scripts/model_smoke.py` record a UTC generation time and SHA-256 values for the
model, binary, and index. The current report contains those provenance fields.
Grammar-constrained JSON does not establish multi-step task reliability.

## Integration

`local-llm` compiles `llama-cpp-2` and `llama-cpp-sys-2` at exact version 0.1.156,
with default features disabled. This is a deliberate API pin, not a claim that it
is the latest version. Updating one without the other is not supported. The
native build needs CMake, a C/C++ compiler and libclang for bindings. These are
build dependencies, never runtime subprocesses.

The runtime uses CPU layers only, actual model tokenization, a bounded context,
chunked prefill and grammar-constrained greedy sampling. The pinned sampler's
`sample()` already accepts its token; accepting it twice would corrupt grammar
state. A single native initialization lock serializes inference calls. It does
not serialize deterministic retrieval requests in independent engine instances.

Default context is 4096, allowed 1024-8192. Default output cap is 768, allowed
64-2048. Thread count is 1-64, default at most four available CPUs. The adapter
rejects an overflowing prompt rather than dropping literal constraints. It returns
no partial shell command on model errors or incomplete JSON.

## Prompt and grammar boundaries

The grammar restricts JSON syntax and canonical command vocabulary, not intent
correctness. Retrieved flags form a validator whitelist. Documentation and user
text are encoded as data, with chat delimiter characters escaped. This reduces
trivial delimiter injection but is not a complete prompt-injection defense.
Bounded generation and independent validation remain essential safeguards. The
executor accepts only validated structured argv under bounded execution policy.
A model can still select an incorrect documented command.

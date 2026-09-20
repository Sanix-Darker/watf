use super::{grammar, prompt, Generated, Metrics, Options};
use crate::{
    plan::{Context, Draft},
    Error, Result,
};
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
    send_logs_to_tracing, LogOptions,
};
use std::{num::NonZeroU32, path::PathBuf, sync::Mutex, time::Instant};

// llama backend initialization and global log settings are process-wide. Keep
// embedding calls serialized; this CLI is not a multi-tenant inference server.
struct Native {
    backend: LlamaBackend,
    model: Option<(PathBuf, LlamaModel)>,
}
static NATIVE: Mutex<Option<Native>> = Mutex::new(None);
fn native_error(e: impl std::fmt::Display) -> Error {
    Error::message(format!("native inference: {e}"))
}

pub(super) fn generate(query: &str, context: &Context, options: &Options) -> Result<Generated> {
    let mut guard = NATIVE
        .lock()
        .map_err(|_| Error::message("inference state poisoned"))?;
    let start = Instant::now();
    if guard.is_none() {
        send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));
        *guard = Some(Native {
            backend: LlamaBackend::init().map_err(native_error)?,
            model: None,
        });
    }
    let native = guard.as_mut().expect("native initialized above");
    let reload = native
        .model
        .as_ref()
        .is_none_or(|(path, _)| path != &options.model);
    if reload {
        let params = LlamaModelParams::default().with_n_gpu_layers(0);
        let model = LlamaModel::load_from_file(&native.backend, &options.model, &params)
            .map_err(native_error)?;
        let architecture = model
            .meta_val_str("general.architecture")
            .map_err(native_error)?;
        if architecture != "qwen3" {
            return Err(Error::message("this prompt adapter supports Qwen3 GGUF only; other architectures need a tested chat adapter"));
        }
        native.model = Some((options.model.clone(), model));
    }
    let load_ms = start.elapsed().as_millis();
    let model = &native.model.as_ref().expect("model loaded above").1;
    let prompt = prompt::qwen3(query, context)?;
    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(native_error)?;
    if tokens.is_empty()
        || tokens.len() + options.output_tokens as usize > options.context_tokens as usize
    {
        return Err(Error::message(format!("{} prompt tokens plus {} output tokens exceed context {}; narrow retrieval or raise --context up to 8192", tokens.len(), options.output_tokens, options.context_tokens)));
    }
    let parameters = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(options.context_tokens))
        .with_n_threads(options.threads)
        .with_n_threads_batch(options.threads);
    let mut ctx = model
        .new_context(&native.backend, parameters)
        .map_err(native_error)?;
    let grammar = grammar::for_context(context)?;
    let constrained = LlamaSampler::grammar(model, &grammar, "root").map_err(native_error)?;
    let mut sampler = LlamaSampler::chain_simple([constrained, LlamaSampler::greedy()]);
    let prefill = Instant::now();
    let mut batch = LlamaBatch::new(512, 1);
    for (chunk_index, chunk) in tokens.chunks(512).enumerate() {
        batch.clear();
        for (offset, &token) in chunk.iter().enumerate() {
            let position = chunk_index * 512 + offset;
            batch
                .add(token, position as i32, &[0], position + 1 == tokens.len())
                .map_err(native_error)?;
        }
        ctx.decode(&mut batch).map_err(native_error)?;
    }
    let prefill_ms = prefill.elapsed().as_millis();
    let generating = Instant::now();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut output = String::new();
    for (generated, n) in (0..options.output_tokens).enumerate() {
        // sample() already accepts the token in this pinned binding version.
        // Accepting twice corrupts the grammar state.
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        if model.is_eog_token(token) {
            break;
        }
        output.push_str(
            &model
                .token_to_piece(token, &mut decoder, false, None)
                .map_err(native_error)?,
        );
        if output.len() > 128 * 1024 {
            return Err(Error::message("model output exceeds plan byte limit"));
        }
        if let Ok(draft) = Draft::parse(output.as_bytes()) {
            return Ok(Generated {
                draft,
                metrics: Metrics {
                    prompt_tokens: tokens.len(),
                    generated_tokens: generated + 1,
                    load_ms,
                    prefill_ms,
                    generation_ms: generating.elapsed().as_millis(),
                },
            });
        }
        batch.clear();
        batch
            .add(token, (tokens.len() + n as usize) as i32, &[0], true)
            .map_err(native_error)?;
        ctx.decode(&mut batch).map_err(native_error)?;
    }
    Err(Error::message(
        "model stopped before producing a complete plan; no partial command was returned",
    ))
}

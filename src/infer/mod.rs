//! Optional native CPU inference. Retrieval never depends on this feature.
pub mod grammar;
#[cfg(feature = "local-llm")]
mod native;
pub mod prompt;

use crate::{
    plan::{Context, Draft},
    Error, Result,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Options {
    pub model: PathBuf,
    pub context_tokens: u32,
    pub output_tokens: u32,
    pub threads: i32,
}
impl Options {
    pub fn new(model: PathBuf) -> Self {
        Self {
            model,
            context_tokens: 4096,
            output_tokens: 768,
            threads: std::thread::available_parallelism().map_or(2, |n| n.get().min(4) as i32),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !(1024..=8192).contains(&self.context_tokens)
            || !(64..=2048).contains(&self.output_tokens)
            || self.output_tokens >= self.context_tokens
            || !(1..=64).contains(&self.threads)
        {
            return Err(Error::message(
                "invalid inference limits: context 1024..8192, output 64..2048, threads 1..64",
            ));
        }
        if !self.model.is_file() {
            return Err(Error::message(
                "local GGUF file not found; watf never downloads models at runtime",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub load_ms: u128,
    pub prefill_ms: u128,
    pub generation_ms: u128,
}
#[derive(Debug, Clone, Serialize)]
pub struct Generated {
    pub draft: Draft,
    pub metrics: Metrics,
}

pub fn generate(query: &str, context: &Context, options: &Options) -> Result<Generated> {
    if query.is_empty() || query.len() > crate::text::MAX_QUERY_BYTES || query.contains('\0') {
        return Err(Error::message("invalid planning query"));
    }
    if context.commands.is_empty() {
        return Err(Error::message(
            "no command evidence; narrow the request or import documentation",
        ));
    }
    if !context.uncovered_clauses.is_empty() {
        return Err(Error::message(
            "some request clauses have no retrieved evidence; refusing to silently omit them",
        ));
    }
    #[cfg(feature = "local-llm")]
    {
        options.validate()?;
        native::generate(query, context, options)
    }
    #[cfg(not(feature = "local-llm"))]
    {
        let _ = options;
        Err(Error::message("this is the retrieval-only build; use watf search, or rebuild with --features local-llm"))
    }
}

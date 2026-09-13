//! Offline, evidence-first command discovery. The library never launches processes.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod cli;
pub mod discover;
pub mod error;
pub mod index;
pub mod infer;
pub mod ingest;
pub mod packet;
pub mod plan;
pub mod record;
pub mod text;
#[cfg(feature = "tui")]
pub mod tui;

pub use error::{Error, Result};
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SCHEMA_VERSION: u32 = 1;

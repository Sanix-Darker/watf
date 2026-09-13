//! Portable immutable index. No SQL engine, background daemon, or full-document scans.
mod build;
mod read;
mod search;

pub use build::{build, BuildStats};
pub use read::Index;
pub use search::{Hit, Scratch, SearchOptions, SearchStats};

pub(crate) const MAGIC: &[u8; 8] = b"WATFIDX1";
pub(crate) const HEADER: usize = 128;
pub(crate) const DICT_ENTRY: usize = 16;
pub(crate) const RECORD: usize = 48;
pub(crate) const POSTING: usize = 8;
pub(crate) const MAX_FILE: u64 = 2 * 1024 * 1024 * 1024;
pub(crate) const MAX_RECORDS: usize = 1_000_000;

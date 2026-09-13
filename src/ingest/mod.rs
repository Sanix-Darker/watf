//! Read documentation as data. Nothing in this module executes a command.
pub mod completion;
pub mod docs;
pub mod help;
pub mod man;

use crate::{
    record::{Capability, Source, SourceKind},
    Error, Result,
};
use flate2::read::MultiGzDecoder;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub const MAX_DOC_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
pub const MAX_CORPUS_BYTES: u64 = 512 * 1024 * 1024;

pub fn source(path: &Path, kind: SourceKind, decoded: &[u8]) -> Result<Source> {
    let metadata = fs::metadata(path)?;
    Ok(Source {
        kind,
        reference: fs::canonicalize(path)?.to_string_lossy().into_owned(),
        version: None,
        sha256: Some(format!("{:x}", Sha256::digest(decoded))),
        bytes: Some(metadata.len()),
        modified_unix: metadata
            .modified()
            .ok()
            .and_then(|v| v.duration_since(UNIX_EPOCH).ok())
            .map(|v| v.as_secs()),
    })
}

pub fn reader(path: &Path) -> Result<Box<dyn Read>> {
    let file = File::open(path)?;
    match path.extension().and_then(|s| s.to_str()) {
        Some("gz") => Ok(Box::new(MultiGzDecoder::new(file))),
        Some("xz" | "bz2" | "zst") => Err(Error::message(
            "this build reads plain text and gzip only; compressed format skipped",
        )),
        _ => Ok(Box::new(file)),
    }
}

pub fn read_document(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader(path)?
        .take(MAX_DOC_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DOC_BYTES {
        return Err(Error::message("document exceeds 2 MiB decompressed limit"));
    }
    Ok(bytes)
}

/// Read a line without letting a maliciously long line allocate unbounded memory.
pub fn bounded_line<R: BufRead>(reader: &mut R, max: usize) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if line.is_empty() { None } else { Some(line) });
        }
        let count = available
            .iter()
            .position(|&b| b == b'\n')
            .map_or(available.len(), |p| p + 1);
        if line.len().saturating_add(count) > max {
            return Err(Error::message("input line exceeds size limit"));
        }
        let complete = available[count - 1] == b'\n';
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if complete {
            return Ok(Some(line));
        }
    }
}

pub fn import_jsonl(path: &Path) -> Result<Vec<Capability>> {
    let mut reader = BufReader::new(reader(path)?.take(MAX_CORPUS_BYTES + 1));
    let mut total = 0_u64;
    let mut records = Vec::new();
    while let Some(line) = bounded_line(&mut reader, MAX_LINE_BYTES)? {
        total += line.len() as u64;
        if total > MAX_CORPUS_BYTES {
            return Err(Error::message("corpus exceeds decompressed size limit"));
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let record: Capability = serde_json::from_slice(&line)?;
        record.validate()?;
        records.push(record);
        if records.len() > 1_000_000 {
            return Err(Error::message("too many corpus records"));
        }
    }
    Ok(records)
}

pub fn builtin() -> Result<Vec<Capability>> {
    include_str!("../../data/core.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let cap: Capability = serde_json::from_str(line)?;
            cap.validate()?;
            Ok(cap)
        })
        .collect()
}

#[derive(Default, Debug, serde::Serialize)]
pub struct ScanReport {
    pub files_read: usize,
    pub records: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

pub fn scan_man_dirs(directories: &[PathBuf]) -> Result<(Vec<Capability>, ScanReport)> {
    let mut paths = Vec::new();
    fn walk(path: &Path, depth: usize, paths: &mut Vec<PathBuf>) {
        if depth > 3 || paths.len() >= 20_000 {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            if paths.len() >= 20_000 {
                break;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                walk(&path, depth + 1, paths);
            } else if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                let name = name.strip_suffix(".gz").unwrap_or(name);
                if name.ends_with(".1") || name.ends_with(".8") {
                    paths.push(path);
                }
            }
        }
    }
    for directory in directories {
        walk(directory, 0, &mut paths);
    }
    paths.sort();
    paths.dedup();
    let mut caps = Vec::new();
    let mut report = ScanReport::default();
    for path in paths {
        match man::read(&path, None) {
            Ok(mut found) => {
                report.files_read += 1;
                caps.append(&mut found);
            }
            Err(e) => {
                report.skipped += 1;
                if report.warnings.len() < 16 {
                    report.warnings.push(format!("{}: {e}", path.display()));
                }
            }
        }
        if caps.len() > 1_000_000 {
            return Err(Error::message("local manual corpus exceeds record limit"));
        }
    }
    report.records = caps.len();
    Ok((caps, report))
}

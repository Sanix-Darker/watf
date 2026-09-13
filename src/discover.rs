//! Filesystem metadata only. No version probes, shell aliases, or child processes.
use crate::{
    record::{Source, SourceKind},
    Result,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Debug, Clone, Serialize)]
pub struct Executable {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified_unix: Option<u64>,
}
pub fn executables() -> BTreeMap<String, Executable> {
    let mut entries = BTreeMap::new();
    let Some(path) = env::var_os("PATH") else {
        return entries;
    };
    let mut inspected = 0;
    for directory in env::split_paths(&path) {
        // Relative PATH entries can resolve attacker-controlled project binaries.
        if !directory.is_absolute() {
            continue;
        }
        let Ok(iter) = fs::read_dir(directory) else {
            continue;
        };
        for item in iter.flatten() {
            inspected += 1;
            if inspected > 100_000 {
                return entries;
            }
            let path = item.path();
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            entries.entry(name).or_insert(Executable {
                path,
                bytes: metadata.len(),
                modified_unix: metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
            });
        }
    }
    entries
}

pub fn project_hints(directory: &Path) -> Vec<String> {
    let mut roots = Vec::new();
    for (marker, root) in [
        (".git", "git"),
        ("Cargo.toml", "cargo"),
        ("go.mod", "go"),
        ("compose.yaml", "docker"),
        ("compose.yml", "docker"),
        ("docker-compose.yml", "docker"),
        ("Makefile", "make"),
        ("package.json", "npm"),
        ("pyproject.toml", "python"),
    ] {
        if directory.join(marker).exists() && !roots.iter().any(|r| r == root) {
            roots.push(root.to_owned());
        }
    }
    roots
}

pub fn data_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("WATF_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path).join("watf"));
    }
    let home = env::var_os("HOME")
        .ok_or_else(|| crate::Error::message("set HOME, XDG_DATA_HOME, or WATF_DATA_DIR"))?;
    Ok(PathBuf::from(home).join(".local/share/watf"))
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Snapshot,
    MetadataUnchanged,
    Changed,
    Missing,
    Unknown,
}
pub fn freshness(source: &Source) -> Freshness {
    if matches!(source.kind, SourceKind::Builtin | SourceKind::Catalog) {
        return Freshness::Snapshot;
    }
    let Ok(metadata) = fs::metadata(&source.reference) else {
        return Freshness::Missing;
    };
    let now = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    match (source.bytes, source.modified_unix) {
        (Some(size), Some(time)) if size != metadata.len() || Some(time) != now => {
            Freshness::Changed
        }
        (Some(_), Some(_)) => Freshness::MetadataUnchanged,
        _ => Freshness::Unknown,
    }
}

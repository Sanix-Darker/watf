#![allow(dead_code)]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use watf::{
    index::{self, Index},
    record::{Arity, Capability, Kind, Source, SourceKind},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub struct Temp(pub PathBuf);
impl Temp {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "watf-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn source() -> Source {
    Source {
        kind: SourceKind::Builtin,
        reference: "fixture:reviewed".into(),
        version: None,
        sha256: None,
        modified_unix: None,
        bytes: None,
    }
}
pub fn cap(command: &str, name: &str, summary: &str, kind: Kind) -> Capability {
    Capability {
        id: format!("{command}#{name}"),
        command: command.split(' ').map(str::to_owned).collect(),
        kind,
        name: name.into(),
        summary: summary.into(),
        aliases: vec![],
        arity: Arity::None,
        required: false,
        value_type: None,
        choices: vec![],
        source: source(),
    }
}
pub fn builtins() -> (Temp, Index) {
    let temp = Temp::new();
    let path = temp.path("index.widx");
    index::build(&path, watf::ingest::builtin().unwrap()).unwrap();
    let index = Index::open(&path, true).unwrap();
    (temp, index)
}

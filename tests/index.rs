mod common;
use common::*;
use std::fs;
use watf::{
    index::{self, Index, Scratch, SearchOptions},
    record::{Kind, SourceKind},
};

#[test]
fn roundtrip_preserves_every_field() {
    let temp = Temp::new();
    let path = temp.path("i");
    let mut record = cap("demo child", "--file", "Read a file", Kind::Option);
    record.aliases = vec!["-f".into()];
    record.arity = watf::record::Arity::One;
    record.required = true;
    record.choices = vec!["one".into(), "two".into()];
    record.value_type = Some("string".into());
    index::build(&path, vec![record.clone()]).unwrap();
    let opened = Index::open(&path, true).unwrap();
    opened.verify().unwrap();
    assert_eq!(
        serde_json::to_value(opened.capability(0).unwrap()).unwrap(),
        serde_json::to_value(record).unwrap()
    );
}
#[test]
fn output_is_reproducible() {
    let temp = Temp::new();
    let records = watf::ingest::builtin().unwrap();
    index::build(&temp.path("a"), records.clone()).unwrap();
    index::build(&temp.path("b"), records).unwrap();
    assert_eq!(
        fs::read(temp.path("a")).unwrap(),
        fs::read(temp.path("b")).unwrap()
    );
}
#[test]
fn source_priority_deduplicates() {
    let temp = Temp::new();
    let a = cap("demo", "--file", "Bundled", Kind::Option);
    let mut b = a.clone();
    b.source.kind = SourceKind::Help;
    b.summary = "Actual help".into();
    let stats = index::build(&temp.path("i"), vec![a, b]).unwrap();
    assert_eq!(stats.duplicate_records_removed, 1);
    assert_eq!(
        Index::open(&temp.path("i"), false)
            .unwrap()
            .capability(0)
            .unwrap()
            .summary,
        "Actual help"
    );
}
#[test]
fn old_mapping_survives_atomic_replacement() {
    let temp = Temp::new();
    let path = temp.path("i");
    index::build(&path, vec![cap("demo", "demo", "first", Kind::Command)]).unwrap();
    let old = Index::open(&path, true).unwrap();
    index::build(&path, vec![cap("demo", "demo", "second", Kind::Command)]).unwrap();
    assert_eq!(old.capability(0).unwrap().summary, "first");
    assert_eq!(
        Index::open(&path, true)
            .unwrap()
            .capability(0)
            .unwrap()
            .summary,
        "second"
    );
}
#[test]
fn empty_index_is_rejected() {
    let t = Temp::new();
    assert!(index::build(&t.path("i"), vec![]).is_err());
}
#[test]
fn small_header_is_rejected() {
    assert!(Index::from_bytes(vec![0; 127]).is_err());
}
#[test]
fn corrupt_magic_is_rejected() {
    let (t, _) = builtins();
    let mut b = fs::read(t.path("index.widx")).unwrap();
    b[0] = 0;
    assert!(Index::from_bytes(b).is_err());
}
#[test]
fn corrupt_version_is_rejected() {
    let (t, _) = builtins();
    let mut b = fs::read(t.path("index.widx")).unwrap();
    b[8] = 255;
    assert!(Index::from_bytes(b).is_err());
}
#[test]
fn oversized_count_is_rejected() {
    let (t, _) = builtins();
    let mut b = fs::read(t.path("index.widx")).unwrap();
    b[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Index::from_bytes(b).is_err());
}
#[test]
fn truncated_payload_is_rejected() {
    let (t, _) = builtins();
    let mut b = fs::read(t.path("index.widx")).unwrap();
    b.pop();
    assert!(Index::from_bytes(b).is_err());
}
#[test]
fn wrong_digest_is_rejected() {
    let (t, _) = builtins();
    let mut b = fs::read(t.path("index.widx")).unwrap();
    b[64] ^= 1;
    assert!(Index::from_bytes(b).unwrap().verify().is_err());
}
#[test]
fn invalid_doc_id_is_rejected() {
    let (_, i) = builtins();
    assert!(i.capability(u32::MAX).is_err());
}
#[test]
fn all_scopes_are_addressable() {
    let (_, i) = builtins();
    for c in watf::ingest::builtin().unwrap() {
        assert!(!i.ids_for_command(&c.command_key()).unwrap().is_empty());
    }
}
#[test]
fn scoped_lookup_does_not_leak_flags() {
    let (_, i) = builtins();
    let o = SearchOptions {
        command: Some("git add".into()),
        ..Default::default()
    };
    let (hits, _) = i
        .search("commit message --message", &o, &mut Scratch::default())
        .unwrap();
    assert!(hits
        .iter()
        .all(|h| i.command_key(h.doc).unwrap() == "git add"));
}
#[test]
fn local_queries_skip_catalog_postings() {
    let t = Temp::new();
    let mut records = vec![cap("local", "local", "Search unique needle", Kind::Command)];
    for n in 0..1000 {
        let mut c = cap(
            &format!("aws service op{n}"),
            "needle",
            "Search unique needle",
            Kind::Command,
        );
        c.source.kind = SourceKind::Catalog;
        records.push(c);
    }
    index::build(&t.path("i"), records).unwrap();
    let i = Index::open(&t.path("i"), true).unwrap();
    let (hits, stats) = i
        .search("needle", &SearchOptions::default(), &mut Scratch::default())
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(stats.postings_scanned < 10);
}
#[test]
fn explicit_catalog_enables_catalog() {
    let t = Temp::new();
    let mut c = cap(
        "aws s3api list-buckets",
        "list-buckets",
        "List object storage buckets",
        Kind::Command,
    );
    c.source.kind = SourceKind::Catalog;
    index::build(&t.path("i"), vec![c]).unwrap();
    let i = Index::open(&t.path("i"), true).unwrap();
    assert!(i
        .search(
            "buckets",
            &SearchOptions::default(),
            &mut Scratch::default()
        )
        .unwrap()
        .0
        .is_empty());
    assert_eq!(
        i.search(
            "buckets",
            &SearchOptions {
                include_catalog: true,
                ..Default::default()
            },
            &mut Scratch::default()
        )
        .unwrap()
        .0
        .len(),
        1
    );
}
#[test]
fn zero_limit_is_rejected() {
    let (_, i) = builtins();
    assert!(i
        .search(
            "git",
            &SearchOptions {
                limit: 0,
                ..Default::default()
            },
            &mut Scratch::default()
        )
        .is_err());
}
#[test]
fn long_query_is_rejected() {
    let (_, i) = builtins();
    assert!(i
        .search(
            &"a".repeat(17000),
            &SearchOptions::default(),
            &mut Scratch::default()
        )
        .is_err());
}
#[test]
fn empty_query_yields_no_hits() {
    let (_, i) = builtins();
    assert!(i
        .search("", &SearchOptions::default(), &mut Scratch::default())
        .unwrap()
        .0
        .is_empty());
}
#[test]
fn scratch_is_reusable() {
    let (_, i) = builtins();
    let mut scratch = Scratch::default();
    let o = SearchOptions::default();
    let first = i.search("exclude recursive", &o, &mut scratch).unwrap().0;
    i.search("commit message", &o, &mut scratch).unwrap();
    let again = i.search("exclude recursive", &o, &mut scratch).unwrap().0;
    assert_eq!(
        first.iter().map(|h| h.doc).collect::<Vec<_>>(),
        again.iter().map(|h| h.doc).collect::<Vec<_>>()
    );
}
#[test]
fn lock_file_prevents_publication() {
    let t = Temp::new();
    let p = t.path("i");
    fs::write(p.with_extension("widx.lock"), "test").unwrap();
    assert!(index::build(&p, vec![cap("demo", "demo", "example", Kind::Command)]).is_err());
    assert!(!p.exists());
}

#[test]
fn malformed_source_digest_is_rejected() {
    let mut record = cap("demo", "demo", "fixture", Kind::Command);
    record.source.sha256 = Some("not-a-digest".into());
    assert!(record.validate().is_err());
}
#[test]
fn control_character_in_provenance_is_rejected() {
    let mut record = cap("demo", "demo", "fixture", Kind::Command);
    record.source.reference = "fixture:\u{1b}[31m".into();
    assert!(record.validate().is_err());
}

mod common;
use common::*;
use std::{
    fs,
    io::{Cursor, Write},
};
use watf::{
    ingest::{self, completion, help, man},
    record::{Arity, Kind},
};

#[test]
fn help_aliases_and_arity() {
    let caps = help::parse(
        "Demo tool\nOptions:\n  -f, --file FILE  Read file\n  -q, --quiet  Silence output\n",
        &["demo".into()],
        source(),
    )
    .unwrap();
    let flag = caps.iter().find(|c| c.name == "--file").unwrap();
    assert_eq!(flag.arity, Arity::One);
    assert!(flag.aliases.contains(&"-f".into()));
    assert_eq!(
        caps.iter().find(|c| c.name == "--quiet").unwrap().arity,
        Arity::None
    );
}
#[test]
fn help_child_commands() {
    let caps=help::parse("Demo\nCommands:\n  build  Compile project\n  test  Run tests\nOptions:\n  --verbose  Extra output\n", &["demo".into()],source()).unwrap();
    assert!(caps
        .iter()
        .any(|c| c.command_key() == "demo build" && c.kind == Kind::Command));
}
#[test]
fn help_needs_scope() {
    assert!(help::parse("Example", &[], source()).is_err());
}
#[test]
fn help_optional_equals() {
    let c = help::parse(
        "Example\n  --color[=WHEN]  Use colors\n",
        &["demo".into()],
        source(),
    )
    .unwrap();
    assert_eq!(
        c.iter().find(|c| c.name == "--color").unwrap().arity,
        Arity::Optional
    );
}
#[test]
fn no_roff_includes() {
    assert!(man::plain(".so /etc/passwd\n").is_err());
}
#[test]
fn no_roff_macro_execution() {
    let p = man::plain(".de evil\n.sy touch /tmp/watf-should-never-exist\n..\n.SH NAME\ndemo\n")
        .unwrap();
    assert!(!p.contains("touch"));
    assert!(p.contains("demo"));
}
#[test]
fn roff_font_escapes() {
    assert!(man::plain(".B \\fB\\-a\\fR\n").unwrap().contains("-a"));
}
#[test]
fn gzip_is_native_and_bounded() {
    let t = Temp::new();
    let p = t.path("demo.1.gz");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder
        .write_all(b".SH NAME\ndemo\n.SH OPTIONS\n.B --all\nShow all entries\n")
        .unwrap();
    fs::write(&p, encoder.finish().unwrap()).unwrap();
    assert!(!man::read(&p, None).unwrap().is_empty());
}
#[test]
fn gzip_bomb_is_rejected() {
    let t = Temp::new();
    let p = t.path("big.gz");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder
        .write_all(&vec![b'x'; ingest::MAX_DOC_BYTES + 1])
        .unwrap();
    fs::write(&p, encoder.finish().unwrap()).unwrap();
    assert!(ingest::read_document(&p).is_err());
}
#[test]
fn long_line_is_rejected() {
    assert!(ingest::bounded_line(&mut Cursor::new(vec![b'x'; 33]), 32).is_err());
}
#[test]
fn bounded_line_handles_eof() {
    assert_eq!(
        ingest::bounded_line(&mut Cursor::new(b"abc"), 3)
            .unwrap()
            .unwrap(),
        b"abc"
    );
}
#[test]
fn words_preserve_quoted_empty() {
    assert_eq!(
        completion::words("a '' 'salt and pepper'").unwrap(),
        ["a", "", "salt and pepper"]
    );
}
#[test]
fn words_reject_unclosed_quotes() {
    assert!(completion::words("'unfinished").is_err());
}
#[test]
fn fish_dynamic_is_skipped() {
    let t = Temp::new();
    let p = t.path("demo.fish");
    fs::write(
        &p,
        "complete -c demo -l unsafe -d $(touch nope)\ncomplete -c demo -n 'condition' -l nested\n",
    )
    .unwrap();
    assert!(completion::read(&p, &["demo".into()], "fish")
        .unwrap()
        .is_empty());
}
#[test]
fn fish_static_is_imported() {
    let t = Temp::new();
    let p = t.path("demo.fish");
    fs::write(&p, "complete -c demo -l file -s f -r -d 'Read file'\n").unwrap();
    let caps = completion::read(&p, &["demo".into()], "fish").unwrap();
    assert_eq!(caps.len(), 2);
    assert!(caps.iter().all(|c| c.arity == Arity::One));
}
#[test]
fn bash_words_are_not_code() {
    let t = Temp::new();
    let p = t.path("demo.bash");
    fs::write(&p, "complete -W '--all --file' demo\n").unwrap();
    let caps = completion::read(&p, &["demo".into()], "bash").unwrap();
    assert_eq!(caps.len(), 2);
    assert!(caps.iter().all(|c| c.arity == Arity::Unknown));
}
#[test]
fn zsh_static_arity() {
    let t = Temp::new();
    let p = t.path("_demo");
    fs::write(&p, "'--file[Input file]:file:'\n'--all[Include all]'\n").unwrap();
    let caps = completion::read(&p, &["demo".into()], "zsh").unwrap();
    assert_eq!(caps.len(), 2);
}
#[test]
fn metadata_changes_are_visible() {
    let t = Temp::new();
    let p = t.path("help");
    fs::write(&p, "Demo\n--all  All\n").unwrap();
    let caps = help::read(&p, &["demo".into()]).unwrap();
    fs::write(&p, "Different and longer contents\n").unwrap();
    assert!(matches!(
        watf::discover::freshness(&caps[0].source),
        watf::discover::Freshness::Changed
    ));
}
#[test]
fn malformed_jsonl_is_rejected() {
    let t = Temp::new();
    let p = t.path("bad.jsonl");
    fs::write(&p, "{}\n").unwrap();
    assert!(ingest::import_jsonl(&p).is_err());
}
#[test]
fn unsupported_compression_is_explicit() {
    let t = Temp::new();
    let p = t.path("doc.xz");
    fs::write(&p, b"fake").unwrap();
    assert!(ingest::reader(&p).is_err());
}
#[test]
fn markdown_examples_are_not_flags() {
    let t = Temp::new();
    let p = t.path("demo.md");
    fs::write(
        &p,
        "# demo\n> Copy files\n\n- Copy recursively:\n`demo --recursive src dst`\n",
    )
    .unwrap();
    let c = ingest::docs::read(&p, &["demo".into()]).unwrap();
    assert!(c.iter().any(|c| c.kind == Kind::Example));
    assert!(!c.iter().any(|c| c.kind == Kind::Option));
}

//! Passive local text, Markdown, and cached TLDR ingestion. No URL fetching.
use crate::{
    record::{Arity, Capability, Kind, SourceKind},
    text, Error, Result,
};
use std::path::Path;
pub fn read(path: &Path, command: &[String]) -> Result<Vec<Capability>> {
    if command.is_empty() {
        return Err(Error::message("local document import requires --command"));
    }
    let bytes = super::read_document(path)?;
    let source = super::source(path, SourceKind::LocalDoc, &bytes)?;
    let input = String::from_utf8_lossy(&bytes);
    let prefix = format!("local:{}:{}", source.reference, command.join("/"));
    let summary = input
        .lines()
        .map(str::trim)
        .find(|s| !s.is_empty() && !s.starts_with('#') && !s.starts_with('`'))
        .unwrap_or("Local package documentation");
    let mut out = vec![Capability {
        id: format!("{prefix}#command"),
        command: command.to_vec(),
        kind: Kind::Command,
        name: command.last().cloned().unwrap_or_default(),
        summary: text::compact(summary.trim_start_matches('>'), 500),
        aliases: vec![],
        arity: Arity::Unknown,
        required: false,
        value_type: None,
        choices: vec![],
        source: source.clone(),
    }];
    let mut block = String::new();
    let mut heading = "documentation".to_owned();
    let push = |block: &mut String, heading: &str, out: &mut Vec<Capability>| {
        if block.trim().is_empty() {
            return;
        }
        let n = out.len();
        out.push(Capability {
            id: format!("{prefix}#section-{n}"),
            command: command.to_vec(),
            kind: Kind::Example,
            name: format!("{heading}:{n}"),
            summary: text::compact(block, 2000),
            aliases: vec![],
            arity: Arity::Unknown,
            required: false,
            value_type: None,
            choices: vec![],
            source: source.clone(),
        });
        block.clear();
    };
    for line in input.lines() {
        if line.starts_with('#') {
            push(&mut block, &heading, &mut out);
            heading = text::compact(line.trim_start_matches('#'), 120);
        } else {
            block.push_str(line);
            block.push('\n');
            if block.len() > 1600 {
                push(&mut block, &heading, &mut out);
            }
        }
        if out.len() >= 2048 {
            return Err(Error::message("local document has too many sections"));
        }
    }
    push(&mut block, &heading, &mut out);
    for record in &out {
        record.validate()?;
    }
    Ok(out)
}

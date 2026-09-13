use crate::{
    record::{Arity, Capability, Kind, Source, SourceKind},
    text, Error, Result,
};
use std::path::Path;

pub fn read(path: &Path, command: &[String]) -> Result<Vec<Capability>> {
    let bytes = super::read_document(path)?;
    let source = super::source(path, SourceKind::Help, &bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    parse(&text, command, source)
}

pub fn parse(input: &str, command: &[String], source: Source) -> Result<Vec<Capability>> {
    if command.is_empty() {
        return Err(Error::message(
            "help import requires an explicit command scope",
        ));
    }
    let summary = input
        .lines()
        .map(str::trim)
        .find(|l| {
            !l.is_empty() && !l.starts_with('-') && !l.starts_with("Usage:") && !l.ends_with(':')
        })
        .unwrap_or("Locally documented command");
    let prefix = format!("local:{}:{}", source.reference, command.join("/"));
    let mut records = vec![Capability {
        id: format!("{prefix}#command"),
        command: command.to_vec(),
        kind: Kind::Command,
        name: command.last().cloned().unwrap_or_default(),
        summary: text::compact(summary, 500),
        aliases: vec![],
        arity: Arity::Unknown,
        required: false,
        value_type: None,
        choices: vec![],
        source: source.clone(),
    }];
    let mut last_option: Option<usize> = None;
    let mut commands_section = false;
    for raw in input.lines() {
        let line = raw.trim();
        if line.is_empty() {
            last_option = None;
            continue;
        }
        let heading = line.trim_end_matches(':').to_ascii_lowercase();
        if matches!(
            heading.as_str(),
            "commands" | "available commands" | "subcommands"
        ) {
            commands_section = true;
            last_option = None;
            continue;
        }
        if line.ends_with(':') && !line.starts_with('-') {
            commands_section = false;
            last_option = None;
        }
        if line.starts_with('-') {
            let (spec, description) = line
                .find("  ")
                .map(|at| (&line[..at], line[at..].trim()))
                .unwrap_or((line, ""));
            let mut flags = Vec::new();
            let mut arity = Arity::None;
            let mut value_type = None;
            for word in spec.split(|c: char| c.is_whitespace() || c == ',') {
                if word.is_empty() {
                    continue;
                }
                if word.starts_with('-') && word.len() > 1 {
                    let cut = word.find(['=', '[', '<']).unwrap_or(word.len());
                    let flag = word[..cut].trim_end_matches([':', '.']);
                    if flag.len() > 1
                        && flag
                            .chars()
                            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '?'))
                    {
                        flags.push(flag.to_owned());
                    }
                    if word.contains("[=") {
                        arity = Arity::Optional;
                    } else if word.contains('=') || word.contains('<') {
                        arity = Arity::One;
                    }
                } else if !flags.is_empty() {
                    // A single metavar in the specification is strong evidence of an argument.
                    if word.starts_with('[') {
                        arity = Arity::Optional;
                    } else if word.starts_with('<')
                        || word
                            .chars()
                            .all(|c| c.is_uppercase() || matches!(c, '_' | '-' | '0'..='9'))
                    {
                        arity = Arity::One;
                    } else {
                        arity = Arity::Unknown;
                    }
                    value_type = Some(text::compact(word, 128));
                }
            }
            flags.sort();
            flags.dedup();
            if flags.is_empty() {
                continue;
            }
            let preferred = flags.iter().position(|s| s.starts_with("--")).unwrap_or(0);
            let name = flags.remove(preferred);
            let cap = Capability {
                id: format!("{prefix}#{name}"),
                command: command.to_vec(),
                kind: Kind::Option,
                name,
                summary: text::compact(description, 1000),
                aliases: flags,
                arity,
                required: false,
                value_type,
                choices: vec![],
                source: source.clone(),
            };
            cap.validate()?;
            records.push(cap);
            last_option = Some(records.len() - 1);
            commands_section = false;
        } else if commands_section {
            if let Some(at) = line.find("  ") {
                let name = line[..at].trim();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
                {
                    let mut child = command.to_vec();
                    child.push(name.to_owned());
                    records.push(Capability {
                        id: format!("{prefix}/{name}#command"),
                        command: child,
                        kind: Kind::Command,
                        name: name.to_owned(),
                        summary: text::compact(&line[at..], 500),
                        aliases: vec![],
                        arity: Arity::Unknown,
                        required: false,
                        value_type: None,
                        choices: vec![],
                        source: source.clone(),
                    });
                }
            }
        } else if let Some(index) = last_option {
            if raw.starts_with(char::is_whitespace) && records[index].summary.len() < 1000 {
                let joined = format!("{} {}", records[index].summary, line);
                records[index].summary = text::compact(&joined, 1000);
            } else {
                last_option = None;
            }
        }
    }
    for cap in &records {
        cap.validate()?;
    }
    Ok(records)
}

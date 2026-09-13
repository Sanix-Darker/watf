//! Parse a deliberately small static subset of completion definitions.
//! Dynamic code, shell substitutions, callbacks, and conditional scopes are skipped.
use crate::{
    record::{Arity, Capability, Kind, Source, SourceKind},
    text, Error, Result,
};
use std::path::Path;

pub fn words(input: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut active = false;
    for ch in input.chars() {
        if escaped {
            current.push(ch);
            active = true;
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            active = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                current.push(ch);
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            active = true;
        } else if ch.is_whitespace() {
            if active {
                out.push(std::mem::take(&mut current));
                active = false;
            }
        } else {
            current.push(ch);
            active = true;
        }
    }
    if escaped || quote.is_some() {
        return Err(Error::message("unclosed quote or escape"));
    }
    if active {
        out.push(current);
    }
    Ok(out)
}

fn option(
    command: &[String],
    name: String,
    description: String,
    arity: Arity,
    source: &Source,
) -> Capability {
    Capability {
        id: format!("local:{}:{}#{name}", source.reference, command.join("/")),
        command: command.to_vec(),
        kind: Kind::Option,
        name,
        summary: text::compact(&description, 500),
        aliases: vec![],
        arity,
        required: false,
        value_type: None,
        choices: vec![],
        source: source.clone(),
    }
}

pub fn read(path: &Path, command: &[String], flavor: &str) -> Result<Vec<Capability>> {
    if command.is_empty() {
        return Err(Error::message("completion import requires a command scope"));
    }
    let bytes = super::read_document(path)?;
    let source = super::source(path, SourceKind::Completion, &bytes)?;
    let input = String::from_utf8_lossy(&bytes);
    let mut output = Vec::new();
    for raw in input.lines() {
        let line = raw.trim();
        if line.starts_with('#') || line.contains('$') || line.contains('`') {
            continue;
        }
        match flavor {
            "fish" => {
                let Ok(tokens) = words(line) else {
                    continue;
                };
                if tokens.first().map(String::as_str) != Some("complete")
                    || tokens.iter().any(|t| t == "-n" || t == "--condition")
                {
                    continue;
                }
                let get = |short: &str, long: &str| {
                    tokens
                        .windows(2)
                        .find(|w| w[0] == short || w[0] == long)
                        .map(|w| w[1].clone())
                };
                if get("-c", "--command").as_deref() != Some(command[0].as_str()) {
                    continue;
                }
                let arity = if tokens
                    .iter()
                    .any(|t| t == "-r" || t == "--require-parameter")
                {
                    Arity::One
                } else {
                    Arity::Unknown
                };
                let description = get("-d", "--description").unwrap_or_default();
                if let Some(name) = get("-l", "--long-option") {
                    output.push(option(
                        command,
                        format!("--{name}"),
                        description.clone(),
                        arity,
                        &source,
                    ));
                }
                if let Some(name) = get("-s", "--short-option") {
                    output.push(option(
                        command,
                        format!("-{name}"),
                        description,
                        arity,
                        &source,
                    ));
                }
            }
            "bash" => {
                let Ok(tokens) = words(line) else {
                    continue;
                };
                if tokens.first().map(String::as_str) != Some("complete")
                    || tokens.last() != command.first()
                {
                    continue;
                }
                if let Some(pair) = tokens.windows(2).find(|w| w[0] == "-W") {
                    for name in pair[1].split_whitespace().filter(|w| w.starts_with('-')) {
                        output.push(option(
                            command,
                            name.to_owned(),
                            "Static Bash completion word; arity unknown".to_owned(),
                            Arity::Unknown,
                            &source,
                        ));
                    }
                }
            }
            "zsh" => {
                // Accept simple standalone '-x[description]:value:' entries only.
                let line = line.trim_end_matches('\\').trim().trim_matches(['\'', '"']);
                if !line.starts_with('-') || line.contains('(') {
                    continue;
                }
                if let (Some(start), Some(end)) = (line.find('['), line.find(']')) {
                    if end <= start {
                        continue;
                    }
                    let name = &line[..start];
                    if name.chars().all(|c| c.is_alphanumeric() || c == '-') {
                        let arity = if line[end + 1..].starts_with(':') {
                            Arity::One
                        } else {
                            Arity::None
                        };
                        output.push(option(
                            command,
                            name.to_owned(),
                            line[start + 1..end].to_owned(),
                            arity,
                            &source,
                        ));
                    }
                }
            }
            _ => {
                return Err(Error::message(
                    "completion flavor must be fish, bash, or zsh",
                ))
            }
        }
    }
    for record in &output {
        record.validate()?;
    }
    Ok(output)
}

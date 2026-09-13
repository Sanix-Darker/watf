use crate::{
    record::{Capability, SourceKind},
    Error, Result,
};
use std::path::Path;

fn unescape(line: &str) -> String {
    let mut s = line.to_owned();
    for marker in ["\\fB", "\\fI", "\\fR", "\\fP", "\\fC", "\\&"] {
        s = s.replace(marker, "");
    }
    for (from, to) in [
        ("\\-", "-"),
        ("\\e", "\\"),
        ("\\(em", "-"),
        ("\\(en", "-"),
        ("\\~", " "),
        ("\\ ", " "),
        ("\\(aq", "'"),
    ] {
        s = s.replace(from, to);
    }
    s.replace('"', "")
}

/// Conservative roff/mdoc extraction, not a roff interpreter. Includes are not followed.
pub fn plain(input: &str) -> Result<String> {
    let mut result = String::new();
    let mut in_definition = false;
    for line in input.lines() {
        if line.starts_with(".so ") {
            return Err(Error::message(
                "roff .so include skipped; import its target explicitly",
            ));
        }
        if line.starts_with(".de ") || line.starts_with(".am ") {
            in_definition = true;
            continue;
        }
        if in_definition {
            if line == ".." {
                in_definition = false;
            }
            continue;
        }
        if line.starts_with(".\\\"") || line.starts_with("'\\\"") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('.') {
            let (name, content) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            match name {
                "TH" | "Dd" | "Dt" | "Os" => continue,
                "SH" | "SS" | "Sh" | "Ss" => {
                    result.push('\n');
                    result.push_str(&unescape(content));
                    result.push_str(":\n");
                }
                "TP" | "PP" | "P" | "Pp" | "LP" | "br" => result.push('\n'),
                "B" | "I" | "BR" | "BI" | "IR" | "RB" | "RI" | "IP" | "Nm" | "Nd" => {
                    result.push_str(&unescape(content));
                    result.push('\n');
                }
                "Fl" => {
                    result.push('-');
                    result.push_str(&unescape(content));
                    result.push('\n');
                }
                "Ar" => {
                    result.push('<');
                    result.push_str(&unescape(content));
                    result.push_str(">\n");
                }
                _ => {}
            }
        } else {
            result.push_str("    ");
            result.push_str(&unescape(line));
            result.push('\n');
        }
    }
    Ok(result)
}

pub fn read(path: &Path, command: Option<&[String]>) -> Result<Vec<Capability>> {
    let bytes = super::read_document(path)?;
    let source = super::source(path, SourceKind::Man, &bytes)?;
    let owned;
    let command = match command {
        Some(scope) => scope,
        None => {
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| Error::message("manual filename is not UTF-8"))?;
            let filename = filename.strip_suffix(".gz").unwrap_or(filename);
            let name = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
            owned = if let Some(subcommand) = name.strip_prefix("git-") {
                vec!["git".to_owned(), subcommand.to_owned()]
            } else {
                vec![name.to_owned()]
            };
            &owned
        }
    };
    super::help::parse(&plain(&String::from_utf8_lossy(&bytes))?, command, source)
}

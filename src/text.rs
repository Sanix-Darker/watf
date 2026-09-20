//! Bounded normalization and intent splitting. No learned indexer or embeddings.
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_QUERY_BYTES: usize = 16_384;
pub const MAX_TERMS: usize = 64;

fn stop(s: &str) -> bool {
    matches!(
        s,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "by"
            | "for"
            | "from"
            | "how"
            | "i"
            | "in"
            | "is"
            | "it"
            | "me"
            | "my"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "their"
            | "them"
            | "then"
            | "this"
            | "to"
            | "use"
            | "using"
            | "want"
            | "with"
    )
}

pub fn tokens(s: &str) -> Vec<String> {
    let mut output = Vec::new();
    for raw in s.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        if raw.is_empty() || raw.len() > 128 {
            continue;
        }
        // Preserve case for literal short options: -A and -a are different flags.
        if raw.starts_with('-') && raw.len() > 1 {
            output.push(format!("={raw}"));
        }
        for part in raw.split(['-', '_']) {
            let token = part.to_lowercase();
            if token.len() > 1 && !stop(&token) {
                output.push(token);
            }
        }
    }
    output
}

pub fn query_terms(s: &str) -> BTreeMap<String, f32> {
    let mut out = BTreeMap::new();
    for t in tokens(s).into_iter().take(MAX_TERMS) {
        out.insert(t, 1.0);
    }
    let originals: Vec<_> = out.keys().cloned().collect();
    for t in originals {
        let expansion: &[&str] = match t.as_str() {
            "stage" | "staging" => &["index", "add"],
            "ignore" | "skip" => &["exclude"],
            "background" => &["detach", "detached"],
            "directory" | "directories" => &["folder", "recursive"],
            "folder" => &["directory"],
            "rebuild" => &["build", "recreate"],
            "restart" => &["restart"],
            "tail" => &["follow"],
            "modified" | "changed" => &["mtime", "change"],
            "compression" | "compress" => &["archive", "gzip"],
            "permission" | "permissions" => &["mode", "preserve"],
            "hash" | "checksum" => &["sha256", "digest"],
            "commit" => &["record"],
            "find" => &["search"],
            "pid" => &["process"],
            "ports" => &["port", "socket"],
            "files" => &["file"],
            "containers" => &["container"],
            "logs" => &["log"],
            _ => &[],
        };
        for word in expansion {
            if out.len() >= MAX_TERMS {
                break;
            }
            out.entry((*word).to_owned()).or_insert(0.32);
        }
    }
    out
}

/// Split conjunctions only outside quotes. This is a retrieval heuristic, not a planner.
pub fn clauses(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if escaped {
            escaped = false;
            i += s[i..].chars().next().map(char::len_utf8).unwrap_or(1);
            continue;
        }
        if c == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'\'' || c == b'"' {
            quote = Some(c);
        } else {
            let rest = &s[i..];
            let skip = if rest.starts_with(" and ") {
                5
            } else if rest.starts_with(" then ") {
                6
            } else if rest.starts_with(" && ") {
                4
            } else if c == b';' || c == b'\n' || (c == b',' && comma_starts_action(&s[i + 1..])) {
                1
            } else {
                0
            };
            if skip > 0 && out.len() < 7 {
                let part = s[start..i].trim();
                if !part.is_empty() {
                    out.push(part.to_owned());
                }
                i += skip;
                start = i;
                continue;
            }
        }
        // Advance by a complete scalar so the next substring is valid UTF-8.
        i += s[i..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    let tail = s[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_owned());
    }
    if out.is_empty() {
        out.push(s.trim().to_owned());
    }
    out
}

fn comma_starts_action(rest: &str) -> bool {
    let first = rest
        .trim_start()
        .split(|c: char| !c.is_alphabetic())
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        first.as_str(),
        "add"
            | "apply"
            | "archive"
            | "build"
            | "check"
            | "commit"
            | "compare"
            | "convert"
            | "copy"
            | "create"
            | "display"
            | "download"
            | "extract"
            | "fetch"
            | "find"
            | "follow"
            | "inspect"
            | "list"
            | "move"
            | "print"
            | "pull"
            | "push"
            | "remove"
            | "restart"
            | "run"
            | "save"
            | "search"
            | "show"
            | "stage"
            | "start"
            | "stop"
            | "upload"
            | "validate"
            | "verify"
    )
}

pub fn compact(s: &str, max_chars: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

pub fn compact_bytes(s: &str, max_bytes: usize) -> String {
    let mut out = compact(s, max_bytes);
    let mut end = out.len().min(max_bytes);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out
}

pub fn unique_terms(s: &str) -> BTreeSet<String> {
    tokens(s).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_flags_keep_case() {
        assert!(tokens("-A -a").contains(&"=-A".to_owned()));
    }
    #[test]
    fn quoted_conjunction_is_literal() {
        assert_eq!(clauses("commit 'salt and pepper' then build").len(), 2);
    }
    #[test]
    fn comma_separated_actions_are_clauses() {
        assert_eq!(
            clauses("show the diff, stage the file, commit it, then display the latest commit")
                .len(),
            4
        );
        assert_eq!(clauses("show files, directories, and links").len(), 2);
        assert_eq!(clauses("print 'build, run, verify', then stop").len(), 2);
    }
    #[test]
    fn unicode_is_safe() {
        assert_eq!(clauses("créer un dépôt and build").len(), 2);
    }
    #[test]
    fn expansion_is_bounded() {
        assert!(query_terms(&"stage ".repeat(10000)).len() <= MAX_TERMS);
    }
}

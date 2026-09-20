//! Deterministic command-scope routing from retrieval scores. No model calls.
use crate::packet::Packet;
use serde::Serialize;

pub const GENERIC_INTENT_TERMS: &[&str] = &[
    "check", "current", "display", "get", "please", "print", "run", "show",
];

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub command: String,
    pub score: f32,
    pub clause_coverage: usize,
    pub matched_terms: u16,
    pub program_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub candidates: Vec<Candidate>,
    pub reason: &'static str,
}

pub fn decisive_command(classification: &Classification, clause_count: usize) -> Option<&str> {
    let top = classification.candidates.first()?;
    if top.clause_coverage < clause_count || top.matched_terms < 2 {
        return None;
    }
    let decisive = match classification.candidates.get(1) {
        None => true,
        Some(second) if top.clause_coverage > second.clause_coverage => true,
        Some(second) => {
            top.score >= second.score * 1.35
                || (top.matched_terms > second.matched_terms && top.score >= second.score * 1.15)
        }
    };
    decisive.then_some(top.command.as_str())
}

pub fn classify(packet: &Packet, query: &str) -> Classification {
    struct Group<'a> {
        command: &'a str,
        score: f32,
        clauses: u8,
        matched_terms: u16,
        program_available: bool,
    }

    let mut grouped: Vec<Group<'_>> = Vec::with_capacity(packet.evidence.len().min(16));
    for evidence in &packet.evidence {
        let group = if let Some(group) = grouped
            .iter_mut()
            .find(|group| group.command == evidence.command)
        {
            group
        } else {
            grouped.push(Group {
                command: &evidence.command,
                score: 0.0,
                clauses: 0,
                matched_terms: 0,
                program_available: false,
            });
            grouped.last_mut().unwrap()
        };
        group.score = group.score.max(evidence.score);
        for &clause in &evidence.clauses {
            if clause < 8 {
                group.clauses |= 1 << clause;
            }
        }
        group.matched_terms = group.matched_terms.max(evidence.matched_terms);
        group.program_available |= evidence.program_available;
    }
    let mut candidates: Vec<_> = grouped
        .into_iter()
        .map(|group| Candidate {
            command: group.command.to_owned(),
            score: group.score,
            clause_coverage: group.clauses.count_ones() as usize,
            matched_terms: group.matched_terms,
            program_available: group.program_available,
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.clause_coverage
            .cmp(&a.clause_coverage)
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.command.cmp(&b.command))
    });
    let Some(top) = candidates.first() else {
        return Classification {
            status: "none",
            command: None,
            candidates,
            reason: "no_evidence",
        };
    };
    if packet.clause_count != 1 {
        return Classification {
            status: "ambiguous",
            command: None,
            candidates,
            reason: "multi_clause_requires_planning",
        };
    }
    if top.clause_coverage < packet.clause_count {
        return Classification {
            status: "ambiguous",
            command: None,
            candidates,
            reason: "incomplete_clause_coverage",
        };
    }
    if let Some(index) = explicit_scope_candidate(query, &candidates, packet.clause_count) {
        let selected = candidates.remove(index);
        let command = selected.command.clone();
        candidates.insert(0, selected);
        return Classification {
            status: "resolved",
            command: Some(command),
            candidates: candidates.into_iter().take(4).collect(),
            reason: "explicit_scope_terms",
        };
    }
    let probe = Classification {
        status: "ambiguous",
        command: None,
        candidates: candidates.iter().take(4).cloned().collect(),
        reason: "insufficient_margin",
    };
    let command = decisive_command(&probe, packet.clause_count).map(str::to_owned);
    if command
        .as_deref()
        .is_some_and(|command| shared_leaf_ambiguity(query, command, &candidates))
    {
        return Classification {
            status: "ambiguous",
            command: None,
            candidates: probe.candidates,
            reason: "shared_leaf_ambiguity",
        };
    }
    let decisive = command.is_some();
    Classification {
        status: if decisive { "resolved" } else { "ambiguous" },
        command,
        candidates: probe.candidates,
        reason: if decisive {
            "retrieval_margin"
        } else {
            "insufficient_margin"
        },
    }
}

fn explicit_scope_candidate(
    query: &str,
    candidates: &[Candidate],
    clause_count: usize,
) -> Option<usize> {
    let query_tokens = crate::text::tokens(query);
    let mut matches = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.clause_coverage < clause_count {
            continue;
        }
        let Some(leaf) = candidate.command.split_whitespace().last() else {
            continue;
        };
        if candidates
            .iter()
            .filter(|peer| {
                peer.clause_coverage >= clause_count
                    && peer.command.split_whitespace().last() == Some(leaf)
            })
            .take(2)
            .count()
            < 2
        {
            continue;
        }
        let command_tokens = crate::text::tokens(&candidate.command);
        if !command_tokens.is_empty()
            && query_tokens
                .windows(command_tokens.len())
                .any(|window| window == command_tokens)
        {
            matches.push((index, command_tokens.len()));
        }
    }
    let longest = matches.iter().map(|(_, len)| *len).max()?;
    let mut longest_matches = matches
        .into_iter()
        .filter(|(_, len)| *len == longest)
        .map(|(index, _)| index);
    let selected = longest_matches.next()?;
    longest_matches.next().is_none().then_some(selected)
}

fn shared_leaf_ambiguity(query: &str, command: &str, candidates: &[Candidate]) -> bool {
    let leaf = command.split_whitespace().last().unwrap_or("");
    let leaf_terms = crate::text::tokens(leaf)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if leaf_terms.is_empty() {
        return false;
    }
    let query_terms = crate::text::tokens(query);
    if !leaf_terms.iter().all(|term| query_terms.contains(term))
        || query_terms.iter().any(|term| {
            !leaf_terms.contains(term) && !GENERIC_INTENT_TERMS.contains(&term.as_str())
        })
    {
        return false;
    }
    candidates
        .iter()
        .filter(|candidate| {
            candidate.program_available
                && candidate.clause_coverage >= 1
                && candidate.command.split_whitespace().last() == Some(leaf)
        })
        .take(2)
        .count()
        >= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        index::SearchStats,
        packet::Evidence,
        record::{Arity, Kind},
    };

    fn evidence(command: &str, score: f32, clauses: &[usize], matched_terms: u16) -> Evidence {
        Evidence {
            id: format!("{command}#command"),
            command: command.to_owned(),
            kind: Kind::Command,
            name: command.split_whitespace().last().unwrap().to_owned(),
            aliases: vec![],
            arity: Arity::Unknown,
            summary: String::new(),
            source: 0,
            program_available: true,
            clauses: clauses.to_vec(),
            doc: 0,
            score,
            matched_terms,
        }
    }

    fn packet(evidence: Vec<Evidence>, clause_count: usize) -> Packet {
        Packet {
            schema_version: crate::SCHEMA_VERSION,
            status: "evidence".to_owned(),
            evidence,
            sources: vec![],
            clause_count,
            uncovered_clauses: vec![],
            truncated: false,
            estimated_tokens: 0,
            token_estimator: "test",
            stats: SearchStats::default(),
        }
    }

    #[test]
    fn route_requires_coverage_or_margin() {
        let resolved = classify(
            &packet(
                vec![
                    evidence("git status", 10.0, &[0], 3),
                    evidence("git log", 4.0, &[0], 2),
                ],
                1,
            ),
            "show git status",
        );
        assert_eq!(resolved.command.as_deref(), Some("git status"));

        let ambiguous = classify(
            &packet(
                vec![
                    evidence("git status", 10.0, &[0], 3),
                    evidence("git log", 9.0, &[0], 3),
                ],
                1,
            ),
            "show git status",
        );
        assert_eq!(ambiguous.status, "ambiguous");

        let extra_term = classify(
            &packet(
                vec![
                    evidence("git status", 12.0, &[0], 3),
                    evidence("git log", 10.0, &[0], 2),
                ],
                1,
            ),
            "show git status",
        );
        assert_eq!(extra_term.command.as_deref(), Some("git status"));

        let partial = classify(
            &packet(vec![evidence("git status", 10.0, &[0], 3)], 2),
            "show git status then log",
        );
        assert_eq!(partial.reason, "multi_clause_requires_planning");
    }

    #[test]
    fn explicit_scope_resolves_shared_leaf_with_inverted_scores() {
        let mut unavailable = evidence("kubectl logs", 4.0, &[0], 2);
        unavailable.program_available = false;
        let cases = [
            (
                "bare shared leaf",
                "show logs",
                vec![
                    evidence("docker compose logs", 10.0, &[0], 3),
                    evidence("docker logs", 4.0, &[0], 2),
                ],
                "shared_leaf_ambiguity",
                None,
            ),
            (
                "shorter explicit scope",
                "show docker logs",
                vec![
                    evidence("docker compose logs", 10.0, &[0], 3),
                    evidence("docker logs", 4.0, &[0], 2),
                ],
                "explicit_scope_terms",
                Some("docker logs"),
            ),
            (
                "longer explicit scope",
                "show docker compose logs",
                vec![
                    evidence("docker logs", 10.0, &[0], 2),
                    evidence("docker compose logs", 4.0, &[0], 3),
                ],
                "explicit_scope_terms",
                Some("docker compose logs"),
            ),
            (
                "unavailable peer",
                "show logs",
                vec![evidence("docker logs", 10.0, &[0], 2), unavailable],
                "retrieval_margin",
                Some("docker logs"),
            ),
            (
                "single scope",
                "show logs",
                vec![evidence("docker logs", 10.0, &[0], 2)],
                "retrieval_margin",
                Some("docker logs"),
            ),
            (
                "different leaves",
                "show logs",
                vec![
                    evidence("docker logs", 10.0, &[0], 2),
                    evidence("git status", 4.0, &[0], 2),
                ],
                "retrieval_margin",
                Some("docker logs"),
            ),
        ];
        for (name, query, evidence, reason, command) in cases {
            let classification = classify(&packet(evidence, 1), query);
            assert_eq!(classification.reason, reason, "{name}");
            assert_eq!(classification.command.as_deref(), command, "{name}");
            if let Some(command) = command {
                assert_eq!(classification.candidates[0].command, command, "{name}");
            }
        }
    }
}

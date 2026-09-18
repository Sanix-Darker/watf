//! Deterministic command-scope routing from retrieval scores. No model calls.
use crate::packet::Packet;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

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

pub fn classify(packet: &Packet) -> Classification {
    let mut grouped: BTreeMap<&str, (f32, BTreeSet<usize>, u16, bool)> = BTreeMap::new();
    for evidence in &packet.evidence {
        let entry = grouped
            .entry(&evidence.command)
            .or_insert((0.0, BTreeSet::new(), 0, false));
        entry.0 = entry.0.max(evidence.score);
        entry.1.extend(evidence.clauses.iter().copied());
        entry.2 = entry.2.max(evidence.matched_terms);
        entry.3 |= evidence.program_available;
    }
    let mut candidates: Vec<_> = grouped
        .into_iter()
        .map(
            |(command, (score, clauses, matched_terms, program_available))| Candidate {
                command: command.to_owned(),
                score,
                clause_coverage: clauses.len(),
                matched_terms,
                program_available,
            },
        )
        .collect();
    candidates.sort_by(|a, b| {
        b.clause_coverage
            .cmp(&a.clause_coverage)
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.command.cmp(&b.command))
    });
    candidates.truncate(4);
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
    let decisive = top.matched_terms >= 2
        && match candidates.get(1) {
            None => true,
            Some(second) if top.clause_coverage > second.clause_coverage => true,
            Some(second) => top.score >= second.score * 1.35,
        };
    let command = decisive.then(|| top.command.clone());
    Classification {
        status: if decisive { "resolved" } else { "ambiguous" },
        command,
        candidates,
        reason: if decisive {
            "retrieval_margin"
        } else {
            "insufficient_margin"
        },
    }
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
        let resolved = classify(&packet(
            vec![
                evidence("git status", 10.0, &[0], 3),
                evidence("git log", 4.0, &[0], 2),
            ],
            1,
        ));
        assert_eq!(resolved.command.as_deref(), Some("git status"));

        let ambiguous = classify(&packet(
            vec![
                evidence("git status", 10.0, &[0], 3),
                evidence("git log", 9.0, &[0], 3),
            ],
            1,
        ));
        assert_eq!(ambiguous.status, "ambiguous");

        let partial = classify(&packet(vec![evidence("git status", 10.0, &[0], 3)], 2));
        assert_eq!(partial.reason, "multi_clause_requires_planning");
    }
}

use super::{Index, POSTING};
use crate::{text, Error, Result};
use serde::Serialize;
use std::{
    cmp::{Ordering, Reverse},
    collections::{BTreeSet, BinaryHeap},
};

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    pub include_catalog: bool,
    pub include_fields: bool,
    pub command: Option<String>,
    pub roots: Option<BTreeSet<String>>,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 32,
            include_catalog: false,
            include_fields: false,
            command: None,
            roots: None,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub doc: u32,
    pub score: f32,
    pub matched_terms: u16,
}
#[derive(Debug, Default, Clone, Serialize)]
pub struct SearchStats {
    pub postings_scanned: usize,
    pub candidates_scored: usize,
    pub matched_query_terms: usize,
}

#[derive(Default)]
pub struct Scratch {
    scores: Vec<f32>,
    marks: Vec<u32>,
    matches: Vec<u16>,
    touched: Vec<u32>,
    epoch: u32,
}
impl Scratch {
    fn add(&mut self, doc: u32, impact: f32, count: usize) -> Result<()> {
        let d = doc as usize;
        if d >= count || !impact.is_finite() || impact < 0.0 {
            return Err(Error::message("corrupt posting document or impact"));
        }
        if self.marks[d] != self.epoch {
            self.marks[d] = self.epoch;
            self.scores[d] = 0.0;
            self.matches[d] = 0;
            self.touched.push(doc);
        }
        self.scores[d] += impact;
        self.matches[d] = self.matches[d].saturating_add(1);
        Ok(())
    }
    fn reset(&mut self, count: usize) {
        if self.scores.len() != count {
            self.scores.resize(count, 0.0);
            self.marks.resize(count, 0);
            self.matches.resize(count, 0);
        }
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.marks.fill(0);
            self.epoch = 1;
        }
        self.touched.clear();
    }
}

#[derive(Debug, Clone)]
struct Scored {
    doc: u32,
    score: f32,
    terms: u16,
}
impl PartialEq for Scored {
    fn eq(&self, other: &Self) -> bool {
        self.doc == other.doc && self.score.to_bits() == other.score.to_bits()
    }
}
impl Eq for Scored {}
impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Scored {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.doc.cmp(&self.doc))
    }
}

impl Index {
    pub fn search(
        &self,
        query: &str,
        options: &SearchOptions,
        scratch: &mut Scratch,
    ) -> Result<(Vec<Hit>, SearchStats)> {
        if query.len() > text::MAX_QUERY_BYTES {
            return Err(Error::message("query exceeds 16 KiB"));
        }
        if !(1..=256).contains(&options.limit) {
            return Err(Error::message("search limit must be 1..256"));
        }
        scratch.reset(self.count);
        let mut stats = SearchStats::default();
        let mut lists = Vec::new();
        for (term, weight) in text::query_terms(query) {
            let term = if options.include_catalog {
                term
            } else {
                format!("~{term}")
            };
            if let Some((offset, count)) = self.term(&term)? {
                lists.push((count, offset, weight));
            }
        }
        lists.sort_by_key(|p| p.0);
        stats.matched_query_terms = lists.len();
        let scope = options
            .command
            .as_deref()
            .map(|key| self.ids_for_command(key))
            .transpose()?;
        for (count, start, weight) in lists {
            // Exact scopes are usually tiny. Probe their IDs in sorted posting
            // lists rather than scanning a giant global list on every subquery.
            if let Some(ids) = &scope {
                for &doc in ids {
                    let mut low = 0;
                    let mut high = count;
                    while low < high {
                        stats.postings_scanned += 1;
                        let mid = low + (high - low) / 2;
                        let found = self.number(start + mid * POSTING)?;
                        if found < doc {
                            low = mid + 1;
                        } else {
                            high = mid;
                        }
                    }
                    if low < count && self.number(start + low * POSTING)? == doc {
                        let impact = f32::from_bits(self.number(start + low * POSTING + 4)?);
                        scratch.add(doc, impact * weight, self.count)?;
                    }
                }
            } else {
                stats.postings_scanned += count;
                for n in 0..count {
                    let at = start + n * POSTING;
                    let doc = self.number(at)?;
                    let impact = f32::from_bits(self.number(at + 4)?);
                    scratch.add(doc, impact * weight, self.count)?;
                }
            }
        }
        let mut heap = BinaryHeap::<Reverse<Scored>>::with_capacity(options.limit + 1);
        for &doc in &scratch.touched {
            let priority = self.priority(doc)?;
            if priority == 1 && !options.include_catalog {
                continue;
            }
            if self.kind_code(doc)? == 2 && !options.include_fields {
                continue;
            }
            if let Some(command) = &options.command {
                if self.command_key(doc)? != command {
                    continue;
                }
            }
            if let Some(roots) = &options.roots {
                if !roots.contains(self.root(doc)?) {
                    continue;
                }
            }
            stats.candidates_scored += 1;
            let d = doc as usize;
            // Score is relevance, not probability. Slightly prefer local evidence.
            let score = scratch.scores[d]
                * (1.0 + f32::from(priority.saturating_sub(1)) * 0.03)
                * (1.0 + f32::from(scratch.matches[d].min(8)) * 0.06);
            let candidate = Scored {
                doc,
                score,
                terms: scratch.matches[d],
            };
            if heap.len() < options.limit {
                heap.push(Reverse(candidate));
            } else if heap.peek().is_some_and(|min| candidate > min.0) {
                heap.pop();
                heap.push(Reverse(candidate));
            }
        }
        let mut ranked: Vec<_> = heap.into_iter().map(|r| r.0).collect();
        ranked.sort_by(|a, b| b.cmp(a));
        Ok((
            ranked
                .into_iter()
                .map(|s| Hit {
                    doc: s.doc,
                    score: s.score,
                    matched_terms: s.terms,
                })
                .collect(),
            stats,
        ))
    }
}

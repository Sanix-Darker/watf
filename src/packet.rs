//! Hierarchical, clause-aware retrieval with a hard serialized-output byte cap.
use crate::{
    discover::{self, Executable, Freshness},
    index::{Hit, Index, Scratch, SearchOptions, SearchStats},
    record::{Arity, Kind, Source},
    text, Error, Result, SCHEMA_VERSION,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone)]
pub struct Options {
    pub max_bytes: usize,
    pub limit: usize,
    pub catalog: bool,
    pub fields: bool,
    pub installed_only: bool,
    pub command: Option<String>,
    pub project_hints: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            max_bytes: 4096,
            limit: 16,
            catalog: false,
            fields: false,
            installed_only: false,
            command: None,
            project_hints: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub id: String,
    pub command: String,
    pub kind: Kind,
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub arity: Arity,
    pub summary: String,
    pub source: usize,
    pub program_available: bool,
    pub clauses: Vec<usize>,
    // Internal ID avoids reparsing stable IDs. Never serialized into agent context.
    #[serde(skip)]
    pub doc: u32,
    #[serde(skip)]
    pub score: f32,
    #[serde(skip)]
    pub matched_terms: u16,
}
#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    #[serde(flatten)]
    pub source: Source,
    pub freshness: Freshness,
}
#[derive(Debug, Clone, Serialize)]
pub struct Packet {
    pub schema_version: u32,
    pub status: String,
    pub evidence: Vec<Evidence>,
    pub sources: Vec<Provenance>,
    pub clause_count: usize,
    pub uncovered_clauses: Vec<usize>,
    pub truncated: bool,
    pub estimated_tokens: usize,
    pub token_estimator: &'static str,
    #[serde(skip)]
    pub stats: SearchStats,
}
impl Packet {
    fn refresh(&mut self) {
        self.uncovered_clauses = (0..self.clause_count)
            .filter(|c| !self.evidence.iter().any(|e| e.clauses.contains(c)))
            .collect();
        self.status = if self.evidence.is_empty() {
            "no_evidence"
        } else if !self.uncovered_clauses.is_empty() {
            "partial"
        } else {
            "evidence"
        }
        .to_owned();
        let old = std::mem::take(&mut self.sources);
        let mut remap = BTreeMap::new();
        for evidence in &mut self.evidence {
            let next = if let Some(&index) = remap.get(&evidence.source) {
                index
            } else {
                let index = self.sources.len();
                self.sources.push(old[evidence.source].clone());
                remap.insert(evidence.source, index);
                index
            };
            evidence.source = next;
        }
    }
    pub fn json(&self) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
    pub fn fit(&mut self, max_bytes: usize) -> Result<()> {
        loop {
            self.refresh();
            // Fixed-point estimate includes the full response, not only descriptions.
            let bytes = loop {
                let bytes = self.json()?;
                let estimated = bytes.len().div_ceil(4);
                if estimated == self.estimated_tokens {
                    break bytes;
                }
                self.estimated_tokens = estimated;
            };
            if bytes.len() <= max_bytes {
                return Ok(());
            }
            if self.evidence.pop().is_none() {
                return Err(Error::message(
                    "byte budget cannot hold an empty evidence packet",
                ));
            }
            self.truncated = true;
        }
    }
}

pub struct Engine {
    pub index: Index,
    scratch: Scratch,
    pub inventory: BTreeMap<String, Executable>,
    hints: Vec<String>,
}
impl Engine {
    pub fn open(path: &Path, mmap: bool) -> Result<Self> {
        let mut engine = Self::new(Index::open(path, mmap)?);
        engine.inventory = discover::executables();
        if let Ok(cwd) = std::env::current_dir() {
            engine.hints = discover::project_hints(&cwd);
        }
        Ok(engine)
    }
    /// Deterministic constructor for embedding and benchmarks, without environment inspection.
    pub fn new(index: Index) -> Self {
        Self {
            index,
            scratch: Scratch::default(),
            inventory: BTreeMap::new(),
            hints: Vec::new(),
        }
    }

    pub fn lookup(&mut self, query: &str, options: &Options) -> Result<Packet> {
        if query.trim().is_empty() || query.len() > text::MAX_QUERY_BYTES {
            return Err(Error::message("query must contain 1..16384 bytes"));
        }
        if !(256..=1_048_576).contains(&options.max_bytes) || !(1..=128).contains(&options.limit) {
            return Err(Error::message(
                "max-bytes must be 256..1048576 and limit must be 1..128",
            ));
        }
        let clauses = text::clauses(query);
        let catalog = options.catalog || text::unique_terms(query).contains("aws");
        let roots = options
            .installed_only
            .then(|| self.inventory.keys().cloned().collect::<BTreeSet<_>>());
        let mut base = SearchOptions {
            limit: 64,
            include_catalog: catalog,
            include_fields: options.fields,
            command: options.command.clone(),
            roots,
        };
        let mut queues: Vec<Vec<Hit>> = Vec::new();
        let mut stats = SearchStats::default();
        for clause in &clauses {
            let (mut hits, found) = self.index.search(clause, &base, &mut self.scratch)?;
            add_stats(&mut stats, &found);
            if options.project_hints {
                for hit in &mut hits {
                    if self
                        .hints
                        .iter()
                        .any(|h| self.index.root(hit.doc).is_ok_and(|r| r == h))
                    {
                        hit.score *= 1.06;
                    }
                }
                hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.doc.cmp(&b.doc)));
            }
            if options.fields {
                queues.push(hits);
                continue;
            }
            let mut commands = BTreeSet::new();
            let mut heads = Vec::new();
            let mut details = Vec::new();
            for hit in hits {
                let command = self.index.command_key(hit.doc)?.to_owned();
                if !commands.insert(command.clone()) {
                    continue;
                }
                if commands.len() > 4 {
                    break;
                }
                let scoped_ids = self.index.ids_for_command(&command)?;
                if let Some(id) = scoped_ids
                    .into_iter()
                    .find(|&id| self.index.kind_code(id).is_ok_and(|kind| kind == 0))
                {
                    heads.push(Hit {
                        doc: id,
                        score: hit.score,
                        matched_terms: hit.matched_terms,
                    });
                }
                base.command = Some(command);
                base.limit = 6;
                let (fine, found) = self.index.search(clause, &base, &mut self.scratch)?;
                add_stats(&mut stats, &found);
                details.extend(
                    fine.into_iter()
                        .filter(|h| self.index.kind_code(h.doc).is_ok_and(|k| k == 1 || k == 3)),
                );
                base.command = options.command.clone();
                base.limit = 64;
            }
            heads.extend(details);
            let queue = heads;
            queues.push(queue);
        }
        let available = queues
            .iter()
            .flatten()
            .map(|hit| hit.doc)
            .collect::<BTreeSet<_>>()
            .len();
        let mut selected: Vec<Evidence> = Vec::new();
        let mut source_keys = BTreeMap::new();
        let mut sources = Vec::new();
        let mut doc_indices = BTreeMap::new();
        // Round robin over clauses prevents one verbose tool from using every slot.
        for depth in 0..queues.iter().map(Vec::len).max().unwrap_or(0) {
            for (clause, queue) in queues.iter().enumerate() {
                let Some(hit) = queue.get(depth) else {
                    continue;
                };
                if let Some(&existing) = doc_indices.get(&hit.doc) {
                    let evidence: &mut Evidence = &mut selected[existing];
                    if !evidence.clauses.contains(&clause) {
                        evidence.clauses.push(clause);
                    }
                    evidence.score = evidence.score.max(hit.score);
                    evidence.matched_terms = evidence.matched_terms.max(hit.matched_terms);
                    continue;
                }
                if selected.len() >= options.limit {
                    continue;
                }
                let capability = self.index.capability(hit.doc)?;
                let key = serde_json::to_string(&capability.source)?;
                let source = if let Some(&id) = source_keys.get(&key) {
                    id
                } else {
                    let id = sources.len();
                    sources.push(Provenance {
                        freshness: discover::freshness(&capability.source),
                        source: capability.source.clone(),
                    });
                    source_keys.insert(key, id);
                    id
                };
                let available = self.inventory.contains_key(capability.root());
                let position = selected.len();
                doc_indices.insert(hit.doc, position);
                selected.push(Evidence {
                    id: capability.id,
                    command: capability.command.join(" "),
                    kind: capability.kind,
                    name: capability.name,
                    aliases: capability.aliases,
                    arity: capability.arity,
                    summary: text::compact(&capability.summary, 160),
                    source,
                    program_available: available,
                    clauses: vec![clause],
                    doc: hit.doc,
                    score: hit.score,
                    matched_terms: hit.matched_terms,
                });
            }
        }
        let truncated = selected.len() < available;
        let mut packet = Packet {
            schema_version: SCHEMA_VERSION,
            status: String::new(),
            evidence: selected,
            sources,
            clause_count: clauses.len(),
            uncovered_clauses: vec![],
            truncated,
            estimated_tokens: 0,
            token_estimator: "utf8_bytes_div4_not_model_tokens",
            stats,
        };
        packet.fit(options.max_bytes)?;
        Ok(packet)
    }
}
fn add_stats(target: &mut SearchStats, value: &SearchStats) {
    target.postings_scanned += value.postings_scanned;
    target.candidates_scored += value.candidates_scored;
    target.matched_query_terms += value.matched_query_terms;
}

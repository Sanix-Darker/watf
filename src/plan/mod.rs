//! Bounded plan IR, documented-surface validation, and inert shell rendering.
mod render;
mod validate;

use crate::{
    index::Index,
    packet::Packet,
    record::{Capability, Kind},
    Error, Result,
};
pub use render::{quote, render};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub use validate::{validate, ValidationOptions};

pub const MAX_STEPS: usize = 8;
pub const MAX_ARGS: usize = 64;
pub const MAX_LITERAL: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    NeedsInput,
    Unsupported,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum After {
    Start,
    Success,
    Always,
    Pipe,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedirectMode {
    Truncate,
    Append,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redirect {
    pub mode: RedirectMode,
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Canonical indexed scope, for example `git commit`. Not shell source.
    pub command: String,
    /// Literal argv after the canonical command components.
    pub args: Vec<String>,
    pub after: After,
    pub stdout: Option<Redirect>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub status: Status,
    pub steps: Vec<Step>,
    pub questions: Vec<String>,
}
impl Draft {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 128 * 1024 {
            return Err(Error::message("plan exceeds 128 KiB"));
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct StepEvidence {
    pub step: usize,
    pub ids: Vec<String>,
    pub program_available: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub status: Status,
    pub accepted: bool,
    pub executed: bool,
    pub approval_required: bool,
    pub validation_scope: &'static str,
    pub steps: Vec<Step>,
    pub evidence: Vec<StepEvidence>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub questions: Vec<String>,
    pub shell: Option<String>,
    pub shell_dialect: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Context {
    pub commands: Vec<CommandContext>,
    pub uncovered_clauses: Vec<usize>,
}
#[derive(Debug, Clone, Serialize)]
pub struct CommandContext {
    pub command: String,
    pub summary: String,
    pub options: Vec<OptionContext>,
}
#[derive(Debug, Clone, Serialize)]
pub struct OptionContext {
    pub name: String,
    pub aliases: Vec<String>,
    pub arity: crate::record::Arity,
    pub required: bool,
    pub description: String,
}

/// Retrieve the smallest selected command surfaces, with no model calls. The
/// inference adapter checks the real tokenizer budget before attempting a plan.
pub fn context(index: &Index, packet: &Packet) -> Result<Context> {
    let mut seen = BTreeSet::new();
    let mut commands = Vec::new();
    for evidence in &packet.evidence {
        if !seen.insert(evidence.command.clone()) || commands.len() >= 8 {
            continue;
        }
        let records: Vec<Capability> = index
            .ids_for_command(&evidence.command)?
            .into_iter()
            .map(|id| index.capability(id))
            .collect::<Result<_>>()?;
        let Some(command) = records.iter().find(|r| r.kind == Kind::Command) else {
            continue;
        };
        let selected: BTreeSet<_> = packet
            .evidence
            .iter()
            .filter(|e| e.command == evidence.command)
            .map(|e| e.id.as_str())
            .collect();
        let mut flags: Vec<_> = records.iter().filter(|r| r.kind == Kind::Option).collect();
        flags.sort_by(|a, b| {
            b.required
                .cmp(&a.required)
                .then_with(|| {
                    selected
                        .contains(b.id.as_str())
                        .cmp(&selected.contains(a.id.as_str()))
                })
                .then(a.name.cmp(&b.name))
        });
        let options = flags
            .into_iter()
            .take(24)
            .map(|r| OptionContext {
                name: r.name.clone(),
                aliases: r.aliases.clone(),
                arity: r.arity,
                required: r.required,
                description: crate::text::compact(&r.summary, 160),
            })
            .collect();
        commands.push(CommandContext {
            command: evidence.command.clone(),
            summary: crate::text::compact(&command.summary, 180),
            options,
        });
    }
    Ok(Context {
        commands,
        uncovered_clauses: packet.uncovered_clauses.clone(),
    })
}

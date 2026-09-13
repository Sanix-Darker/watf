use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Command,
    Option,
    InputField,
    Example,
}
impl Kind {
    pub fn code(self) -> u8 {
        match self {
            Self::Command => 0,
            Self::Option => 1,
            Self::InputField => 2,
            Self::Example => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arity {
    None,
    One,
    Two,
    Optional,
    Many,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Builtin,
    Man,
    Help,
    Completion,
    LocalDoc,
    Catalog,
}
impl SourceKind {
    pub fn priority(self) -> u8 {
        match self {
            Self::Help => 5,
            Self::Man => 4,
            Self::Completion => 3,
            Self::Builtin | Self::LocalDoc => 2,
            Self::Catalog => 1,
        }
    }
    pub fn is_local(self) -> bool {
        matches!(
            self,
            Self::Man | Self::Help | Self::Completion | Self::LocalDoc
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub kind: SourceKind,
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub command: Vec<String>,
    pub kind: Kind,
    pub name: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub arity: Arity,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    pub source: Source,
}
impl Capability {
    pub fn root(&self) -> &str {
        self.command.first().map(String::as_str).unwrap_or("")
    }
    pub fn command_key(&self) -> String {
        self.command.join(" ")
    }
    pub fn key(&self) -> String {
        format!(
            "{}\x1f{}\x1f{}",
            self.command_key(),
            self.kind.code(),
            self.name
        )
    }
    pub fn matches_flag(&self, flag: &str) -> bool {
        self.kind == Kind::Option && (self.name == flag || self.aliases.iter().any(|a| a == flag))
    }
    pub fn validate(&self) -> Result<()> {
        if self.command.is_empty() || self.command.len() > 12 {
            return Err(Error::message("capability needs 1..12 command components"));
        }
        if self.command.iter().any(|s| {
            s.is_empty() || s.len() > 128 || s.chars().any(|c| c.is_control() || c.is_whitespace())
        }) {
            return Err(Error::message("invalid command component"));
        }
        if self.id.is_empty()
            || self.id.len() > 2048
            || self.name.len() > 512
            || self.summary.len() > 8192
            || self.source.reference.len() > 4096
        {
            return Err(Error::message("capability exceeds a field limit"));
        }
        if self.id.chars().any(char::is_control)
            || self.source.reference.chars().any(char::is_control)
            || self
                .source
                .version
                .as_ref()
                .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
            || self
                .source
                .sha256
                .as_ref()
                .is_some_and(|v| v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(Error::message("invalid provenance metadata"));
        }
        if self.aliases.len() > 64 || self.choices.len() > 8192 {
            return Err(Error::message(
                "capability has excessive aliases or choices",
            ));
        }
        for s in std::iter::once(&self.name)
            .chain(self.aliases.iter())
            .chain(self.choices.iter())
        {
            if s.len() > 4096 || s.chars().any(|c| c.is_control()) {
                return Err(Error::message(
                    "control character or oversized capability value",
                ));
            }
        }
        Ok(())
    }
}

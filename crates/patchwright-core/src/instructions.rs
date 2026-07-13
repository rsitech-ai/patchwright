use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Write};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstructionKind {
    Organization,
    User,
    ProjectConfiguration,
    RootAgents,
    DirectoryAgents,
    Branch,
    Task,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionSource {
    pub kind: InstructionKind,
    pub location: String,
    pub content: String,
    pub content_sha256: String,
    pub enforced: bool,
}

impl InstructionSource {
    #[must_use]
    pub fn new(
        kind: InstructionKind,
        location: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let content = content.into();
        let content_sha256 = hex_digest(content.as_bytes());
        Self {
            kind,
            location: location.into(),
            content,
            content_sha256,
            enforced: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionConflict {
    pub key: String,
    pub values: Vec<String>,
    pub effective_value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveInstructions {
    pub sources: Vec<InstructionSource>,
    pub conflicts: Vec<InstructionConflict>,
}

pub struct InstructionResolver;

impl InstructionResolver {
    #[must_use]
    pub fn resolve(mut sources: Vec<InstructionSource>) -> EffectiveInstructions {
        sources.sort_by_key(|source| source.kind);
        let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for source in &sources {
            for line in source.content.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();
                    if !key.is_empty() && !value.is_empty() {
                        values
                            .entry(key.to_owned())
                            .or_default()
                            .push(value.to_owned());
                    }
                }
            }
        }
        let conflicts = values
            .into_iter()
            .filter(|(_, values)| values.windows(2).any(|pair| pair[0] != pair[1]))
            .map(|(key, values)| InstructionConflict {
                key,
                effective_value: values.last().cloned().unwrap_or_default(),
                values,
            })
            .collect();
        EffectiveInstructions { sources, conflicts }
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(&mut output, "{byte:02x}").expect("writing to String is infallible");
            output
        })
}

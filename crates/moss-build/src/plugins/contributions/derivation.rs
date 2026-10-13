//! `contributes.derivations` — a plugin's declaration that it can turn one
//! kind of file into embeddable fragments by running a pinned one-shot binary.
//!
//! Core knows nothing about the format: it matches a file's extension against
//! `accepts`, runs the binary the declaration pins, and files the fragments
//! the binary prints under the names the declaration lists. The declaration
//! itself is the consent surface — the binary is pinned by hash like a stack's
//! artifact, so there is no JavaScript call that would need a separate grant.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::stack::{StackContribution, StackHooks, StackSource};

/// What a fragment's text is. Only markdown exists; an unknown value is
/// refused at parse time rather than guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FragmentType {
    Markdown,
}

/// The words for counting this kind of file in a sentence ("3 docs").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivationNoun {
    pub one: String,
    pub other: String,
}

/// The binary that does the work. `binary` names it for people and, for a
/// `download` source that names no `executable`, is where inside the unpacked
/// artifact it lives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivationRun {
    pub binary: String,
    pub artifact: Vec<StackSource>,
}

/// One derivation: a file type a plugin knows how to unpack into fragments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivationContribution {
    /// Lowercase file extensions without the dot.
    pub accepts: Vec<String>,
    pub noun: DerivationNoun,
    /// The fragments the binary may produce, by name.
    pub fragments: BTreeMap<String, FragmentType>,
    /// The lines of the placeholder page shown for a file that has no result
    /// yet: each is empty or `#<fragment name>`, standing for that fragment.
    #[serde(default)]
    pub stub: Vec<String>,
    pub run: DerivationRun,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DerivationDeclarationError {
    BadId(String),
    NoAccepts { id: String },
    BadExtension { id: String, ext: String },
    NoFragments { id: String },
    BadFragmentName { id: String, name: String },
    BadStubLine { id: String, line: String },
    BadBinary { id: String, binary: String },
    NoSources { id: String },
    MissingSha256 { id: String, url: String },
    EmptyNoun { id: String },
}

impl std::fmt::Display for DerivationDeclarationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use DerivationDeclarationError::*;
        match self {
            BadId(id) => write!(f, "derivation id `{id}` must be non-empty lowercase letters, digits and `-`"),
            NoAccepts { id } => write!(f, "derivation `{id}` accepts no file extension"),
            BadExtension { id, ext } => write!(
                f,
                "derivation `{id}` accepts `{ext}`, which is not a lowercase extension without a dot"
            ),
            NoFragments { id } => write!(f, "derivation `{id}` declares no fragments"),
            BadFragmentName { id, name } => write!(
                f,
                "derivation `{id}` names a fragment `{name}` outside [a-z][a-z0-9-]*"
            ),
            BadStubLine { id, line } => write!(
                f,
                "derivation `{id}` has a stub line `{line}` that is neither empty nor `#<declared fragment>`"
            ),
            BadBinary { id, binary } => write!(
                f,
                "derivation `{id}` names its binary `{binary}`; it must be a plain file name"
            ),
            NoSources { id } => write!(f, "derivation `{id}` declares no artifact sources"),
            MissingSha256 { id, url } => write!(
                f,
                "derivation `{id}` declares a download source with no sha256 ({url}) — a download source must be hashed"
            ),
            EmptyNoun { id } => write!(f, "derivation `{id}` needs a non-empty noun in both forms"),
        }
    }
}

fn is_slug(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_fragment_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_extension(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

impl DerivationContribution {
    /// Refuse a declaration moss could not run safely or render sensibly.
    /// `id` is the key it is declared under.
    pub fn validate(&self, id: &str) -> Result<(), DerivationDeclarationError> {
        use DerivationDeclarationError::*;
        let owned = || id.to_string();
        if !is_slug(id) {
            return Err(BadId(owned()));
        }
        if self.accepts.is_empty() {
            return Err(NoAccepts { id: owned() });
        }
        if let Some(ext) = self.accepts.iter().find(|e| !is_extension(e)) {
            return Err(BadExtension { id: owned(), ext: ext.clone() });
        }
        if self.noun.one.trim().is_empty() || self.noun.other.trim().is_empty() {
            return Err(EmptyNoun { id: owned() });
        }
        if self.fragments.is_empty() {
            return Err(NoFragments { id: owned() });
        }
        if let Some(name) = self.fragments.keys().find(|n| !is_fragment_name(n)) {
            return Err(BadFragmentName { id: owned(), name: name.clone() });
        }
        for line in &self.stub {
            let ok = line.is_empty()
                || line.strip_prefix('#').is_some_and(|name| self.fragments.contains_key(name));
            if !ok {
                return Err(BadStubLine { id: owned(), line: line.clone() });
            }
        }
        let binary = &self.run.binary;
        if binary.is_empty() || binary.starts_with('.') || binary.contains(['/', '\\']) {
            return Err(BadBinary { id: owned(), binary: binary.clone() });
        }
        if self.run.artifact.is_empty() {
            return Err(NoSources { id: owned() });
        }
        for source in &self.run.artifact {
            if let StackSource::Download { sha256: None, url, .. } = source {
                return Err(MissingSha256 { id: owned(), url: url.clone() });
            }
        }
        Ok(())
    }

    /// The same pin read as a stack, so the stack executor's `obtain`,
    /// `stage` and `binary_path` resolve and unpack the binary unchanged. A
    /// download that names no `executable` gets `run.binary` as its own.
    pub fn as_stack(&self, id: &str, version: &str) -> StackContribution {
        let sources = self
            .run
            .artifact
            .iter()
            .cloned()
            .map(|source| match source {
                StackSource::Download { platform, url, sha256, archive_format, executable } => {
                    StackSource::Download {
                        platform,
                        url,
                        sha256,
                        archive_format,
                        executable: executable.or_else(|| Some(self.run.binary.clone())),
                    }
                }
                path @ StackSource::Path { .. } => path,
            })
            .collect();
        StackContribution {
            id: id.to_string(),
            display_name: None,
            version: version.to_string(),
            sources,
            start: vec![],
            stop: vec![],
            uninstall: vec![],
            recover: vec![],
            self_heal_grace_secs: None,
            preserve_on_uninstall: vec![],
            hooks: StackHooks::default(),
        }
    }
}

#[cfg(test)]
#[path = "derivation_tests.rs"]
mod derivation_tests;

//! The setup contract's field vocabulary: what one declared setting *is*
//! ([`Field`]), the closed [`Need`] vocabulary, and the parse-time closure
//! over a declared list ([`validate_declared_fields`]).
//!
//! Split from `contributions.rs`, which keeps the contribution kinds and the
//! legacy folds: this file is the vocabulary a plugin author reads, that one
//! is how moss normalizes a manifest into it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One precondition the host can evaluate with no plugin code running.
///
/// A closed vocabulary that grows only when moss can evaluate the new
/// predicate itself. A need is not a grant: `binary:git` says the publish
/// cannot proceed without git, while `execute_binary:git` in `requires` says
/// the sandbox may run it — the IPFS plugin is granted Kubo for its local path
/// and needs no binary at all with `provider = pinata`, which is why the two
/// are never derived from each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// moss's Docker stack is running. The host remedy is moss's own stack
    /// start, which a plugin-reported error never could wire to.
    Stack,
    /// A resolvable executable with this basename.
    Binary(String),
}

impl Serialize for Need {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Need::Stack => s.serialize_str("stack"),
            Need::Binary(name) => s.serialize_str(&format!("binary:{name}")),
        }
    }
}

impl<'de> Deserialize<'de> for Need {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        if raw == "stack" {
            return Ok(Need::Stack);
        }
        if let Some(name) = raw.strip_prefix("binary:") {
            if !name.is_empty() {
                return Ok(Need::Binary(name.to_string()));
            }
        }
        Err(serde::de::Error::custom(format!(
            "unknown need `{raw}` — the closed vocabulary is `stack` and `binary:<name>`"
        )))
    }
}

/// The closed set of field types. Four, not five: there is no `login` type —
/// a sign-in is a `check_setup` blocker with a zero-field form, because auth
/// is a flow, not a settings entry.
// specta renames: the generated bindings share one namespace with the
// frontmatter schema's `Field`/`FieldType` (moss-core), so these export
// under a `Setting` prefix there while the Rust names stay the contract's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename = "SettingFieldType")]
pub enum FieldType {
    /// Free text, or a closed select when `options` is present.
    String,
    Number,
    Boolean,
    /// Lives in moss's keystore, drawn as a password field, filtered out of
    /// every serialized config. `secret` rather than `password`, deliberately:
    /// `password` names a widget, this names custody.
    Secret,
}

/// One choice of a closed select. Per-choice `label` and `description` sit
/// beside the value — never in parallel maps keyed `"<field>.<value>"`, which
/// is the shape this replaced.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[specta(rename = "SettingFieldOption")]
pub struct FieldOption {
    /// The stored value.
    pub value: String,
    /// What the choice is called in the select.
    pub label: String,
    /// One sentence about the choice, when the label alone is not enough.
    #[serde(default)]
    pub description: Option<String>,
}

/// One field moss draws — in the settings form, and in a blocker's form. The
/// vocabulary is Home Assistant's and JSON Schema's almost verbatim, so a
/// plugin author arrives already knowing it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[specta(rename = "SettingField")]
pub struct Field {
    /// The storage and read-back name (`moss.getSecret(key)`, the config key).
    pub key: String,

    /// Which of the closed set of four this is.
    #[serde(rename = "type")]
    pub field_type: FieldType,

    /// What moss's form calls it. Filled from the title-cased key when the
    /// manifest says nothing, so every drawn field has a name.
    #[serde(default)]
    pub label: String,

    /// One sentence under the field.
    #[serde(default)]
    pub description: Option<String>,

    /// A heading the settings form draws above the first field of each run
    /// of consecutive fields sharing this value — manifest order carries the
    /// grouping.
    #[serde(default)]
    pub group: Option<String>,

    /// Pre-filled value. Refused on a `secret` (a default credential is a
    /// placeholder nobody replaces or a token a registry serves to everyone)
    /// and refused beside `required` (a defaulted field can never be missing).
    #[serde(default)]
    pub default: Option<serde_json::Value>,

    /// Present on a `string` field, it closes the value set: moss draws a
    /// select, and a `when` clause may reference this field.
    // No `skip_serializing_if`: the renderer receives this struct over the
    // bindings seam, where an omitted key and the type saying `options:
    // SettingFieldOption[]` would disagree.
    #[serde(default)]
    pub options: Vec<FieldOption>,

    /// Publish blocks while the value is blank, and moss draws the ask.
    /// Secrets do not take it — an applicable secret is required by
    /// definition.
    #[serde(default)]
    pub required: bool,

    /// Declared but never drawn in the settings form. Still a real config
    /// key: stepped flows write it, and `when` may reference it if it carries
    /// `options`. A hidden `secret` is how a plugin declares custody of a
    /// credential it never asks for — the token a sign-in flow earned.
    #[serde(default)]
    pub hidden: bool,

    /// Ghost text in the empty input.
    #[serde(default)]
    pub placeholder: Option<String>,

    /// Where the user gets one. Rendered as a link beside the field; a
    /// credential prompt with no way to obtain the credential is a dead end.
    #[serde(default)]
    pub help_url: Option<String>,

    /// Local validation, string fields only — checked per keystroke, blocking
    /// save. Requires `pattern_message`, because a bare regex is a hostile
    /// error message.
    #[serde(default)]
    pub pattern: Option<String>,

    /// What to say when `pattern` does not match.
    #[serde(default)]
    pub pattern_message: Option<String>,

    /// An AND over `{ key: value }` pairs, values strings only. Closed at
    /// parse time rather than defended at runtime: it may reference only
    /// fields declared earlier in the list, the referenced field must carry
    /// `options`, and every value must be one of them — see
    /// [`validate_declared_fields`]. There is no negation (options are closed,
    /// so write the complement) and no any-of list (write two fields, or the
    /// complement).
    #[serde(default, deserialize_with = "when_strings")]
    pub when: HashMap<String, String>,
}

/// Refuse a non-string `when` value with the reason, not a serde type dump.
///
/// The list form ("any of these") is deliberately unrepresentable: the
/// scalar-only comparison before it had Ansible's `required_if` bug, where a
/// list on the declared side compares unequal to every scalar and the rule is
/// quietly inert. Making the value a string at parse means that silent-false
/// cannot exist at all.
fn when_strings<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<HashMap<String, String>, D::Error> {
    let raw: HashMap<String, serde_json::Value> = Deserialize::deserialize(d)?;
    raw.into_iter()
        .map(|(k, v)| match v {
            serde_json::Value::String(s) => Ok((k, s)),
            other => Err(serde::de::Error::custom(format!(
                "`when.{k}` must be a single string, got {other} — there is no any-of \
                 list; options are closed, so write the complement instead"
            ))),
        })
        .collect()
}

impl Field {
    /// Does this field apply, given the plugin's resolved settings?
    ///
    /// A pair matches when the resolved value renders to the declared string
    /// (`"true"` matches a boolean `true`). A key the config has no value for
    /// **counts as matching**, so the field stays applicable — the fail-closed
    /// direction: an extra prompt is recoverable, a publish that quietly
    /// skipped a token is not. Parse closure means this branch is only
    /// reachable for a folded legacy manifest whose `when` predates the rules.
    pub fn when_matches(&self, config: &HashMap<String, serde_json::Value>) -> bool {
        self.when.iter().all(|(k, want)| {
            config.get(k).is_none_or(|got| match got.as_str() {
                Some(text) => text == want,
                None => got.to_string() == *want,
            })
        })
    }

    /// A `secret` field synthesized from a legacy declaration surface.
    pub(crate) fn legacy_secret(
        key: String,
        label: String,
        help_url: Option<String>,
        when: HashMap<String, String>,
    ) -> Self {
        Field {
            key,
            field_type: FieldType::Secret,
            label,
            description: None,
            group: None,
            default: None,
            options: Vec::new(),
            required: false,
            hidden: false,
            placeholder: None,
            help_url,
            pattern: None,
            pattern_message: None,
            when,
        }
    }
}

/// One credential under the pre-contract `setup.credentials` list. Parse-only:
/// [`crate::plugins::types::PluginManifest::parse`] folds each entry into a
/// `secret` [`Field`].
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CredentialNeed {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub help_url: Option<String>,
    /// String values only — the list form this once accepted is refused with
    /// the same named error the `settings` surface gives (see [`when_strings`]).
    #[serde(default, deserialize_with = "when_strings")]
    pub when: HashMap<String, String>,

    /// A credential may not carry a `default`, and saying so costs a parse
    /// error rather than a shipped secret. Raycast's schema refuses it on a
    /// `password` preference for the same reason: a default credential is
    /// either a placeholder the user never replaces or a real token baked
    /// into a manifest that a registry serves to everyone.
    ///
    /// Only this one key is refused. `deny_unknown_fields` would refuse it
    /// too, and would also make every future key a parse failure on the moss
    /// that predates it.
    /// Never read — it exists only so serde has a field to refuse the key on.
    #[allow(dead_code)]
    #[serde(default, rename = "default", deserialize_with = "refuse_a_default")]
    no_default: (),
}

/// Fails whenever the key is present at all — see `CredentialNeed`'s `default` guard.
fn refuse_a_default<'de, D: serde::Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(serde::de::Error::custom(
        "a credential must not declare a `default` — moss's modal collects it, \
         and a default is either a placeholder or a token shipped to everyone",
    ))
}

/// The parse-time closure over a declared `settings` list. Every rule here is
/// a named error a plugin author reads at parse, instead of a runtime
/// fail-open a user pays for — the two live fail-opens the old
/// `required_credentials` carried (an absent `when` key silently un-requiring
/// a credential, and a scalar comparison that never matched a list) cannot
/// exist under it.
///
/// Bound and strip every string on a field that the PLUGIN AUTHOR wrote.
///
/// These land in moss's own form — a label beside moss's asterisk, a sentence
/// under moss's input, an error where moss's validation speaks. A direction
/// override inside any of them reverses the sentence moss built around it, and
/// an unbounded one lays out a form that has no column to push back. `key` is
/// deliberately absent: it is a config identifier, never drawn as prose, and
/// [`validate_declared_fields`] already closes the set it may belong to.
///
/// Applied to the folded legacy fields too, unlike the validation above. That
/// asymmetry is the point — leniency there is about not unloading a rule an
/// installed manifest never had, and no manifest ever had permission to
/// reverse moss's sentences.
fn bound_author_text(field: &mut Field) {
    use moss_core::untrusted_text::{bounded, MAX_NAME, MAX_SENTENCE};
    field.label = bounded(&field.label, MAX_NAME);
    for text in [&mut field.description, &mut field.pattern_message]
        .into_iter()
        .flatten()
    {
        *text = bounded(text, MAX_SENTENCE);
    }
    if let Some(v) = &mut field.placeholder {
        *v = bounded(v, MAX_NAME);
    }
    for option in &mut field.options {
        option.label = bounded(&option.label, MAX_NAME);
        if let Some(v) = &mut option.description {
            *v = bounded(v, MAX_SENTENCE);
        }
    }
}

/// Every author-authored string on a setup's fields, bounded in one pass.
pub(crate) fn bound_author_text_in(fields: &mut [Field]) {
    for field in fields {
        bound_author_text(field);
    }
}

/// Runs on the natively-declared list only. Fields folded from the legacy
/// surfaces stay lenient — a rule an installed manifest never had must not
/// unload it — and their `when` falls back to [`Field::when_matches`]'s
/// fail-closed runtime answer.
pub(crate) fn validate_declared_fields(fields: &[Field]) -> Result<(), String> {
    for (index, field) in fields.iter().enumerate() {
        let key = &field.key;
        if fields[..index].iter().any(|earlier| earlier.key == *key) {
            return Err(format!(
                "field `{key}` is declared twice in `settings` — two drawn fields \
                 would write one config key"
            ));
        }
        if field.field_type == FieldType::Secret && field.default.is_some() {
            return Err(format!(
                "field `{key}`: a `secret` must not declare a `default` — a default \
                 credential is a placeholder nobody replaces or a token shipped to everyone"
            ));
        }
        if field.field_type == FieldType::Secret && field.required {
            return Err(format!(
                "field `{key}`: a `secret` does not take `required` — an applicable \
                 secret is required by definition"
            ));
        }
        if field.required && field.default.is_some() {
            return Err(format!(
                "field `{key}` declares both `required` and `default` — a defaulted \
                 field can never be missing, so declare one or the other"
            ));
        }
        if field.pattern.is_some() && field.pattern_message.is_none() {
            return Err(format!(
                "field `{key}` has a `pattern` without a `pattern_message` — a bare \
                 regex is a hostile error message"
            ));
        }
        if !field.options.is_empty() && field.field_type != FieldType::String {
            return Err(format!(
                "field `{key}`: `options` close the value set of a `string` field, \
                 not a `{:?}`",
                field.field_type
            ));
        }
        for (target, want) in &field.when {
            let Some(referenced) =
                fields[..index].iter().find(|earlier| &earlier.key == target)
            else {
                return Err(format!(
                    "field `{key}`: `when` references `{target}`, which is not declared \
                     earlier in `settings` — a `when` may only look back, which is what \
                     makes cycles unrepresentable"
                ));
            };
            if referenced.options.is_empty() {
                return Err(format!(
                    "field `{key}`: `when` references `{target}`, which has no `options` \
                     — a `when` target must be a closed select"
                ));
            }
            if !referenced.options.iter().any(|o| &o.value == want) {
                return Err(format!(
                    "field `{key}`: `when.{target}` is `{want}`, which is not one of \
                     `{target}`'s options"
                ));
            }
        }
    }
    Ok(())
}

/// `snake_case` → `Title Case`, for a field the manifest gave no label.
pub(crate) fn title_case(key: &str) -> String {
    key.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

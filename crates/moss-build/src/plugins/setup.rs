//! The `check_setup` verdict protocol (plugin setup contract).
//!
//! Two verdicts, no third: `ready`, or `blocked` with blockers. A blocker may
//! carry a form in the same [`Field`] vocabulary the settings page draws,
//! which is how multi-step flows happen — claim a name, sign in, verify an
//! email. Every invocation is cold: the hook re-derives the current step from
//! durable state, so a flow survives an app restart half-way through.
//!
//! The pre-contract wire shape (`{ready, needs: [{id, message, actions}]}`)
//! normalizes on deserialize: each action becomes a blocker whose id is the
//! action's (the routing verb the hook expects back) and whose form is the
//! zero-field button, so the renderer has exactly one shape to draw.

use super::contributions::Field;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What `check_setup` receives. `settings` is the contract's name for the
/// resolved plain fields (manifest defaults overlaid with persisted values);
/// `config` carries the same map for plugins written before the rename.
///
/// The first call arrives with no `action`; when the user submits a blocker's
/// form, the same hook runs again with that blocker's id and the submitted
/// values, and answers with the next state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupContext {
    /// Absolute path to the project folder about to be published
    pub project_path: String,

    /// Resolved plain settings, filled by the dispatcher (no caller knows
    /// which plugin will run).
    #[serde(default)]
    pub settings: HashMap<String, serde_json::Value>,

    /// Deprecated alias of `settings` — same map, pre-contract name.
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,

    /// The id of the blocker whose form the user submitted; absent on the
    /// first call.
    #[serde(default)]
    pub action: Option<String>,

    /// The submitted form values riding with `action`. Empty on the first
    /// call and for zero-field forms.
    #[serde(default)]
    pub values: HashMap<String, serde_json::Value>,
}

/// A blocker's form: same field vocabulary as declared settings, same
/// renderer. `default` on a form field carries a suggestion; `when` is not
/// honored here — the steps are the conditionality.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SetupForm {
    /// A zero-field form is a button.
    #[serde(default)]
    pub fields: Vec<Field>,
    /// Submit button label.
    pub submit: String,
}

/// One reason the contribution is not ready, and what to do about it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SetupBlocker {
    /// The routing verb: submitted back as `SetupContext::action`. Naming
    /// blockers is API design — the next step's `action` is always the id of
    /// a blocker the hook itself returned. Ids beginning `moss:` are the
    /// host's; a plugin blocker never carries one and the host's own (a
    /// failed manifest `need`) render through this same shape.
    pub id: String,
    /// What the person reads before acting. A side effect's consequence
    /// ("keeps running after moss quits") goes here, not in a second slot.
    #[serde(default)]
    pub message: String,
    /// Present when there is something to submit; absent when the message is
    /// the whole answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<SetupForm>,
    /// Per-field errors on a re-shown step. Their presence marks the verdict
    /// a RE-ASK of this blocker, the one answering shape moss does not
    /// persist submitted values on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_errors: Option<HashMap<String, String>>,
}

/// The `check_setup` answer. A hook that cannot tell — a timeout, an
/// unreachable host — answers `blocked` with a message that says so and a
/// zero-field retry form; there is deliberately no third verdict.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum SetupVerdict {
    Ready,
    Blocked {
        #[serde(default)]
        blockers: Vec<SetupBlocker>,
    },
}

impl SetupVerdict {
    pub fn is_ready(&self) -> bool {
        matches!(self, SetupVerdict::Ready)
    }

    /// Did this verdict re-ask the blocker the user just answered, with field
    /// errors? Moss persists submitted values on every answering verdict
    /// EXCEPT this one — the values were judged wrong.
    pub fn reasks_with_field_errors(&self, action: &str) -> bool {
        match self {
            SetupVerdict::Ready => false,
            SetupVerdict::Blocked { blockers } => blockers.iter().any(|b| {
                b.id == action && b.field_errors.as_ref().is_some_and(|e| !e.is_empty())
            }),
        }
    }
}

/// The two shapes a hook may answer with. Disjoint keys (`status` vs `ready`),
/// so untagged resolution cannot misfile one as the other.
#[derive(Deserialize)]
#[serde(untagged)]
enum WireVerdict {
    New(NewVerdict),
    Legacy(LegacyVerdict),
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum NewVerdict {
    Ready,
    Blocked {
        #[serde(default)]
        blockers: Vec<SetupBlocker>,
    },
}

/// The pre-contract wire shape. Parsed only — never constructed, never
/// serialized — so first-party hooks written before the contract keep
/// answering while they migrate.
#[derive(Deserialize)]
struct LegacyVerdict {
    ready: bool,
    #[serde(default)]
    needs: Vec<LegacyNeed>,
}

#[derive(Deserialize)]
struct LegacyNeed {
    id: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    actions: Vec<LegacyAction>,
}

#[derive(Deserialize)]
struct LegacyAction {
    id: String,
    label: String,
    #[serde(default)]
    consent: Option<String>,
}

impl<'de> Deserialize<'de> for SetupVerdict {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut verdict = match WireVerdict::deserialize(d)? {
            WireVerdict::New(NewVerdict::Ready) => SetupVerdict::Ready,
            WireVerdict::New(NewVerdict::Blocked { blockers }) => {
                SetupVerdict::Blocked { blockers }
            }
            WireVerdict::Legacy(legacy) => legacy.into(),
        };
        // Both wire shapes converge here, which is why the bound sits after
        // the match rather than in either arm: a verdict is a plugin's answer
        // arriving at runtime, so unlike a manifest it never passed the parse
        // in `contributions.rs`, and the legacy path even BUILDS a message by
        // interpolating one author string into another.
        if let SetupVerdict::Blocked { blockers } = &mut verdict {
            for blocker in blockers.iter_mut() {
                bound_blocker_text(blocker);
            }
        }
        Ok(verdict)
    }
}

/// Everything on a blocker that the plugin wrote, bounded and stripped.
///
/// `id` is exempt: it is a routing verb submitted back as `action`, never
/// drawn. The field-error map's KEYS are exempt for the same reason — they
/// name a field — while its values are a sentence beside moss's input.
fn bound_blocker_text(blocker: &mut SetupBlocker) {
    use moss_core::untrusted_text::{bounded, MAX_NAME, MAX_SENTENCE};
    blocker.message = bounded(&blocker.message, MAX_SENTENCE);
    if let Some(form) = &mut blocker.form {
        form.submit = bounded(&form.submit, MAX_NAME);
        crate::plugins::contributions::settings::bound_author_text_in(&mut form.fields);
    }
    if let Some(errors) = &mut blocker.field_errors {
        for message in errors.values_mut() {
            *message = bounded(message, MAX_SENTENCE);
        }
    }
}

impl From<LegacyVerdict> for SetupVerdict {
    fn from(legacy: LegacyVerdict) -> Self {
        if legacy.ready {
            return SetupVerdict::Ready;
        }
        let blockers = legacy
            .needs
            .into_iter()
            .flat_map(|need| {
                if need.actions.is_empty() {
                    // Nothing to click: the message is the whole answer.
                    return vec![SetupBlocker {
                        id: need.id,
                        message: need.message,
                        form: None,
                        field_errors: None,
                    }];
                }
                need.actions
                    .into_iter()
                    .map(|action| SetupBlocker {
                        id: action.id,
                        // The contract folded the consent string into the
                        // blocker's message — it is what the person reads
                        // before pressing the button.
                        message: match action.consent {
                            Some(consent) if !consent.is_empty() => {
                                format!("{} {consent}", need.message)
                            }
                            _ => need.message.clone(),
                        },
                        form: Some(SetupForm { fields: vec![], submit: action.label }),
                        field_errors: None,
                    })
                    .collect()
            })
            .collect();
        SetupVerdict::Blocked { blockers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> SetupVerdict {
        serde_json::from_str(json).expect("verdict must parse")
    }

    /// A verdict is the plugin talking at RUNTIME, so nothing it carries has
    /// been near the manifest parse. Both wire shapes are asserted because the
    /// bound sits after the match that unifies them — and the legacy path is
    /// the sharper case, since it BUILDS the message by interpolating the
    /// consent string into the need's, so an override in either half would
    /// reverse the other.
    #[test]
    fn every_word_a_verdict_carries_is_bounded_and_stripped() {
        use moss_core::untrusted_text::{MAX_NAME, MAX_SENTENCE};
        let long = "x".repeat(MAX_SENTENCE + 40);

        let new_shape = parse(&format!(
            r#"{{"status":"blocked","blockers":[{{"id":"go","message":"m\u202E{long}",
                 "form":{{"submit":"s\u202E{}","fields":[]}},
                 "field_errors":{{"tok":"bad\u202E{long}"}}}}]}}"#,
            "y".repeat(MAX_NAME + 40)
        ));
        let b = &blockers(&new_shape)[0];
        assert_eq!(b.id, "go", "the routing verb is untouched — it is never drawn");
        assert!(!b.message.contains('\u{202E}'));
        assert_eq!(b.message.chars().count(), MAX_SENTENCE + 1);
        let form = b.form.as_ref().unwrap();
        assert!(!form.submit.contains('\u{202E}'));
        assert_eq!(form.submit.chars().count(), MAX_NAME + 1);
        let err = &b.field_errors.as_ref().unwrap()["tok"];
        assert!(!err.contains('\u{202E}'), "a field error speaks beside moss's input");
        assert_eq!(err.chars().count(), MAX_SENTENCE + 1);

        let legacy = parse(
            r#"{"ready":false,"needs":[{"id":"n","message":"need\u202Ey",
                "actions":[{"id":"a","label":"go\u202Ey","consent":"and\u202Ey"}]}]}"#,
        );
        let b = &blockers(&legacy)[0];
        assert!(!b.message.contains('\u{202E}'), "the folded consent is stripped too");
        assert!(!b.form.as_ref().unwrap().submit.contains('\u{202E}'));
    }

    fn blockers(v: &SetupVerdict) -> &[SetupBlocker] {
        match v {
            SetupVerdict::Blocked { blockers } => blockers,
            SetupVerdict::Ready => panic!("expected blocked"),
        }
    }

    #[test]
    fn the_contract_shapes_parse_as_themselves() {
        assert!(parse(r#"{"status":"ready"}"#).is_ready());
        let v = parse(
            r#"{"status":"blocked","blockers":[{"id":"claim_name",
                "form":{"fields":[{"key":"onion_name","type":"string",
                  "label":"Site name","description":"The name."}],
                "submit":"Claim name"}}]}"#,
        );
        let b = blockers(&v);
        assert_eq!(b[0].id, "claim_name");
        assert_eq!(b[0].message, "", "an omitted message is empty, not an error");
        let form = b[0].form.as_ref().unwrap();
        assert_eq!(form.submit, "Claim name");
        assert_eq!(form.fields[0].key, "onion_name");
    }

    /// The legacy github shape: one need, one action. The action's id becomes
    /// the blocker's — it is the routing verb `ctx.action` carries back — and
    /// the action's label becomes the zero-field form's submit.
    #[test]
    fn a_legacy_need_with_an_action_becomes_a_blocker_with_a_button() {
        let v = parse(
            r#"{"ready":false,"needs":[{"id":"github_account",
                "message":"Sign in.","actions":[{"id":"sign_in","label":"Sign in to GitHub"}]}]}"#,
        );
        let b = blockers(&v);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].id, "sign_in");
        assert_eq!(b[0].message, "Sign in.");
        let form = b[0].form.as_ref().unwrap();
        assert!(form.fields.is_empty());
        assert_eq!(form.submit, "Sign in to GitHub");
    }

    #[test]
    fn a_legacy_need_without_actions_becomes_a_formless_blocker() {
        let v = parse(
            r#"{"ready":false,"needs":[{"id":"net","message":"Could not reach GitHub."}]}"#,
        );
        let b = blockers(&v);
        assert_eq!(b[0].id, "net");
        assert!(b[0].form.is_none());
    }

    #[test]
    fn legacy_ready_and_consent_both_normalize() {
        assert!(parse(r#"{"ready":true}"#).is_ready());
        let v = parse(
            r#"{"ready":false,"needs":[{"id":"daemon","message":"Daemon is down.",
                "actions":[{"id":"start","label":"Start",
                  "consent":"Keeps running after moss quits."}]}]}"#,
        );
        assert_eq!(
            blockers(&v)[0].message,
            "Daemon is down. Keeps running after moss quits.",
            "the consent sentence folds into what the person reads before the click"
        );
    }

    /// Serialization is always the contract shape — the legacy form is
    /// parse-only, so what reaches the renderer has exactly one grammar.
    #[test]
    fn a_normalized_verdict_serializes_as_the_contract_shape() {
        let v = parse(r#"{"ready":false,"needs":[{"id":"x","message":"m"}]}"#);
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["status"], "blocked");
        assert_eq!(json["blockers"][0]["id"], "x");
        assert!(json.get("ready").is_none());
    }

    #[test]
    fn a_field_errors_reask_of_the_submitted_blocker_is_recognized() {
        let v = parse(
            r#"{"status":"blocked","blockers":[{"id":"claim_name",
                "field_errors":{"onion_name":"Taken."}}]}"#,
        );
        assert!(v.reasks_with_field_errors("claim_name"));
        assert!(!v.reasks_with_field_errors("other_step"), "a different blocker is a next step");
        assert!(!parse(r#"{"status":"ready"}"#).reasks_with_field_errors("claim_name"));

        // A re-sent blocker WITHOUT field errors is an answering verdict — a
        // form-level error in `message` still persists what was submitted.
        let v = parse(
            r#"{"status":"blocked","blockers":[{"id":"claim_name","message":"Try again."}]}"#,
        );
        assert!(!v.reasks_with_field_errors("claim_name"));
    }
}

use super::*;

fn registry() -> Arc<TaskRegistry> {
    Arc::new(TaskRegistry::new())
}

fn win() -> WindowId {
    WindowId::from("main")
}

// ── Verb + Amount value objects (Step 3 R2) ───────────────────────

#[test]
fn verb_normalizes_plugin_supplied_words() {
    // moss owns presentation: a plugin proposes only the word (R13).
    assert_eq!(Verb::normalized("  syndicated ").0, "Syndicated"); // trim + capitalize
    assert_eq!(Verb::normalized("🚀 shipped").0, "Shipped"); // emoji strip
                                                             // R13: leading AND trailing punctuation/glyphs are moss's, not the
                                                             // plugin's — a "🚀 syndicated!!!" spoof becomes the plain word.
    assert_eq!(Verb::normalized("🚀 syndicated!!!").0, "Syndicated");
    // An end-trim reaches an override only at an edge. This one sits between
    // two letters, and a verb is drawn inside moss's own receipt line, so it
    // reversed moss's words rather than the plugin's.
    assert_eq!(Verb::normalized("Pub\u{202E}lished").0, "Published");
    assert_eq!(
        Verb::normalized("aVeryLongVerbThatExceedsTheClampLimit")
            .0
            .chars()
            .count(),
        24
    ); // clamp (char count, not byte len)
    assert_eq!(Verb::core(Verb::PUBLISHED).0, "Published");
    assert_eq!(Verb::core(Verb::UPLOADED).0, "Uploaded");
}

#[test]
fn amount_is_a_value_not_a_string() {
    let a = Amount {
        count: 142,
        noun: "pages".into(),
    };
    let json = serde_json::to_value(&a).unwrap();
    assert_eq!(json["count"], 142);
    assert_eq!(json["noun"], "pages");
}

// ── tone is computed from job nature, not stored (Step 3 Phase 6 R5) ──

#[test]
fn tone_is_computed_from_job_nature() {
    let reg = registry();

    // A Build media child (parent set) → Ambient. Child Jobs live and die
    // at the hairline; success makes no sound (design §"Ambient").
    let parent = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Build,
        TaskTone::Ambient,
    );
    let child = reg.spawn_child(
        win(),
        TaskScope::Preview,
        TaskKind::AssetTransform,
        TaskTone::Ambient,
        parent.id,
    );
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let child_task = tasks.iter().find(|t| t.id == child.id).unwrap();
    assert_eq!(
        child_task.tone(),
        TaskTone::Ambient,
        "a media child Job (parent set) is Ambient",
    );

    // An Awaiting Job → Awaiting (the one interrupt — needs the user's
    // action). State-driven: the moment a Job enters Awaiting, the wire
    // tone flips so the awaiting renderer owns it (inline-status drops its
    // badge on the same flip).
    let deploy = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Awaiting,
    );
    deploy.awaiting("verify your email", Action::None);
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let deploy_task = tasks.iter().find(|t| t.id == deploy.id).unwrap();
    assert_eq!(
        deploy_task.tone(),
        TaskTone::Awaiting,
        "a Job in the Awaiting state is Awaiting-toned",
    );

    // A deploy (Preview, Deploy) Running → Inline (anchored adjacent to the
    // publish surface), matching the deploy.rs producer + the ADR table.
    let publish = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Inline,
    );
    publish.progress(Some(0.5), Some("Publishing".into()));
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let publish_task = tasks.iter().find(|t| t.id == publish.id).unwrap();
    assert_eq!(
        publish_task.tone(),
        TaskTone::Inline,
        "a running deploy is Inline",
    );

    // A Save (ActionPanel, Save) Running → Inline (matches the editor save
    // producer + panel_task_wire_strips_instant_and_preserves_state).
    let save = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    let save_task = tasks.iter().find(|t| t.id == save.id).unwrap();
    assert_eq!(save_task.tone(), TaskTone::Inline, "a Save is Inline");

    // A plugin Import routed to (ActionPanel, Import) Running → Ambient
    // (matches the route_plugin_task OnboardingFlow row + the runtime
    // lifecycle test).
    let import = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Import,
        TaskTone::Ambient,
    );
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    let import_task = tasks.iter().find(|t| t.id == import.id).unwrap();
    assert_eq!(
        import_task.tone(),
        TaskTone::Ambient,
        "an Import is Ambient",
    );
}

// ── §4 terminal-state smart constructors (Step 3 #1,#2,#3,#6) ──────

fn blocking_advisory() -> crate::advisory::Advisory {
    crate::advisory::Advisory {
        scope: crate::advisory::Scope::Account,
        severity: crate::advisory::Severity::Blocking,
        item: None,
        what: "Trial expired".into(),
        action: crate::advisory::Action::InApp {
            op: crate::advisory::AppOp::OpenBilling,
            args: serde_json::Value::Null,
            label: "Subscribe".into(),
        },
    }
}

fn degraded_advisory() -> crate::advisory::Advisory {
    crate::advisory::Advisory {
        scope: crate::advisory::Scope::File,
        severity: crate::advisory::Severity::ShippedDegraded,
        item: Some("clip.mov".into()),
        what: "shipped unoptimized".into(),
        action: crate::advisory::Action::None,
    }
}

#[test]
fn done_with_blocking_advisory_is_failed_not_succeeded() {
    // Invariant #1: a "succeeded-but-blocking" Job is unrepresentable.
    let state = TaskState::done(Some("Published".into()), vec![blocking_advisory()]);
    match state {
        TaskState::Failed { advisory } => {
            assert!(matches!(
                advisory.severity,
                crate::advisory::Severity::Blocking
            ))
        }
        other => panic!("expected Failed with the blocking advisory, got {other:?}"),
    }
}

#[test]
fn done_with_degraded_advisory_is_still_success() {
    // Invariant #2: Done with non-empty advisories is still success.
    let state = TaskState::done(Some("Published".into()), vec![degraded_advisory()]);
    match state {
        TaskState::Succeeded { advisories, .. } => assert_eq!(advisories.len(), 1),
        other => panic!("expected Succeeded, got {other:?}"),
    }
}

#[test]
fn done_empty_is_non_surfacing_success() {
    // Invariant #6: Done{advisories: ∅} is non-surfacing success.
    let state = TaskState::done(None, vec![]);
    assert!(matches!(state, TaskState::Succeeded { ref advisories, .. } if advisories.is_empty()));
}

#[test]
fn failed_with_carries_the_root_cause() {
    // Invariant #3: Failed carries its root-cause advisory directly
    // (the collapsed `Failed { advisory }` shape — no Option).
    let state = TaskState::failed_with(blocking_advisory());
    match state {
        TaskState::Failed { advisory } => {
            assert_eq!(advisory.what, "Trial expired");
        }
        other => panic!("expected Failed carrying the advisory, got {other:?}"),
    }
}

#[test]
fn awaiting_is_not_terminal() {
    // Invariant #5 guard (R-c): an Awaiting Job is never terminal —
    // `is_terminal()` structurally excludes it. Pins the property so a
    // future enum edit can't silently make Awaiting absorbing.
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Awaiting,
    );
    h.awaiting("verify your email", Action::None);
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    assert!(!tasks[0].is_terminal());
}

// ── T5.1: Syndicate stays Inline (rail) when Awaiting ────────────────

#[test]
fn syndicate_awaiting_stays_inline_deploy_awaiting_stays_awaiting() {
    // A Syndicate job in Awaiting state must surface on its rail lamp
    // (Inline), never the window Awaiting thread — even while waiting for
    // the user in the matters room.  A Deploy job in Awaiting state must
    // still escalate to Awaiting (unchanged behaviour).
    let reg = registry();

    // Syndicate (ActionPanel scope, matching the OnboardingFlow route)
    let syn = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Syndicate,
        TaskTone::Inline,
    );
    syn.awaiting("publish the draft in Matters", Action::None);
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    let syn_task = tasks.iter().find(|t| t.id == syn.id).unwrap();
    assert_eq!(
        syn_task.tone(),
        TaskTone::Inline,
        "a Syndicate job in Awaiting must stay Inline (rail), not pop the Awaiting window",
    );

    // Deploy (Preview scope) — must still escalate to Awaiting
    let dep = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Awaiting,
    );
    dep.awaiting("verify your domain", Action::None);
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let dep_task = tasks.iter().find(|t| t.id == dep.id).unwrap();
    assert_eq!(
        dep_task.tone(),
        TaskTone::Awaiting,
        "a Deploy job in Awaiting must keep the Awaiting tone",
    );
}

// ── PanelTask Job fields: verb/amount/elapsed/parent (Step 3 R-a/R-b) ──

#[test]
fn panel_task_job_fields_default_none() {
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Build,
        TaskTone::Ambient,
    );
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let task = tasks.iter().find(|t| t.id == h.id).unwrap();
    assert!(task.verb.is_none());
    assert!(task.amount().is_none()); // #4: private, only done_with sets it
    assert!(task.elapsed().is_none());
    assert!(task.parent.is_none());
}

#[test]
fn done_with_sets_verb_amount_elapsed_on_success() {
    let reg = registry();
    let mut h = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Inline,
    );
    h.done_with(
        Verb::core(Verb::PUBLISHED),
        Some(Amount {
            count: 142,
            noun: "files".into(),
        }),
        None,
        vec![],
    );
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let task = tasks.iter().find(|t| t.id == h.id).unwrap();
    assert_eq!(task.verb, Some(Verb::core(Verb::PUBLISHED)));
    assert_eq!(
        task.amount(),
        Some(&Amount {
            count: 142,
            noun: "files".into()
        })
    );
    assert!(task.elapsed().is_some()); // frozen measured span (R-a)
    assert!(matches!(task.state, TaskState::Succeeded { .. }));
}

#[test]
fn blocking_downgrade_keeps_amount_none() {
    // C2 + #4: a Done-with-Blocking flips to Failed and must NOT stamp amount.
    let reg = registry();
    let mut h = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Inline,
    );
    h.done_with(
        Verb::core(Verb::PUBLISHED),
        Some(Amount {
            count: 142,
            noun: "files".into(),
        }),
        None,
        vec![blocking_advisory()],
    );
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let task = tasks.iter().find(|t| t.id == h.id).unwrap();
    assert!(
        task.amount().is_none(),
        "a Failed run must not carry an amount (#4/C2)"
    );
    assert!(matches!(task.state, TaskState::Failed { .. }));
}

// `done_with_elapsed_freezes_the_producer_supplied_span` and
// `done_with_elapsed_blocking_downgrade_keeps_amount_none` were removed with
// `TaskHandle::done_with_elapsed` (Phase-4 review FIX 4 — dead API). The
// equivalent registry-level coverage lives in
// `done_with_elapsed_by_id_drives_the_parent_receipt` +
// `done_with_elapsed_by_id_blocking_advisory_keeps_amount_none` below.

// ── TaskId wire format ────────────────────────────────────────────
//
// Regression guard for the live bug found 2026-05-30: specta types
// `u64` as a TS `string`, so the frontend echoes the task id back as
// the string `"1"`. The Tauri command param must deserialize THAT —
// a bare `u64` param rejected it (`invalid type: string "1", expected
// u64`), which crashed every plugin `task.progress()`/terminal call and
// aborted the whole hook. These tests pin the string-in / string-out
// contract at the type level so it can't silently regress.

#[test]
fn task_id_deserializes_from_specta_string() {
    // The exact value the frontend sends back (specta stringifies u64).
    let id: TaskId = serde_json::from_str("\"1\"").expect("string id");
    assert_eq!(id, TaskId(1));
}

#[test]
fn task_id_deserializes_from_raw_number() {
    // Robustness: a non-specta caller may send a JSON number.
    let id: TaskId = serde_json::from_str("42").expect("number id");
    assert_eq!(id, TaskId(42));
}

#[test]
fn task_id_serializes_to_string() {
    // Output must be a string so the frontend round-trips it losslessly.
    let json = serde_json::to_string(&TaskId(7)).expect("serialize");
    assert_eq!(json, "\"7\"");
}

#[test]
fn task_id_round_trips_string() {
    let original = TaskId(123_456_789);
    let json = serde_json::to_string(&original).unwrap();
    let back: TaskId = serde_json::from_str(&json).unwrap();
    assert_eq!(original, back);
}

#[test]
fn task_id_rejects_non_numeric_string() {
    assert!(serde_json::from_str::<TaskId>("\"nope\"").is_err());
}

#[test]
fn task_id_deserializes_inside_command_arg_shape() {
    // Mirror the real Tauri command arg object: `taskId` arrives as the
    // specta-string `"1"` nested in the argument struct. This is the
    // exact shape `report_plugin_task_lifecycle_command` receives.
    #[derive(Deserialize)]
    struct Args {
        task_id: Option<TaskId>,
    }
    let args: Args = serde_json::from_str(r#"{ "task_id": "1" }"#).expect("command args");
    assert_eq!(args.task_id, Some(TaskId(1)));

    // `None` for the Started call (id minted server-side) must also work.
    let started: Args = serde_json::from_str(r#"{ "task_id": null }"#).expect("null id");
    assert_eq!(started.task_id, None);
}

// ── PluginHook → TaskKind mapping ─────────────────────────────────

#[test]
fn plugin_hook_maps_to_task_kind_1_to_1() {
    let cases = [
        (PluginHook::Import, TaskKind::Import),
        (PluginHook::Publish, TaskKind::Publish),
        (PluginHook::Deploy, TaskKind::Deploy),
        (PluginHook::Syndicate, TaskKind::Syndicate),
        (PluginHook::Process, TaskKind::Process),
    ];
    for (hook, expected) in cases {
        let kind: TaskKind = hook.into();
        assert_eq!(kind, expected, "PluginHook::{:?} → TaskKind", hook);
    }
}

// ── Router exhaustiveness ─────────────────────────────────────────

/// Asserts every `(PluginHook, TriggerContext)` cross-product produces
/// a defined `(TaskScope, TaskKind, TaskTone)` triple. 5 hooks × 4
/// triggers = 20 rows. This test would catch a missing router row
/// even though the compiler's exhaustive-match check already would —
/// belt-and-suspenders against the next person adding a wildcard arm.
#[test]
fn router_covers_every_cell() {
    use PluginHook::*;
    use TriggerContext::*;

    let hooks = [Import, Publish, Deploy, Syndicate, Process];
    let triggers = [OnboardingFlow, SettingsManual, Background, ManualOne];

    let mut seen = 0;
    for &hook in &hooks {
        for &trigger in &triggers {
            let signal = PluginTaskSignal {
                hook,
                trigger,
                has_progress: true,
                cancellable: false,
                awaiting_user: false,
            };
            let (_scope, kind, _tone) = route_plugin_task(signal);
            // kind must agree with the From<PluginHook> mapping
            assert_eq!(kind, TaskKind::from(hook), "kind for {hook:?}+{trigger:?}");
            seen += 1;
        }
    }
    assert_eq!(seen, 20, "router must cover every (hook, trigger) pair");
}

/// Spot-check specific router rows from the Layer 2 table.
#[test]
fn router_spot_checks_match_layer2_table() {
    let cases = [
        (
            PluginHook::Import,
            TriggerContext::OnboardingFlow,
            TaskScope::ActionPanel,
            TaskTone::Ambient,
        ),
        (
            PluginHook::Import,
            TriggerContext::SettingsManual,
            TaskScope::Workspace,
            TaskTone::Narrated,
        ),
        (
            PluginHook::Publish,
            TriggerContext::Background,
            TaskScope::Workspace,
            TaskTone::Ambient,
        ),
        (
            PluginHook::Deploy,
            TriggerContext::ManualOne,
            TaskScope::Preview,
            TaskTone::Inline,
        ),
        (
            PluginHook::Syndicate,
            TriggerContext::OnboardingFlow,
            TaskScope::ActionPanel,
            TaskTone::Inline,
        ),
        (
            PluginHook::Process,
            TriggerContext::OnboardingFlow,
            TaskScope::Workspace,
            TaskTone::Ambient,
        ),
    ];
    for (hook, trigger, expected_scope, expected_tone) in cases {
        let signal = PluginTaskSignal {
            hook,
            trigger,
            has_progress: true,
            cancellable: false,
            awaiting_user: false,
        };
        let (scope, _kind, tone) = route_plugin_task(signal);
        assert_eq!(scope, expected_scope, "scope for {hook:?}+{trigger:?}");
        assert_eq!(tone, expected_tone, "tone for {hook:?}+{trigger:?}");
    }
}

// ── Awaiting preserves scope ──────────────────────────────────────

#[test]
fn awaiting_escalates_tone_but_preserves_scope() {
    // Deploy + OnboardingFlow normally routes to (Preview, Deploy, Inline).
    // With awaiting_user, the tone escalates to Awaiting but scope STAYS
    // on Preview — a deploy waiting on DNS surfaces where the deploy lives,
    // not in the action panel.
    let signal = PluginTaskSignal {
        hook: PluginHook::Deploy,
        trigger: TriggerContext::OnboardingFlow,
        has_progress: true,
        cancellable: false,
        awaiting_user: true,
    };
    let (scope, kind, tone) = route_plugin_task(signal);
    assert_eq!(scope, TaskScope::Preview, "scope preserved on awaiting");
    assert_eq!(kind, TaskKind::Deploy);
    assert_eq!(tone, TaskTone::Awaiting, "tone escalated to Awaiting");
}

#[test]
fn awaiting_preserves_scope_across_all_rows() {
    use PluginHook::*;
    use TriggerContext::*;

    let hooks = [Import, Publish, Deploy, Syndicate, Process];
    let triggers = [OnboardingFlow, SettingsManual, Background, ManualOne];

    for &hook in &hooks {
        for &trigger in &triggers {
            let base = PluginTaskSignal {
                hook,
                trigger,
                has_progress: true,
                cancellable: false,
                awaiting_user: false,
            };
            let (base_scope, _, _) = route_plugin_task(base);
            let awaiting = PluginTaskSignal {
                awaiting_user: true,
                ..base
            };
            let (await_scope, _, await_tone) = route_plugin_task(awaiting);
            assert_eq!(
                base_scope, await_scope,
                "scope must be preserved for {hook:?}+{trigger:?} when awaiting",
            );
            assert_eq!(
                await_tone,
                TaskTone::Awaiting,
                "tone must escalate to Awaiting for {hook:?}+{trigger:?}",
            );
        }
    }
}

// ── TaskRegistry aggregation ──────────────────────────────────────

#[test]
fn max_running_fraction_picks_highest_across_concurrent_tasks() {
    let reg = registry();
    let h1 = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Import,
        TaskTone::Ambient,
    );
    let h2 = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Import,
        TaskTone::Ambient,
    );
    let h3 = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Import,
        TaskTone::Ambient,
    );

    h1.progress(Some(0.25), None);
    h2.progress(Some(0.7), None);
    h3.progress(Some(0.5), None);

    let max = reg.max_running_fraction(&win(), TaskScope::ActionPanel);
    assert_eq!(max, Some(0.7));
}

#[test]
fn max_running_fraction_is_none_when_all_indeterminate() {
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Build,
        TaskTone::Ambient,
    );
    h.progress(None, Some("scanning".into()));
    assert_eq!(reg.max_running_fraction(&win(), TaskScope::Preview), None);
}

#[test]
fn max_running_fraction_ignores_terminal_tasks() {
    let reg = registry();
    let h1 = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    let h2 = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    h1.progress(Some(0.9), None);
    h2.progress(Some(0.3), None);
    h1.succeeded(Some("Saved".into()));
    let max = reg.max_running_fraction(&win(), TaskScope::ActionPanel);
    assert_eq!(max, Some(0.3), "terminal task is excluded");
}

#[test]
fn latest_narrated_returns_newest_running_narrated_task() {
    let reg = registry();
    let _ambient = reg.spawn(
        win(),
        TaskScope::Workspace,
        TaskKind::Build,
        TaskTone::Ambient,
    );
    let n1 = reg.spawn(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
    );
    let n2 = reg.spawn(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
    );
    n1.progress(Some(0.1), Some("phase 1".into()));
    n2.progress(Some(0.4), Some("phase 2".into()));

    let latest = reg
        .latest_narrated(&win(), TaskScope::Workspace)
        .expect("should have a narrated task");
    assert_eq!(latest.id, n2.id, "latest_narrated picks newest spawn");
}

#[test]
fn latest_narrated_skips_terminal_tasks() {
    let reg = registry();
    let n1 = reg.spawn(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
    );
    let n2 = reg.spawn(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
    );
    n2.succeeded(None);

    let latest = reg
        .latest_narrated(&win(), TaskScope::Workspace)
        .expect("should fall back to n1");
    assert_eq!(latest.id, n1.id, "terminal narrated task is skipped");
}

#[test]
fn any_awaiting_flips_on_awaiting_state() {
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Awaiting,
    );
    assert!(
        !reg.any_awaiting(&win(), TaskScope::Preview),
        "running, not awaiting"
    );
    h.awaiting("verify DNS in your registrar", Action::None);
    assert!(reg.any_awaiting(&win(), TaskScope::Preview));
    h.progress(Some(0.5), None);
    assert!(
        !reg.any_awaiting(&win(), TaskScope::Preview),
        "progress clears awaiting"
    );
}

#[test]
fn failed_unrecoverable_lists_only_unrecoverable_failures() {
    // Post-collapse, recoverability lives in the advisory's severity:
    // a `Blocking` advisory is the unrecoverable case the toast subscriber
    // surfaces; `NeedsAction`/`ShippedDegraded` are recoverable (no toast).
    let reg = registry();
    let a = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    let b = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Inline,
    );
    let c = reg.spawn(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Ambient,
    );

    a.failed("disk full", false); // unrecoverable (Blocking) → toast
    b.failed("offline", true); // recoverable (NeedsAction) → no toast
    c.succeeded(None); // not a failure

    let failures = reg.failed_unrecoverable();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, a.id);
}

// ── State transitions ─────────────────────────────────────────────

#[test]
fn spawn_progress_succeeded_chain() {
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    assert_eq!(tasks.len(), 1);
    assert!(matches!(
        tasks[0].state,
        TaskState::Running {
            fraction: None,
            message: None
        }
    ));

    h.progress(Some(0.5), Some("writing".into()));
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    assert!(matches!(
        tasks[0].state,
        TaskState::Running {
            fraction: Some(_),
            ..
        }
    ));

    h.succeeded(Some("Saved".into()));
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    assert!(matches!(tasks[0].state, TaskState::Succeeded { .. }));
    assert!(tasks[0].is_terminal());
}

#[test]
fn spawn_awaiting_progress_succeeded_implicit_resume() {
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Awaiting,
    );
    h.awaiting("click the link in your email", Action::None);
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    assert!(matches!(tasks[0].state, TaskState::Awaiting { .. }));

    // Next progress() implicitly resumes (no explicit resumed()).
    h.progress(Some(0.9), Some("propagating DNS".into()));
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    assert!(matches!(tasks[0].state, TaskState::Running { .. }));

    h.succeeded(None);
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    assert!(matches!(tasks[0].state, TaskState::Succeeded { .. }));
}

#[test]
fn spawn_failed_synthesizes_advisory_from_error() {
    // Post-collapse, a bare `failed(error, recoverable)` synthesizes the
    // root-cause advisory: `what` from the error, severity from
    // recoverability (unrecoverable → Blocking).
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    h.failed("permission denied", false);
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    match &tasks[0].state {
        TaskState::Failed { advisory } => {
            assert_eq!(advisory.what, "permission denied");
            assert!(
                matches!(advisory.severity, Severity::Blocking),
                "unrecoverable → Blocking",
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn spawn_cancelled_marks_terminal() {
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
    );
    h.cancelled();
    let tasks = reg.tasks(&win(), TaskScope::Workspace);
    assert!(matches!(tasks[0].state, TaskState::Cancelled));
    assert!(tasks[0].is_terminal());
}

// ── Window isolation ──────────────────────────────────────────────

#[test]
fn registry_isolates_tasks_per_window() {
    let reg = registry();
    let w1 = WindowId::from("main");
    let w2 = WindowId::from("launcher");
    let h1 = reg.spawn(
        w1.clone(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    let _h2 = reg.spawn(
        w2.clone(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    h1.progress(Some(0.8), None);

    assert_eq!(reg.tasks(&w1, TaskScope::ActionPanel).len(), 1);
    assert_eq!(reg.tasks(&w2, TaskScope::ActionPanel).len(), 1);
    assert_eq!(
        reg.max_running_fraction(&w1, TaskScope::ActionPanel),
        Some(0.8)
    );
    // w2's task has no fraction yet → None
    assert_eq!(reg.max_running_fraction(&w2, TaskScope::ActionPanel), None);
}

// ── prune_terminal ────────────────────────────────────────────────

#[test]
fn prune_terminal_removes_only_terminal_tasks() {
    let reg = registry();
    let a = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    let b = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    a.succeeded(None);
    b.progress(Some(0.5), None);

    reg.prune_terminal(&win(), TaskScope::ActionPanel);
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, b.id);
}

// ── with_progress closure form ────────────────────────────────────

#[test]
fn with_progress_marks_succeeded_on_ok() {
    let reg = registry();
    let result: Result<i32, String> = reg.with_progress(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
        "Importing",
        |h| {
            h.progress(Some(0.5), None);
            Ok(42)
        },
    );
    assert_eq!(result, Ok(42));
    let tasks = reg.tasks(&win(), TaskScope::Workspace);
    assert_eq!(tasks.len(), 1);
    assert!(matches!(tasks[0].state, TaskState::Succeeded { .. }));
}

#[test]
fn with_progress_marks_failed_on_err() {
    let reg = registry();
    let result: Result<(), String> = reg.with_progress(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
        "Importing",
        |_h| Err("boom".to_string()),
    );
    assert_eq!(result, Err("boom".to_string()));
    let tasks = reg.tasks(&win(), TaskScope::Workspace);
    assert_eq!(tasks.len(), 1);
    match &tasks[0].state {
        TaskState::Failed { advisory } => {
            assert_eq!(advisory.what, "boom");
            assert!(
                matches!(advisory.severity, Severity::NeedsAction),
                "with_progress defaults to recoverable → NeedsAction",
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

// ── fade_after on Inline ──────────────────────────────────────────

#[test]
fn set_fade_after_persists_on_task() {
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    h.set_fade_after(Some(Duration::from_secs(30)));
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    assert_eq!(tasks[0].fade_after, Some(Duration::from_secs(30)));
}

// ── title field ──────────────────────────────────────────────────

#[test]
fn spawn_then_title_sets_panel_task_title() {
    let reg = registry();
    let mut h = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    // Spawned tasks start with no title.
    let pre = reg.tasks(&win(), TaskScope::ActionPanel);
    assert_eq!(pre[0].title, None, "spawn should leave title None");

    h.title("Saving index.md");
    let post = reg.tasks(&win(), TaskScope::ActionPanel);
    assert_eq!(post[0].title.as_deref(), Some("Saving index.md"));
}

#[test]
fn title_builder_returns_mut_self_for_chaining_through_let_binding() {
    // When internal moss code calls PanelTask directly, the
    // intended call shape is fluent. Because `title` takes `&mut self`,
    // the temporary from `spawn(...)` must be bound to a `mut` local
    // before titling — the binding is the chain.
    let reg = registry();
    let mut h = reg.spawn(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
    );
    let returned = h.title("Importing from Matters");
    // Returned reference must be the same handle (chaining contract).
    returned.title("Importing 42 files"); // override is allowed
    let tasks = reg.tasks(&win(), TaskScope::Workspace);
    assert_eq!(tasks[0].title.as_deref(), Some("Importing 42 files"));
}

#[test]
fn with_progress_populates_title_from_label() {
    let reg = registry();
    let _: Result<(), String> = reg.with_progress(
        win(),
        TaskScope::Workspace,
        TaskKind::Import,
        TaskTone::Narrated,
        "Importing 42 files",
        |h| {
            // Title is set before the closure body runs so renderers
            // observing mid-closure progress events see the right label.
            let snapshot = reg.tasks(&win(), TaskScope::Workspace);
            assert_eq!(
                snapshot[0].title.as_deref(),
                Some("Importing 42 files"),
                "title should be set by with_progress before closure runs",
            );
            h.progress(Some(0.5), None);
            Ok(())
        },
    );
    let tasks = reg.tasks(&win(), TaskScope::Workspace);
    assert_eq!(tasks[0].title.as_deref(), Some("Importing 42 files"));
}

#[test]
fn with_progress_empty_label_leaves_title_none() {
    let reg = registry();
    let _: Result<(), String> = reg.with_progress(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
        "", // empty label → don't set a title
        |_h| Ok(()),
    );
    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    assert_eq!(
        tasks[0].title, None,
        "empty label should leave title as None rather than an empty string",
    );
}

// ── wire-snapshot + emitter (T2) ─────────────────────────────────

#[test]
fn panel_task_wire_strips_instant_and_preserves_state() {
    let reg = registry();
    let mut h = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    h.title("Saving index.md");
    h.set_fade_after(Some(Duration::from_secs(30)));
    h.progress(Some(0.42), Some("writing".into()));

    let tasks = reg.tasks(&win(), TaskScope::ActionPanel);
    let wire = PanelTaskWire::from(&tasks[0]);

    // Wire snapshot round-trips through serde_json — the whole point
    // of the type is that it crosses the Tauri FFI boundary.
    let json = serde_json::to_string(&wire).expect("serialize");
    let back: PanelTaskWire = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.id, wire.id);
    assert_eq!(back.title.as_deref(), Some("Saving index.md"));
    assert_eq!(back.fade_after_ms, Some(30_000));
    assert_eq!(back.scope, TaskScope::ActionPanel);
    assert_eq!(back.tone, TaskTone::Inline);
    match back.state {
        TaskState::Running { fraction, message } => {
            assert!((fraction.unwrap() - 0.42).abs() < f32::EPSILON);
            assert_eq!(message.as_deref(), Some("writing"));
        }
        other => panic!("expected Running, got {other:?}"),
    }
}

#[test]
fn emitter_fires_on_spawn_and_each_mutation() {
    use std::sync::Mutex;
    let captured: Arc<Mutex<Vec<PanelTaskWire>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = Arc::clone(&captured);

    let reg = registry();
    reg.set_emitter(Box::new(move |snap| {
        captured_clone.lock().unwrap().push(snap);
    }));

    // 1 spawn + 1 progress + 1 succeeded = 3 emissions.
    let h = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    h.progress(Some(0.5), None);
    h.succeeded(Some("Saved".into()));

    let snaps = captured.lock().unwrap();
    assert_eq!(
        snaps.len(),
        3,
        "spawn + progress + succeeded = 3 wire events"
    );
    assert!(matches!(
        snaps[0].state,
        TaskState::Running {
            fraction: None,
            message: None
        }
    ));
    assert!(matches!(
        snaps[1].state,
        TaskState::Running {
            fraction: Some(_),
            ..
        }
    ));
    assert!(matches!(snaps[2].state, TaskState::Succeeded { .. }));
}

#[test]
fn emitter_can_be_cleared_to_silence_subsequent_emits() {
    use std::sync::Mutex;
    let captured: Arc<Mutex<Vec<PanelTaskWire>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = Arc::clone(&captured);

    let reg = registry();
    reg.set_emitter(Box::new(move |snap| {
        captured_clone.lock().unwrap().push(snap);
    }));
    let h = reg.spawn(
        win(),
        TaskScope::ActionPanel,
        TaskKind::Save,
        TaskTone::Inline,
    );
    assert_eq!(captured.lock().unwrap().len(), 1);

    reg.clear_emitter();
    h.succeeded(None);
    assert_eq!(
        captured.lock().unwrap().len(),
        1,
        "post-clear mutation should not emit",
    );
}

// ── Awaiting carries a typed Action (R3) ─────────────────────────

#[test]
fn awaiting_carries_the_typed_action() {
    // R3: `Awaiting` carries `action: Action` directly (no Option, no
    // legacy `escape`). A bare cancel is `Action::None`; a recovery
    // affordance is a typed `InApp`/`Command`/`Link`.
    let reg = registry();
    let h = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Deploy,
        TaskTone::Awaiting,
    );
    h.awaiting(
        "verify your email",
        Action::InApp {
            op: crate::advisory::AppOp::SignIn,
            args: serde_json::json!({ "reason": "verify_email" }),
            label: "Open Settings".into(),
        },
    );
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    match &tasks[0].state {
        TaskState::Awaiting { directive, action } => {
            assert_eq!(directive, "verify your email");
            assert!(matches!(
                action,
                Action::InApp {
                    op: crate::advisory::AppOp::SignIn,
                    ..
                }
            ));
        }
        other => panic!("expected Awaiting carrying the typed action, got {other:?}"),
    }
}

#[test]
fn awaiting_serializes_action_on_the_wire() {
    // The collapsed shape serializes `state: "awaiting"` with a typed
    // `action` (no `escape` field). The frontend awaiting renderer reads
    // `action` to paint its affordance.
    let state = TaskState::Awaiting {
        directive: "click the link in your email".into(),
        action: Action::None,
    };
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains("\"state\":\"awaiting\""), "got {json}");
    assert!(json.contains("\"action\""), "got {json}");
    assert!(!json.contains("escape"), "escape must be gone: {json}");
}

// ── parent/child Jobs (Step 3 Phase 4 — media nests under Build) ──────

#[test]
fn spawn_child_sets_parent_to_the_build_job() {
    // The media child Job nests under the Build parent (Phase 4): spawn the
    // parent first, then spawn a child carrying `parent: Some(buildJobId)`.
    let reg = registry();
    let parent = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Build,
        TaskTone::Ambient,
    );
    let _child = reg.spawn_child(
        win(),
        TaskScope::Preview,
        TaskKind::AssetTransform,
        TaskTone::Ambient,
        parent.id,
    );
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let child_task = tasks
        .iter()
        .find(|t| t.parent == Some(parent.id))
        .expect("child Job should carry parent = build job id");
    assert_eq!(child_task.parent, Some(parent.id));
    // The parent itself has no parent.
    let parent_task = tasks.iter().find(|t| t.id == parent.id).unwrap();
    assert!(parent_task.parent.is_none());
}

#[test]
fn done_with_elapsed_by_id_drives_the_parent_receipt() {
    // The Build receipt is driven from the media worker, which holds only the
    // registry + the parent TaskId (no &mut TaskHandle). `done_with_elapsed_by_id`
    // applies the producer-supplied receipt by id, mirroring the handle method.
    let reg = registry();
    let parent = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Build,
        TaskTone::Ambient,
    );
    reg.done_with_elapsed_by_id(
        parent.id,
        Verb::core(Verb::BUILT),
        Some(Amount {
            count: 142,
            noun: "pages".into(),
        }),
        None,
        vec![],
        Duration::from_millis(2100),
    );
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let parent_task = tasks.iter().find(|t| t.id == parent.id).unwrap();
    assert!(matches!(parent_task.state, TaskState::Succeeded { .. }));
    assert_eq!(parent_task.verb.as_ref().unwrap().0, "Built");
    assert_eq!(parent_task.amount().unwrap().count, 142);
    assert_eq!(parent_task.elapsed(), Some(Duration::from_millis(2100)));
}

#[test]
fn done_with_elapsed_by_id_blocking_advisory_keeps_amount_none() {
    // Invariant #4 + C2 across the by-id path: a Blocking advisory flips the
    // Build Job to Failed and must NOT stamp an amount.
    let reg = registry();
    let parent = reg.spawn(
        win(),
        TaskScope::Preview,
        TaskKind::Build,
        TaskTone::Ambient,
    );
    reg.done_with_elapsed_by_id(
        parent.id,
        Verb::core(Verb::BUILT),
        Some(Amount {
            count: 9,
            noun: "pages".into(),
        }),
        None,
        vec![blocking_advisory()],
        Duration::from_millis(500),
    );
    let tasks = reg.tasks(&win(), TaskScope::Preview);
    let parent_task = tasks.iter().find(|t| t.id == parent.id).unwrap();
    assert!(matches!(parent_task.state, TaskState::Failed { .. }));
    assert!(parent_task.amount().is_none());
}

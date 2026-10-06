//! Sync test: every publish path in this crate asks the gates that refuse a
//! publish.
//!
//! Open-half twin of the desktop app's `publish_gate_invariant_test.rs` —
//! that file's `GATES` table is a flat list of `(path, call, consequence)`
//! rows mixing `src/*` (desktop) and `../open/crates/moss-build/src/deploy/*.rs`
//! rows, each checked independently with no cross-row comparison. This twin
//! carries only the two open-half rows (the `must_precede` pair that shares
//! one file); the desktop half keeps its own rows.
//!
//! Not carried here: `a_headless_build_leaves_the_verdict_the_gate_reads`,
//! which drives `moss::build_sync` — a desktop-crate-only entry point with no
//! moss-build equivalent.
//!
//! `moss_build::deploy::refuse_publish` is what makes "a published site cannot
//! contain a broken file" true for callers that never ran the frontend's
//! check. No cheaper layer can see a MISSING call — so this reads the
//! sources and fails if a publish entry point stops asking.

use std::path::Path;

struct Gate {
    path: &'static str,
    call: &'static str,
    consequence: &'static str,
    must_precede: Option<&'static str>,
}

const PUBLISH_PREFLIGHT: &str = "deploy::refuse_publish(";
const PUBLISH_SETUP: &str = "publish_setup::refuse_publish(";
const BROKEN_FILE: &str = "a site with a broken file can be published from it. Restore the \
     call AFTER the in-flight drain and the pre-publish rebuild — earlier reads \
     a half-encoded site as a broken one";
const SETUP_MISSING: &str = "a publish with no credential stored now reaches the plugin, which \
     fails inside its own upload with the remote service's words instead of moss's. \
     Restore the call BEFORE the build — what the target declared is knowable at t=0";

const GATES: &[Gate] = &[
    // The headless moss-hosted publish (moss deploy <folder>). The route
    // builds first, so its gate reads that build's own verdict — which is why
    // the call sits after `run_pipeline` and not at the top of the driver.
    Gate {
        path: "src/deploy/push.rs",
        call: PUBLISH_PREFLIGHT,
        consequence: BROKEN_FILE,
        must_precede: None,
    },
    // Both plugin publishes — the gate sits at the top of
    // `run_plugin_deploy_inner`, which both routes enter. `must_precede`
    // pairs it with the setup gate in the same file, in the order both
    // routes ask them: setup first, in each caller's preamble before the
    // build; the publish preflight second, inside the shared body after it.
    Gate {
        path: "src/deploy/plugin_push.rs",
        call: PUBLISH_SETUP,
        consequence: SETUP_MISSING,
        must_precede: Some(PUBLISH_PREFLIGHT),
    },
];

#[test]
fn every_publish_path_asks_its_gates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for gate in GATES {
        let src = std::fs::read_to_string(root.join(gate.path))
            .unwrap_or_else(|e| panic!("publish path {} must be readable: {e}", gate.path));
        let at = src.find(gate.call).unwrap_or_else(|| {
            panic!(
                "{} is a publish entry point and no longer calls {} — {}. If this file \
                 stopped being a publish path, drop its row from GATES.",
                gate.path, gate.call, gate.consequence
            )
        });
        if let Some(later) = gate.must_precede {
            let later_at = src
                .find(later)
                .unwrap_or_else(|| panic!("{} no longer calls {later}", gate.path));
            assert!(
                at < later_at,
                "{} now asks {} after {later}, so a publish that was refusable at t=0 \
                 waits out the drain and the rebuild first",
                gate.path,
                gate.call
            );
        }
    }
}

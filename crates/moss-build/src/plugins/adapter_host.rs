//! What the engine adapter and the plugin manager both hold: the one record
//! that says where a host call's state lives, whether a desktop is attached,
//! where progress is reported, and on which runtime background work runs.
//!
//! Before this record the adapter carried an `Option<AppHandle>` plus a
//! stand-in `StandaloneHostState`, and every site that needed the hook state
//! asked "app or standalone?" first. The state is now resolved ONCE, where the
//! manager is built (`tauri_app_host::adapter_host_for` app-side,
//! [`AdapterHost::headless`] elsewhere), and every reader takes `state.hooks`.
//! Nothing in here names tauri (ADR-050, ADR-076).

use std::sync::Arc;

use crate::build::ports::reporter::BuildReporter;
use crate::build::ports::spawner::{Spawner, TokioSpawner};
use crate::engine::app_host::SharedAppHost;
use crate::engine::host_fns::HostState;

#[derive(Clone)]
pub struct AdapterHost {
    /// Task registries and hook-execution state — the app's managed singletons
    /// when there is an app, fresh ones headless. Every field is an `Arc`, so a
    /// clone of this record is a second handle on the same state.
    pub state: HostState,
    /// The desktop, when there is one. `None` is the headless build, where the
    /// app-only command arms refuse by name (ADR-050).
    pub app: Option<SharedAppHost>,
    /// Where the manager reports plugin progress and failures.
    pub reporter: Arc<dyn BuildReporter>,
    /// The process's one async runtime.
    pub spawner: Arc<dyn Spawner>,
}

impl AdapterHost {
    /// A host with no desktop: fresh registries, plain tokio.
    pub fn headless(reporter: Arc<dyn BuildReporter>) -> Self {
        Self {
            state: HostState::standalone(),
            app: None,
            reporter,
            spawner: Arc::new(TokioSpawner),
        }
    }
}

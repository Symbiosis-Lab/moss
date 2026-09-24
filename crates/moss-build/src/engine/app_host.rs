//! The app-only half of the engine host — a flag-column seam.
//!
//! Every method here needs a desktop: the live webview cookie jar, the action
//! panel, the system browser, or the app's managed state. The engine holds an
//! `Option<&dyn AppHost>` / `Option<SharedAppHost>`; `None` is the headless
//! build, where the gated command arms refuse BY NAME instead of silently
//! dropping plugin content (`need_app` in `host_fns.rs`).
//!
//! The one implementation is `plugins::tauri_app_host::TauriAppHost`, a thin
//! wrapper over `tauri::AppHandle`. This trait exists so the engine itself
//! never names tauri — a later amendment moves the engine into
//! `moss-build`, and this file crosses with it while the impl stays app-side.
//!
//! Explicit methods, not a stringly `dispatch(cmd, args)` arm: seam width
//! matters, and a string-routed method would hide exactly the surface this
//! trait is supposed to declare.

/// Shared, clonable handle for the threads that outlive a borrow — the engine
/// thread parks one inside its `__TAURI__.event.emit` closures.
pub type SharedAppHost = std::sync::Arc<dyn AppHost>;

/// A bus subscription handle, as [`AppHost::listen`] returns it.
pub type ListenerId = u32;

#[async_trait::async_trait]
pub trait AppHost: Send + Sync {
    // ── live-webview cookie jar (app-only by nature) ─────────────────────────

    /// Read `plugin`'s cookies from the live webview jar, serialized as a JSON
    /// array — the exact string the `get_plugin_cookie` arm replies with.
    async fn get_plugin_cookies(
        &self,
        plugin: &str,
        project_path: &str,
    ) -> Result<String, String>;

    /// Write cookies into the live webview jar. `cookies` is the raw JSON the
    /// SDK sent (an array of `CookieInput`); parsing stays app-side with the
    /// type.
    async fn set_plugin_cookies(
        &self,
        plugin: &str,
        project_path: &str,
        cookies: serde_json::Value,
    ) -> Result<(), String>;

    /// Clear `plugin`'s cookie domains from the live webview jar.
    async fn clear_plugin_cookies(&self, plugin: &str, project_path: &str)
        -> Result<(), String>;

    // ── action panel + system browser ────────────────────────────────────────

    /// `plugin` is the CALLER's id, injected by the host seam rather than taken
    /// from the plugin's own arguments: it is what lets the shell title the
    /// panel after the plugin instead of after the URL it happened to open.
    async fn open_action_panel(
        &self,
        url: String,
        title: Option<String>,
        below_titlebar: bool,
        plugin: Option<String>,
    ) -> Result<(), String>;

    async fn set_action_panel_html(
        &self,
        html: String,
        title: Option<String>,
        plugin: Option<String>,
    ) -> Result<(), String>;

    async fn close_action_panel(&self) -> Result<(), String>;

    async fn open_system_browser(&self, url: String) -> Result<(), String>;

    /// Switch the main shell back to the editor view — plugins call this after
    /// closing a login/setup panel so the user isn't left staring at an empty
    /// panel slot.
    async fn return_to_editor(&self) -> Result<(), String>;

    // ── moss's own credential modal, opened on a plugin's behalf ─────────────

    /// Ask the user for `plugin`'s credential `key` again, in MOSS's modal, and
    /// resolve with what they typed. `Ok(None)` = cancelled.
    ///
    /// App-only by nature: the answer is a person's, so a host with no window
    /// has nobody to ask. The engine's reject arm degrades to erase-and-return-
    /// `null` there rather than refusing, because forgetting a dead token is
    /// still the right thing to do in a headless build.
    ///
    /// `detail` is the plugin's one sentence saying why it is asking again. The
    /// label and help link are NOT parameters: they come from the plugin's
    /// manifest, resolved app-side, so a plugin cannot relabel its own field
    /// into something the user reads as a different question.
    async fn prompt_credential(
        &self,
        plugin: &str,
        project_path: &str,
        key: &str,
        detail: Option<String>,
    ) -> Result<Option<String>, String>;

    // ── the app's event bus, for the engine's `listen` shim ─────────────────

    /// Subscribe to one named event on the app's bus. `handler` receives each
    /// event's payload as its JSON text. Returns the id [`Self::unlisten`]
    /// takes. App-only by nature: headless there is no bus to listen on, and
    /// the engine's `__engine_listen__` arm answers `null` without registering.
    fn listen(&self, name: String, handler: Box<dyn Fn(&str) + Send + Sync + 'static>) -> ListenerId;

    fn unlisten(&self, id: ListenerId);

    // ── engine-side `__TAURI__.event.emit` routing ───────────────────────────

    /// Route one plugin-emitted event to the app: toasts to the typed
    /// `MossEvent` bus, the broadcast names to every webview, everything else
    /// to the browser panel. Fire-and-forget — the emit arm never fails the JS
    /// caller over a delivery problem. (`plugin-message` never reaches this;
    /// it rides the dispatch bridge regardless of app.)
    fn emit_plugin_event(&self, name: &str, payload_json: Option<String>);
}

//! The renderer thread: one QuickJS runtime running the vendored pele bundle.
//!
//! A QuickJS runtime is `!Send`, so it lives on its own thread, started by the
//! first diagram a process meets, and callers reach it over a channel. The
//! thread carries an explicit stack and the engine's stack limit sits well
//! under it: with the limit at or above the thread's stack, a deep recursion
//! overflows the native stack and aborts the process instead of raising a
//! `RangeError`. A memory limit bounds what one diagram can allocate, and an
//! interrupt handler stops a render that runs past its time budget. After
//! either, the thread starts a fresh engine for the next diagram.
//!
//! One thread serves the whole build. A diagram takes about a millisecond to
//! draw in a release build (0.2 to 1.7 ms measured across the 33 types), so
//! a page's diagrams queue for less time than loading the renderer into a
//! second engine would take (about 60 ms).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rquickjs::{Context, Function, Runtime};

/// The pele release embedded in this binary and the SHA-256 of its bundle,
/// written by `scripts/pele/update.mjs`.
const PIN: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/pele/pin.txt"));
const BUNDLE_BROTLI: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/pele/pele.global.js.br"));
/// pele's licence, loaded with the bundle so every copy of it carries it.
const NOTICE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/pele/NOTICE"));

const THREAD_STACK: usize = 16 << 20;
const ENGINE_STACK: usize = THREAD_STACK / 4;
const ENGINE_MEMORY: usize = 256 << 20;
/// Wall-clock budget for one render. The slowest diagram under the edge limit
/// takes well under a second in a release build.
const BUDGET: Duration = Duration::from_secs(3);

/// Limits moss sets on the renderer. The source limit equals the renderer's
/// own default and the others sit under theirs; all are pinned here so an
/// upgrade cannot move them.
const MAX_SOURCE_CHARS: usize = 50_000;
const MAX_EDGES: usize = 2_000;
const MAX_OUTPUT_CHARS: usize = 2_000_000;

/// The link schemes a drawing may keep, for the renderer and the sanitiser.
pub(super) const LINK_SCHEMES: &[&str] = &["http", "https", "mailto", "tel"];

/// The render options. Nothing in a diagram can change them. The clock is
/// fixed so a gantt chart or timeline draws the same bytes on every build.
fn options() -> String {
    serde_json::json!({
        "now": 1_767_225_600_000_u64,
        "images": false,
        "links": true,
        "linkSchemes": LINK_SCHEMES,
        "responsive": false,
        "limit": MAX_SOURCE_CHARS,
        "maxEdges": MAX_EDGES,
        "outputLimit": MAX_OUTPUT_CHARS,
    })
    .to_string()
}

/// QuickJS reads local time from the operating system, and the renderer does
/// its date arithmetic in local time, so a gantt chart would draw different
/// bytes in another time zone or season. Every local-time view of a date the
/// renderer uses is made UTC: the getters and setters, the offset, the
/// many-argument constructor and a date-time string written without a zone.
const DATES_IN_UTC: &str = r#"
(() => {
  const D = Date, P = D.prototype;
  for (const n of ['FullYear', 'Month', 'Date', 'Day', 'Hours', 'Minutes', 'Seconds', 'Milliseconds']) {
    P['get' + n] = P['getUTC' + n];
    if (P['setUTC' + n]) P['set' + n] = P['setUTC' + n];
  }
  P.getTimezoneOffset = () => 0;
  const zoned = (s) => (typeof s === 'string' && /^\d{4}-\d\d-\d\dT[\d:.]+$/.test(s.trim()) ? s.trim() + 'Z' : s);
  function UTCDate(...a) {
    if (!new.target) return new D(D.now()).toISOString();
    if (a.length >= 2) return new D(D.UTC(...a));
    return a.length === 1 ? new D(zoned(a[0])) : new D();
  }
  UTCDate.prototype = P;
  UTCDate.UTC = D.UTC;
  UTCDate.now = D.now;
  UTCDate.parse = (s) => D.parse(zoned(s));
  globalThis.Date = UTCDate;
})();
"#;

/// Defines `__moss_render(source, idPrefix, maxWidth)`, which returns serde's
/// JSON form of `Result<Drawn, Refusal>`.
const GLUE: &str = r#"
((options) => {
  globalThis.__moss_render = (source, idPrefix, maxWidth) => {
    try {
      const o = { ...options, idPrefix };
      if (maxWidth > 0) o.maxWidth = maxWidth;
      const r = pele.render(source, o);
      return JSON.stringify({ Ok: { svg: r.svg, width: r.width, height: r.height, type: r.type, links: r.links } });
    } catch (e) {
      // Only the renderer's own refusals carry a code; anything else (a stack
      // or memory limit, a renderer bug) is an engine failure.
      if (!(e && e.code)) throw e;
      return JSON.stringify({ Err: { code: String(e.code), line: e.line | 0, message: String(e.message) } });
    }
  };
})(__OPTIONS__);
"#;

/// The whole pin line, version and bundle digest, for content addresses.
pub(super) fn pin() -> &'static str {
    PIN.trim()
}

#[cfg(test)]
pub(super) fn pele_version() -> &'static str {
    PIN.split_whitespace().next().unwrap_or("")
}

#[cfg(test)]
pub(super) fn pinned_sha256() -> &'static str {
    PIN.split_whitespace().nth(1).unwrap_or("")
}

pub(super) fn bundle_source() -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    brotli::BrotliDecompress(&mut std::io::Cursor::new(BUNDLE_BROTLI), &mut out)?;
    Ok(out)
}

/// One drawing as the renderer returned it.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
pub(crate) struct Drawn {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    #[serde(rename = "type")]
    pub kind: String,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
pub(crate) struct Link {
    pub id: String,
    pub href: String,
    pub internal: bool,
}

/// The renderer's refusal of a source: `code` is pele's (`syntax`,
/// `semantic`, `limit`, `unsupported-diagram`), `line` is 1-based within the
/// diagram, 0 when unknown.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub(crate) struct Refusal {
    pub code: String,
    pub line: u32,
    pub message: String,
}

/// Why the renderer drew nothing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EngineError {
    Refused(Refusal),
    /// The render ran past [`BUDGET`].
    TimedOut,
    /// The engine could not be reached or failed outside the renderer.
    Engine(String),
}

enum Job {
    Render { source: String, id_prefix: String, max_width: u32 },
    #[cfg(test)]
    Eval(String),
}

type Answer = Result<String, EngineError>;

static WORKER: OnceLock<Sender<(Job, SyncSender<Answer>)>> = OnceLock::new();

fn ask(job: Job) -> Answer {
    let (reply, answer) = sync_channel(1);
    WORKER.get_or_init(spawn).send((job, reply)).map_err(|_| EngineError::Engine("renderer thread is gone".into()))?;
    answer.recv().map_err(|_| EngineError::Engine("renderer thread is gone".into()))?
}

/// Draws `source`. `max_width` 0 draws the renderer's natural (wide) layout.
pub(crate) fn render(source: &str, id_prefix: &str, max_width: u32) -> Result<Drawn, EngineError> {
    let json = ask(Job::Render { source: source.to_string(), id_prefix: id_prefix.to_string(), max_width })?;
    match serde_json::from_str::<Result<Drawn, Refusal>>(&json) {
        Ok(drawn) => drawn.map_err(EngineError::Refused),
        Err(e) => Err(EngineError::Engine(format!("unreadable renderer reply: {e}"))),
    }
}

/// Evaluates raw script on the renderer's engine, for the limit tests.
#[cfg(test)]
pub(crate) fn eval(script: &str) -> Answer {
    ask(Job::Eval(script.to_string()))
}

fn spawn() -> Sender<(Job, SyncSender<Answer>)> {
    let (tx, rx) = channel::<(Job, SyncSender<Answer>)>();
    let started = std::thread::Builder::new()
        .name("moss-diagrams".into())
        .stack_size(THREAD_STACK)
        .spawn(move || {
            let mut engine = Engine::start();
            for (job, reply) in rx {
                let answer = match &mut engine {
                    Ok(engine) => engine.run(job),
                    Err(e) => Err(e.clone()),
                };
                // A render stopped part-way (budget, memory, stack) may leave
                // the renderer's own state half-written; start clean.
                if answer.is_err() {
                    engine = Engine::start();
                }
                let _ = reply.send(answer);
            }
        });
    if let Err(e) = started {
        log::warn!("could not start the diagram renderer thread: {e}");
    }
    tx
}

struct Engine {
    context: Context,
    deadline: Rc<Cell<Option<Instant>>>,
}

impl Engine {
    fn start() -> Result<Self, EngineError> {
        let fail = |what: &str, e: rquickjs::Error| EngineError::Engine(format!("{what}: {e}"));
        let runtime = Runtime::new().map_err(|e| fail("start QuickJS", e))?;
        runtime.set_memory_limit(ENGINE_MEMORY);
        runtime.set_max_stack_size(ENGINE_STACK);
        let deadline = Rc::new(Cell::new(None::<Instant>));
        let watch = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || watch.get().is_some_and(|t| Instant::now() > t))));
        let context = Context::full(&runtime).map_err(|e| fail("create a context", e))?;
        let bundle = bundle_source().map_err(|e| EngineError::Engine(format!("unpack pele: {e}")))?;
        let mut source = format!("/*\n{NOTICE}*/\n").into_bytes();
        source.extend(bundle);
        let glue = GLUE.replace("__OPTIONS__", &options());
        context.with(|ctx| {
            // The renderer normalises web links through `URL`; without it, it
            // drops them.
            crate::engine::url_shim::install_url(&ctx).map_err(|e| fail("install URL", e))?;
            ctx.eval::<(), _>(DATES_IN_UTC).map_err(|e| fail("pin dates to UTC", e))?;
            ctx.eval::<(), _>(source).map_err(|e| fail("load pele", e))?;
            ctx.eval::<(), _>(glue).map_err(|e| fail("load the render glue", e))
        })?;
        Ok(Engine { context, deadline })
    }

    fn run(&mut self, job: Job) -> Answer {
        self.deadline.set(Some(Instant::now() + BUDGET));
        let result = self.context.with(|ctx| {
            let result = match job {
                Job::Render { source, id_prefix, max_width } => ctx
                    .globals()
                    .get::<_, Function>("__moss_render")
                    .and_then(|render| render.call::<_, String>((source, id_prefix, max_width))),
                #[cfg(test)]
                Job::Eval(script) => ctx.eval::<rquickjs::Coerced<String>, _>(script).map(|s| s.0),
            };
            // Take the pending exception so the next job starts clean.
            result.map_err(|e| if e.is_exception() { format!("{e}: {:?}", ctx.catch()) } else { e.to_string() })
        });
        let timed_out = self.deadline.get().is_some_and(|t| Instant::now() > t);
        self.deadline.set(None);
        result.map_err(|e| if timed_out { EngineError::TimedOut } else { EngineError::Engine(e) })
    }
}

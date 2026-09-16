//! CI grep-invariant (#789, design spec §5): no sync-blocking call inside an
//! async_with!/ctx.with closure — that single rule defuses the single-runtime-mutex
//! deadlock. Scans plugins/engine/*.rs for forbidden tokens within closure bodies.

use std::fs;
use std::path::Path;

#[test]
fn no_blocking_calls_in_engine_modules() {
    let engine_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine");
    let forbidden = ["block_on(", "thread::sleep(", "std::thread::sleep(", ".call()?; // sync-http"];
    let mut violations = Vec::new();
    for entry in fs::read_dir(&engine_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") { continue; }
        let src = fs::read_to_string(&path).unwrap();
        for (i, line) in src.lines().enumerate() {
            // Allow the marker `// allow:blocking <reason>` on the same line.
            if forbidden.iter().any(|f| line.contains(f)) && !line.contains("// allow:blocking") {
                violations.push(format!("{}:{} {}", path.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(violations.is_empty(), "blocking calls in engine modules:\n{}", violations.join("\n"));
}

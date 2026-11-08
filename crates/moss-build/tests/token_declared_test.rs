//! A bare `var(--moss-x)` (no fallback) must name a property moss declares.
//! An undeclared one makes the whole declaration invalid at computed-value
//! time and the browser paints nothing, with no warning anywhere.
//!
//! A property set at runtime on the element itself, read by a rule gated on
//! the same hook, may be read bare when its file says so, once per property:
//! `/* allow:runtime-token --moss-cover-color set inline by grid_cells.rs */`.

use regex::Regex;
use std::collections::HashSet;
use std::path::Path;

#[test]
fn no_bare_var_names_an_undeclared_property() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let roots = [
        crates.join("moss-build/src/assets"),
        crates.join("moss-core/src/contract"),
    ];
    let comment = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let marker = Regex::new(r"allow:runtime-token\s+(--moss-[a-zA-Z0-9-]+)\s+\S").unwrap();
    let declaration = Regex::new(r"(--moss-[a-zA-Z0-9-]+)\s*:").unwrap();
    let usage = Regex::new(r"var\(\s*(--moss-[a-zA-Z0-9-]+)\s*(,)?").unwrap();

    let mut declared: HashSet<String> = moss_core::contract::tokens::load_tokens()
        .unwrap()
        .groups
        .into_iter()
        .flat_map(|g| g.entries)
        .map(|e| format!("--{}", e.name))
        .collect();
    let mut bare = Vec::new();
    let files = roots
        .iter()
        .flat_map(|r| walkdir::WalkDir::new(r))
        .map(|e| e.unwrap());
    for entry in files.filter(|e| e.file_type().is_file()) {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !["css", "ts", "js", "html", "rs", "json"].contains(&ext) {
            continue;
        }
        let raw = std::fs::read_to_string(path).unwrap();
        let allowed: Vec<&str> = marker
            .captures_iter(&raw)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        // Blank comments, keeping line numbers, so prose naming a property is not a use.
        let src = comment.replace_all(&raw, |c: &regex::Captures| {
            c[0].replace(|ch| ch != '\n', " ")
        });
        // Tests name properties their own fixtures declare; custom_props.rs documents `--moss-foo`.
        let name = path.to_string_lossy();
        let exempt = [
            "__tests__",
            "/tests/",
            ".test.ts",
            ".spec.ts",
            "_tests.rs",
            "custom_props.rs",
        ]
        .iter()
        .any(|p| name.contains(p));
        for (i, line) in src.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            declared.extend(declaration.captures_iter(line).map(|c| c[1].to_string()));
            for c in usage
                .captures_iter(line)
                .filter(|c| c.get(2).is_none() && !exempt)
            {
                if !allowed.contains(&&c[1]) {
                    bare.push((c[1].to_string(), format!("{}:{}", path.display(), i + 1)));
                }
            }
        }
    }
    assert!(
        bare.len() > 100,
        "the walk found too little source to trust: {}",
        bare.len()
    );
    let dead: Vec<_> = bare
        .into_iter()
        .filter(|(name, _)| !declared.contains(name))
        .collect();
    assert!(dead.is_empty(), "bare var() of undeclared properties paints nothing: {dead:#?}\nDeclare it, give it a fallback, or mark it allow:runtime-token.");
}

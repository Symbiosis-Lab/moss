use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::engine::{self, EngineError};
use crate::i18n::Language;

fn corpus(kind: &str) -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mermaid-corpus").join(kind);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "mmd"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap()))
        .collect()
}

#[test]
fn the_embedded_bundle_is_the_pinned_release() {
    let digest = format!("{:x}", Sha256::digest(engine::bundle_source().unwrap()));
    assert_eq!(digest, engine::pinned_sha256(), "data/pele/ was changed without scripts/pele/update.mjs");
    let header = String::from_utf8_lossy(&engine::bundle_source().unwrap()[..80]).into_owned();
    assert!(header.contains(&format!("Pele {}", engine::pele_version())), "{header}");
}

#[test]
fn every_corpus_type_draws() {
    let valid = corpus("valid");
    assert_eq!(valid.len(), 33);
    for (name, source) in valid {
        let drawn = engine::render(&source, "d0w", 0).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(drawn.svg.starts_with("<svg") && drawn.width > 0.0, "{name}");
    }
}

#[test]
fn a_malformed_diagram_is_refused_with_its_line() {
    let malformed = corpus("malformed");
    assert!(malformed.len() >= 10);
    for (name, source) in &malformed {
        match engine::render(source, "d0w", 0) {
            Err(EngineError::Refused(_)) => {}
            other => panic!("{name}: {other:?}"),
        }
    }
    let Err(EngineError::Refused(engine::Refusal { code, line, .. })) = engine::render("flowchart TD\n  A --> B\n  B -->\n", "d0w", 0) else {
        panic!("a dangling arrow must not draw");
    };
    assert_eq!((code.as_str(), line), ("syntax", 3));
}

#[test]
fn the_narrow_layout_honours_its_width() {
    let source = "flowchart LR\n  A[Write] --> B[Build] --> C[Publish] --> D[Read] --> E[Share]\n";
    let wide = engine::render(source, "d0w", 0).unwrap();
    let narrow = engine::render(source, "d0n", 360).unwrap();
    assert!(wide.width > 360.0 && narrow.width <= 360.0, "{} {}", wide.width, narrow.width);
}

#[test]
fn a_render_past_its_budget_stops_and_the_engine_carries_on() {
    assert_eq!(engine::eval("for (;;) {}"), Err(EngineError::TimedOut));
    assert!(engine::render("pie\n  \"a\" : 1\n", "d0w", 0).is_ok());
}

#[test]
fn a_deep_recursion_is_refused_not_a_crash() {
    match engine::eval("const f = (n) => n === 0 ? 0 : 1 + f(n - 1); f(1e7)") {
        Err(EngineError::Engine(message)) => assert!(message.contains("stack"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert!(engine::render("pie\n  \"a\" : 1\n", "d0w", 0).is_ok());
}

#[test]
fn an_allocation_past_the_memory_limit_is_refused() {
    assert!(matches!(engine::eval("const a = []; for (;;) a.push(new Array(1e6).fill(1))"), Err(EngineError::Engine(_))));
    assert!(engine::render("pie\n  \"a\" : 1\n", "d0w", 0).is_ok());
}

#[test]
fn the_edge_limit_is_moss_own() {
    let mut source = String::from("flowchart TD\n");
    for i in 0..2_100 {
        source.push_str(&format!("  n{i} --> n{}\n", i + 1));
    }
    let started = std::time::Instant::now();
    match engine::render(&source, "d0w", 0) {
        Err(EngineError::Refused(r)) => assert_eq!(r.code, "limit", "{}", r.message),
        other => panic!("{:?} after {:?}", other.map(|d| d.width), started.elapsed()),
    }
}

#[test]
fn every_drawing_of_the_corpus_passes_the_allowlist() {
    for kind in ["valid", "adversarial"] {
        for (name, source) in corpus(kind) {
            for (prefix, width) in [("d0w", 0), ("d0n", 360)] {
                match engine::render(&source, prefix, width) {
                    Ok(drawn) => {
                        if let Err(why) = super::sanitize::check(&drawn.svg, prefix) {
                            panic!("{kind}/{name} at {width}: {why}");
                        }
                    }
                    Err(EngineError::Refused(_)) if kind == "adversarial" => {}
                    Err(e) => panic!("{kind}/{name}: {e:?}"),
                }
            }
        }
    }
}

#[test]
fn the_shared_allowlist_samples_hold() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mermaid-corpus/allowlist-samples.json");
    let samples: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let wrap = samples["wrap"].as_str().unwrap();
    let prefix = samples["prefix"].as_str().unwrap();
    let list = |key: &str| samples[key].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>();
    for bad in list("refused") {
        assert!(super::sanitize::check(&wrap.replace("{}", &bad), prefix).is_err(), "accepted {bad}");
    }
    for doc in list("refused_documents") {
        assert!(super::sanitize::check(&doc, "").is_err(), "accepted {doc}");
    }
    for fine in list("accepted") {
        assert_eq!(super::sanitize::check(&wrap.replace("{}", &fine), prefix), Ok(()), "{fine}");
    }
}

fn figure(source: &str) -> String {
    super::render(source, "dt", Some(10), Language::En, None).unwrap_or_else(|e| panic!("{e:?}")).html
}

#[test]
fn a_wide_diagram_carries_both_layouts_each_with_its_width_rule() {
    let html = figure("flowchart LR\n  A[Write markdown] --> B[Build the site] --> C[Publish it] --> D[Read it]\n");
    assert!(html.starts_with("<figure class=\"moss-diagram\" data-narrow tabindex=\"0\" data-source-line=\"10\">"), "{html}");
    assert_eq!(html.matches("style=\"--diagram-w:").count(), 2, "{html}");
    assert!(html.contains("<div class=\"moss-diagram-narrow\" data-pagefind-ignore=\"\" style="), "{html}");
    assert!(html.contains("aria-label=\"Flowchart\""), "named by its type without accTitle");
}

#[test]
fn a_narrow_diagram_has_one_layout() {
    let html = figure("pie\n  \"a\" : 1\n");
    assert!(!html.contains("data-narrow") && !html.contains("moss-diagram-narrow"), "{html}");
    assert!(html.contains("aria-label=\"Pie chart\""), "{html}");
}

#[test]
fn a_drawing_is_named_by_its_accessible_title_or_front_matter_title() {
    let titled = figure("flowchart TD\n  accTitle: The flow\n  A --> B\n");
    assert!(titled.contains("aria-labelledby=") && !titled.contains("aria-label=\""), "{titled}");
    let front = figure("---\ntitle: \"Two \\\"states\\\"\"\n---\nstateDiagram-v2\n  [*] --> A\n");
    assert!(front.contains("aria-label=\"Two &quot;states&quot;\""), "{front}");
}

#[test]
fn the_width_rule_floors_the_smallest_text_at_eleven_px() {
    let drawn = engine::render("packet-beta\n  0-15: \"Source\"\n  16-31: \"Destination\"\n", "dtw", 0).unwrap();
    let html = super::render("packet-beta\n  0-15: \"Source\"\n  16-31: \"Destination\"\n", "dt", None, Language::En, None).unwrap().html;
    // The packet diagram draws 10px text, which moss never shrinks.
    assert!(drawn.svg.contains("font-size=\"10\""));
    assert!(html.contains(&format!("--diagram-w:{w}px;--diagram-min:{w}px", w = drawn.width)), "{html}");
}

#[test]
fn a_refused_diagram_shows_its_source_and_a_plain_reason() {
    let reason = |source: &str| super::render(source, "dt", None, Language::En, None).unwrap_err().reason(Language::En);
    assert_eq!(reason("flowchart TD\n  A --> B\n  B -->\n"), "line 3: syntax error near “B -->”");
    assert_eq!(reason("gantt\n  dateFormat YYYY-MM-DD\n  task : 2024-13-45, 3d\n"), "Invalid date:2024-13-45");
    assert_eq!(reason("%% only a comment\n"), "it is empty");
    assert_eq!(reason("sparkline\n  1 2 3\n"), "moss does not draw “sparkline” diagrams");
    assert_eq!(reason(&"x".repeat(50_001)), "Diagram source is longer than the limit of 50000 characters.");

    let refusal = super::render("flowchart TD\n  A -->\n", "dt", None, Language::En, None).unwrap_err();
    let html = super::refused_html(&refusal, Language::En, "<pre tabindex=\"0\"><code class=\"language-mermaid\">a &lt; b</code></pre>\n");
    assert_eq!(
        html,
        "<figure class=\"moss-diagram moss-diagram-error\"><figcaption class=\"moss-diagram-message\">This diagram could not be drawn: line 2: syntax error near “A --&gt;”.</figcaption><pre tabindex=\"0\"><code class=\"language-mermaid\">a &lt; b</code></pre></figure>\n"
    );
}

#[test]
fn a_repeated_diagram_on_a_page_gets_its_own_ids() {
    let page = super::PageDiagrams::new("p.md", Language::En, None);
    let mut out = String::new();
    let source = "flowchart LR\n  accTitle: Twice\n  A --> B\n";
    page.render(&mut out, source, None, "");
    page.render(&mut out, source, None, "");
    let ids: Vec<&str> = out.split(" id=\"").skip(1).map(|r| r.split('"').next().unwrap()).collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert!(!ids.is_empty() && unique.len() == ids.len(), "{ids:?}");
}

#[test]
fn a_web_link_on_a_node_is_kept_and_a_script_link_dropped() {
    let source = "flowchart LR\n  A --> B\n  click A \"javascript:alert(1)\"\n  click B \"HTTPS://Example.com/safe\"\n";
    let html = figure(source);
    assert_eq!(html.matches("<a ").count(), 1, "{html}");
    assert!(html.contains("<a href=\"https://example.com/safe\""), "{html}");
    assert!(!html.contains("javascript"), "{html}");
}

#[test]
fn the_warning_names_the_page_line_when_the_build_knows_it() {
    let source = "flowchart TD\n  A --> B\n  B -->\n";
    let refusal = super::render(source, "dt", None, Language::En, None).unwrap_err();
    assert_eq!(super::warning("a.md", source, Some(7), &refusal), "[a.md] line 10: diagram not drawn: syntax error near “B -->”");
    assert_eq!(super::warning("a.md", source, None, &refusal), "[a.md] diagram “flowchart TD” not drawn: line 3: syntax error near “B -->”");
    let unsafe_ = super::DiagramRefusal::Unsafe("element <script>".into());
    assert_eq!(super::warning("a.md", source, Some(7), &unsafe_), "[a.md] line 7: diagram not drawn: the drawing failed moss's safety check (element <script>)");
}

#[test]
fn a_note_link_points_at_its_page_or_is_dropped() {
    let source = "flowchart LR\n  A[Field notes] --> B[\"Q&A\"] --> C[Missing]\n  class A,B,C internal-link;\n";
    let notes = |name: &str| match name {
        "Field notes" => Some("../field-notes/".to_string()),
        "Q&A" => Some("../q-and-a/?x=1&y=2".to_string()),
        _ => None,
    };
    let figure = super::render(source, "dt", None, Language::En, Some(&notes)).unwrap();
    assert!(figure.html.contains("<a class=\"internal-link\" href=\"../field-notes/\""), "{}", figure.html);
    assert!(figure.html.contains("href=\"../q-and-a/?x=1&amp;y=2\""), "{}", figure.html);
    assert!(!figure.html.contains("href=\"Missing\""), "{}", figure.html);
    assert!(figure.html.contains(">Missing<"), "the node stays, unlinked");
    assert_eq!(figure.unresolved, ["Missing"]);
}

#[test]
fn a_gantt_chart_draws_the_same_bytes_in_every_time_zone() {
    // The golden was drawn by the same renderer under TZ=UTC; on a machine in
    // a zone with daylight saving the local-time arithmetic moves the bars.
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mermaid-corpus/goldens/gantt-across-dst.svg");
    let golden = std::fs::read_to_string(golden_path).unwrap();
    let source = "gantt\n  dateFormat YYYY-MM-DD\n  section Spring\n  Draft :a1, 2026-03-02, 10d\n  Review :after a1, 5d\n  Publish :milestone, 2026-03-20, 0d\n";
    assert_eq!(engine::render(source, "g", 0).unwrap().svg, golden);
}

#[test]
fn a_page_in_another_language_says_why_in_it() {
    let refusal = super::render("flowchart TD\n  A -->\n", "dt", None, Language::De, None).unwrap_err();
    assert_eq!(refusal.reason(Language::De), "Zeile 2: Syntaxfehler bei „A -->“");
    let html = super::refused_html(&refusal, Language::ZhHans, "");
    assert!(html.contains(">无法绘制此图表：第 2 行：“A --&gt;”附近有语法错误。<"), "{html}");
    assert!(figure_in("pie\n  \"a\" : 1\n", Language::ZhHant).contains("aria-label=\"圓餅圖\""));
}

fn figure_in(source: &str, lang: Language) -> String {
    super::render(source, "dt", None, lang, None).unwrap().html
}

#[test]
fn the_renderer_uses_no_local_time_view_the_utc_prelude_leaves_out() {
    // The prelude pins the local-time views pele uses today; one it does not
    // cover would make output depend on the machine's zone again, which a
    // test run in UTC would never notice.
    let bundle = String::from_utf8(engine::bundle_source().unwrap()).unwrap();
    for name in ["toLocale", "toDateString", "toTimeString", "Intl.", "getYear"] {
        assert!(!bundle.contains(name), "pele now uses {name}: extend DATES_IN_UTC in engine.rs");
    }
}

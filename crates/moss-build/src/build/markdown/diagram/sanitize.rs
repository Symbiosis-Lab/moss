//! The allowlist every drawing passes before it reaches a page.
//!
//! The renderer escapes text and drops unsafe links itself, but it releases
//! often and a regression there would ship silently, so the build parses each
//! SVG and accepts only a closed vocabulary of elements, attributes and
//! values. Anything else refuses the drawing — it is never repaired — and the
//! page shows the diagram's source instead. `tests/diagrams/svg-allowlist.mjs`
//! implements the same rule for checking built pages from outside; both are
//! tested against `tests/fixtures/mermaid-corpus/allowlist-samples.json`.

use roxmltree::{Document, Node, NodeType, ParsingOptions};

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

const ELEMENTS: &[&str] = &[
    "svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "tspan", "title", "desc",
    "defs", "linearGradient", "radialGradient", "stop", "marker", "clipPath", "a",
];

const ATTRIBUTES: &[&str] = &[
    // presentation
    "fill", "fill-opacity", "fill-rule", "stroke", "stroke-width", "stroke-dasharray", "stroke-dashoffset",
    "stroke-linecap", "stroke-linejoin", "stroke-opacity", "opacity", "transform", "font-size", "font-family",
    "font-weight", "font-style", "text-anchor", "dominant-baseline", "letter-spacing", "clip-path", "marker-end",
    "marker-start",
    // geometry
    "x", "y", "x1", "x2", "y1", "y2", "cx", "cy", "r", "rx", "ry", "width", "height", "d", "points", "viewBox",
    "offset", "stop-color", "stop-opacity", "gradientUnits", "gradientTransform", "markerWidth", "markerHeight",
    "refX", "refY", "orient", "markerUnits", "preserveAspectRatio", "dx", "dy", "textLength", "lengthAdjust",
    // structure
    "target", "class", "id", "role", "style", "href", "rel", "tabindex", "focusable",
];

const STYLE_PROPERTIES: &[&str] = &[
    "stroke-dasharray", "stroke-dashoffset", "stroke-opacity", "fill-opacity", "stroke-linecap", "stroke-linejoin",
    "max-width", "width", "height", "color", "fill", "stroke", "stroke-width", "font", "font-family", "font-size",
    "font-weight", "font-style", "letter-spacing", "word-spacing", "text-transform", "direction", "rx", "ry",
    "opacity",
];

/// Checks one drawing. `prefix` is the id prefix the drawing was rendered
/// with: every id it declares and every `url(#…)` it uses must carry it, so
/// two drawings on a page cannot reach into each other.
pub(crate) fn check(svg: &str, prefix: &str) -> Result<(), String> {
    // The check parses XML, but the page is read by an HTML parser. CDATA,
    // comments, doctypes and processing instructions are where the two
    // disagree, and the renderer emits none of them.
    if svg.contains("<!") || svg.contains("<?") {
        return Err("markup declaration".into());
    }
    let doc = Document::parse_with_options(svg, ParsingOptions { allow_dtd: false, nodes_limit: 2_000_000 })
        .map_err(|e| format!("not well-formed: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err("root is not <svg>".into());
    }
    root.descendants().try_for_each(|node| check_node(node, prefix))
}

fn check_node(node: Node<'_, '_>, prefix: &str) -> Result<(), String> {
    match node.node_type() {
        NodeType::Root | NodeType::Text => return Ok(()),
        NodeType::Element => {}
        other => return Err(format!("{other:?} node")),
    }
    let name = node.tag_name().name();
    if node.tag_name().namespace() != Some(SVG_NS) || !ELEMENTS.contains(&name) {
        return Err(format!("element <{name}>"));
    }
    for attr in node.attributes() {
        check_attribute(name, attr.namespace(), attr.name(), attr.value(), prefix)?;
    }
    Ok(())
}

fn check_attribute(element: &str, ns: Option<&str>, name: &str, value: &str, prefix: &str) -> Result<(), String> {
    match ns {
        None => {}
        Some(XML_NS) if name == "space" => return Ok(()),
        Some(ns) => return Err(format!("attribute {ns}:{name}")),
    }
    let open = name.starts_with("data-") || name.starts_with("aria-");
    if !open && !ATTRIBUTES.contains(&name) {
        return Err(format!("attribute {element}@{name}"));
    }
    match name {
        "href" if element != "a" => return Err(format!("href on <{element}>")),
        "href" if !link_allowed(value) => return Err(format!("link {}", truncate(value))),
        "target" if !matches!(value, "_blank" | "_self" | "_parent" | "_top") => return Err(format!("target {value}")),
        "id" if !value.starts_with(prefix) => return Err(format!("id {value} lacks the prefix {prefix}")),
        "style" => check_style(value)?,
        _ => {}
    }
    check_urls(value, prefix)
}

/// A link may name a web, mail or phone address, or a place on this site.
fn link_allowed(href: &str) -> bool {
    match href.split_once(':') {
        Some((scheme, _)) => super::engine::LINK_SCHEMES.iter().any(|s| scheme.eq_ignore_ascii_case(s)),
        None => !href.starts_with("//"),
    }
}

/// A link either is or is not an address, so only `href` and `url()` can
/// reach outside the drawing; label text in other attributes ("Data: sales")
/// is not checked for schemes. A CSS escape or `@import` could hide either
/// inside a style, so a style carries neither.
fn check_style(style: &str) -> Result<(), String> {
    let lower = style.to_ascii_lowercase();
    if ["\\", "@import", "expression("].iter().any(|bad| lower.contains(bad)) {
        return Err(format!("unsafe style {}", truncate(style)));
    }
    for declaration in style.split(';') {
        let property = declaration.split(':').next().unwrap_or("").trim();
        if !property.is_empty() && !property.starts_with("--") && !STYLE_PROPERTIES.contains(&property) {
            return Err(format!("style property {property}"));
        }
    }
    Ok(())
}

fn check_urls(value: &str, prefix: &str) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let mut offset = 0;
    while let Some(at) = rest.find("url(") {
        let start = offset + at + 4;
        let inner = value[start..].split(')').next().unwrap_or("");
        let target = inner.trim().trim_matches(|c| c == '\'' || c == '"');
        if !target.starts_with('#') || !target[1..].starts_with(prefix) {
            return Err(format!("url({}) leaves this drawing", truncate(target)));
        }
        offset = start;
        rest = &lower[offset..];
    }
    Ok(())
}

fn truncate(s: &str) -> String {
    s.chars().take(40).collect()
}

//! `[map.layers.<name>]`: the detail layers a site draws under its places, and
//! the frame its places need (what `moss map bbox` prints).
//!
//! Parsing is text-only like the rest of [`crate::config`]; the checks that
//! touch the folder (`source` must exist inside the site) live in
//! [`resolve_layers`], which the build calls once it knows the site root.

use std::path::{Component, Path};

use toml::Value;

use crate::vault::places::Gazetteer;

/// Deepest and shallowest zoom a layer may declare, as web-map zoom levels.
pub const MIN_MAX_ZOOM: i64 = 10;
pub const MAX_MAX_ZOOM: i64 = 19;
const DEFAULT_MAX_ZOOM: u8 = 16;

const KEYS: &str = "kind, source, bbox, max_zoom, budget, credit, annotations";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Historical,
    Streets,
}

impl LayerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Historical => "historical",
            Self::Streets => "streets",
        }
    }
}

/// One declared layer. `bbox` is `[west, south, east, north]` in degrees; it
/// is `None` only straight out of [`ConfigFile::map_layers`](crate::config::ConfigFile::map_layers),
/// and always `Some` after [`resolve_layers`].
#[derive(Debug, Clone, PartialEq)]
pub struct MapLayer {
    pub name: String,
    pub kind: LayerKind,
    pub source: String,
    pub bbox: Option<[f64; 4]>,
    pub max_zoom: u8,
    /// Tile budget in bytes.
    pub budget: Option<u64>,
    pub credit: Option<String>,
    /// The annotation addresses a fetch tool reads, in drawing order. The
    /// build reads only the folder, and uses these just to order plates.
    pub annotations: Vec<String>,
}

/// The layers under `[map]`, in the order the file writes them (`order` ranks
/// a layer name; unranked names sort last, by name).
pub(crate) fn parse_layers(map: Option<&Value>, order: impl Fn(&str) -> usize) -> Result<Vec<MapLayer>, String> {
    let Some(map) = map else { return Ok(Vec::new()) };
    let Some(map) = map.as_table() else { return Err("[map] must be a table".into()) };
    if let Some(key) = map.keys().find(|k| *k != "layers") {
        return Err(format!("[map] has an unknown key \"{key}\"; the only key is the [map.layers.<name>] tables"));
    }
    let Some(layers) = map.get("layers") else { return Ok(Vec::new()) };
    let Some(layers) = layers.as_table() else { return Err("[map.layers] must hold [map.layers.<name>] tables".into()) };
    let mut named: Vec<_> = layers.iter().collect();
    named.sort_by_key(|(name, _)| order(name));
    named.into_iter().map(|(name, value)| parse_layer(name, value)).collect()
}

fn parse_layer(name: &str, value: &Value) -> Result<MapLayer, String> {
    let table = format!("[map.layers.{name}]");
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("{table} the layer name may use only letters, digits, - and _; it names a folder of tiles"));
    }
    let Some(t) = value.as_table() else { return Err(format!("{table} must be a table")) };
    if let Some(key) = t.keys().find(|k| !KEYS.split(", ").any(|known| known == k.as_str())) {
        return Err(format!("{table} has an unknown key \"{key}\"; the keys are {KEYS}"));
    }
    let kind = match t.get("kind") {
        None => return Err(format!("{table} needs a `kind`: \"historical\" or \"streets\"")),
        Some(Value::String(s)) if s == "historical" => LayerKind::Historical,
        Some(Value::String(s)) if s == "streets" => LayerKind::Streets,
        Some(other) => return Err(format!("{table} `kind` must be \"historical\" or \"streets\", not {other}")),
    };
    let source = match t.get("source") {
        None => return Err(format!("{table} needs a `source`: a folder inside the site")),
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        Some(other) => return Err(format!("{table} `source` must be a folder path inside the site, not {other}")),
    };
    let bbox = t.get("bbox").map(|v| parse_bbox(&table, v)).transpose()?;
    let max_zoom = match t.get("max_zoom") {
        None => DEFAULT_MAX_ZOOM,
        Some(Value::Integer(z)) if (MIN_MAX_ZOOM..=MAX_MAX_ZOOM).contains(z) => *z as u8,
        Some(other) => return Err(format!("{table} `max_zoom` must be a whole number from {MIN_MAX_ZOOM} to {MAX_MAX_ZOOM}, not {other}")),
    };
    let budget = match t.get("budget") {
        None => None,
        Some(Value::String(s)) => Some(parse_byte_size(s).ok_or_else(|| format!("{table} `budget` must be a size such as \"40 MB\" or \"500 KB\", not \"{s}\""))?),
        Some(other) => return Err(format!("{table} `budget` must be a size such as \"40 MB\" or \"500 KB\", not {other}")),
    };
    let credit = match t.get("credit") {
        None => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => return Err(format!("{table} `credit` must be text, not {other}")),
    };
    let annotations = match t.get("annotations") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("{table} `annotations` must be a list of addresses, not {v}")))
            .collect::<Result<_, _>>()?,
        Some(other) => return Err(format!("{table} `annotations` must be a list of addresses, not {other}")),
    };
    Ok(MapLayer { name: name.to_string(), kind, source, bbox, max_zoom, budget, credit, annotations })
}

fn parse_bbox(table: &str, value: &Value) -> Result<[f64; 4], String> {
    let bad = || format!("{table} `bbox` must be [west, south, east, north] in degrees, west below east and south below north, not {value}");
    let items = value.as_array().filter(|a| a.len() == 4).ok_or_else(bad)?;
    let mut n = [0.0; 4];
    for (slot, item) in n.iter_mut().zip(items) {
        *slot = match item {
            Value::Float(f) => *f,
            Value::Integer(i) => *i as f64,
            _ => return Err(bad()),
        };
    }
    let [w, s, e, nn] = n;
    let in_range = (-180.0..=180.0).contains(&w) && (-180.0..=180.0).contains(&e) && (-90.0..=90.0).contains(&s) && (-90.0..=90.0).contains(&nn);
    if !in_range || w >= e || s >= nn {
        return Err(bad());
    }
    Ok(n)
}

/// `"40 MB"`, `"500 KB"`, `"1.5 GB"`, `"2048"` (bytes). KB, MB and GB count
/// thousands; KiB, MiB and GiB count 1024s. `None` for anything else.
pub fn parse_byte_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok().filter(|n: &f64| n.is_finite())?;
    let unit = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * unit).round() as u64)
}

/// The parsed layers, each `source` checked against the site folder and each
/// missing `bbox` filled from the gazetteer's places. An error names the table
/// and the key, and fails the build.
pub fn resolve_layers(mut layers: Vec<MapLayer>, site_root: &Path, gazetteer: &Gazetteer) -> Result<Vec<MapLayer>, String> {
    for layer in &mut layers {
        check_source(layer, site_root)?;
        if layer.bbox.is_none() {
            let points: Vec<(f64, f64)> = gazetteer.iter().filter_map(|(_, record)| record.coords).collect();
            let frame = frame_for(&points).ok_or_else(|| {
                format!("[map.layers.{}] has no `bbox` and .moss/places.toml has no place with coordinates to derive one from", layer.name)
            })?;
            layer.bbox = Some(frame.bbox);
        }
    }
    Ok(layers)
}

/// The declared layers' `source` folders as site-relative `/` paths, read
/// straight from `.moss/config.toml`. The scan leaves these folders out of the
/// site (the layer warper still reads them); a config that cannot be read or
/// parsed declares none, and the build reports it where it uses the config.
pub fn declared_sources(site_root: &Path) -> Vec<String> {
    let config = crate::moss_paths::MossPaths::new(site_root).config();
    let Ok(Some(text)) = crate::build::site_config::read_managed_toml_no_wait(&config) else { return Vec::new() };
    let Ok(parsed) = crate::config::ConfigFile::parse(&text) else { return Vec::new() };
    let layers = parsed.map_layers().unwrap_or_default();
    layers
        .iter()
        .filter_map(|layer| {
            let parts: Vec<_> = Path::new(&layer.source)
                .components()
                .filter_map(|c| match c {
                    Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join("/"))
        })
        .collect()
}

fn check_source(layer: &MapLayer, site_root: &Path) -> Result<(), String> {
    let table = format!("[map.layers.{}]", layer.name);
    let relative = Path::new(&layer.source);
    if relative.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(format!("{table} `source` \"{}\" must be a folder inside the site, written relative to it", layer.source));
    }
    let path = site_root.join(relative);
    let resolved = path.canonicalize().map_err(|_| format!("{table} `source` \"{}\" is not a folder in the site", layer.source))?;
    let root = site_root.canonicalize().map_err(|e| format!("{table} cannot read the site folder: {e}"))?;
    if !resolved.starts_with(&root) || !resolved.is_dir() {
        return Err(format!("{table} `source` \"{}\" must be a folder inside the site", layer.source));
    }
    if resolved == root {
        return Err(format!("{table} `source` \"{}\" is the whole site; point it at the folder holding only the layer's plates", layer.source));
    }
    Ok(())
}

/// The frame a set of places needs and a suggested deepest zoom.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// `[west, south, east, north]` in degrees, rounded outward to 1/10000 degree.
    pub bbox: [f64; 4],
    pub max_zoom: u8,
}

/// The frame around `points` (`(lat, lng)`), padded by 10% of each side and at
/// least 0.01 degree. The zoom is the one at which the frame's longer side
/// (in ground distance, so east-west is scaled by the cosine of the latitude)
/// spans about 1024 px, plus 3, kept within the range a layer may declare.
/// Degrees per pixel at zoom z is 360 / (256 * 2^z). `None` for no points.
pub fn frame_for(points: &[(f64, f64)]) -> Option<Frame> {
    let first = points.first()?;
    let (mut s, mut n, mut w, mut e) = (first.0, first.0, first.1, first.1);
    for &(lat, lng) in points {
        s = s.min(lat);
        n = n.max(lat);
        w = w.min(lng);
        e = e.max(lng);
    }
    let pad_lat = ((n - s) * 0.1).max(0.01);
    let pad_lng = ((e - w) * 0.1).max(0.01);
    let floor4 = |x: f64| (x * 1e4).floor() / 1e4;
    let ceil4 = |x: f64| (x * 1e4).ceil() / 1e4;
    let bbox = [
        floor4((w - pad_lng).max(-180.0)),
        floor4((s - pad_lat).max(-90.0)),
        ceil4((e + pad_lng).min(180.0)),
        ceil4((n + pad_lat).min(90.0)),
    ];
    let mid_lat = (bbox[1] + bbox[3]) / 2.0;
    let longer = ((bbox[2] - bbox[0]) * mid_lat.to_radians().cos()).max(bbox[3] - bbox[1]);
    let zoom = (1440.0 / longer).log2() + 3.0;
    let max_zoom = zoom.round().clamp(MIN_MAX_ZOOM as f64, MAX_MAX_ZOOM as f64) as u8;
    Some(Frame { bbox, max_zoom })
}

#[cfg(test)]
#[path = "map_layers_tests.rs"]
mod tests;

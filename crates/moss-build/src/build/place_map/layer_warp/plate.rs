//! One georeferenced plate: the three files a historical layer's folder holds
//! (`<id>.annotation.json`, the image, `<id>.region.json`), read and fitted.
//! A plate that cannot be used is a warning naming it, never a failed build.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::fit::{self, ground_metres, mercator, unmercator, Poly};
use crate::build::assets::paths::compute_binary_hash;
use crate::build::cli_output::{log_note, log_warn_problem};
use crate::build::place_map::world_coords;

/// A plate ready to draw: both fits, the mask and the footprint.
#[derive(Debug)]
pub(crate) struct Plate {
    pub id: String,
    pub image_path: PathBuf,
    /// `[x, y, w, h]` of the fetched image in full-image pixels.
    pub region: [f64; 4],
    /// Fetched pixels in the image's two directions per full-image pixel.
    pub pixels_per_unit: (f64, f64),
    /// The selector polygon in full-image pixels.
    pub mask: Vec<(f64, f64)>,
    /// Web Mercator to full-image pixel.
    pub to_pixel: Poly,
    /// The selector polygon in world coordinates, closed.
    pub footprint: Vec<(f64, f64)>,
    pub rms_metres: f64,
    pub source: String,
    pub rights: String,
    /// Who made the georeference, when `region.json` says.
    pub georeferenced_by: Option<String>,
    pub credit: String,
    pub title: Option<String>,
    /// Content hash of the plate's three files.
    pub digest: String,
}

impl Plate {
    /// `[x0, y0, x1, y1]` of the footprint in world coordinates.
    pub(crate) fn footprint_world(&self) -> [f64; 4] {
        self.footprint.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, &(x, y)| [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)])
    }

    /// Whether the world point has pixels: inside the mask and the fetched region.
    pub(crate) fn has_pixels_at(&self, x: f64, y: f64) -> bool {
        let (mx, my) = mercator(world_coords::unproject_latitude(y), world_coords::unproject_longitude(x));
        let (u, v) = self.to_pixel.eval(mx, my);
        let r = self.region;
        (r[0]..r[0] + r[2]).contains(&u) && (r[1]..r[1] + r[3]).contains(&v) && super::tiles::inside(&self.mask, u, v)
    }

    /// `[west, south, east, north]` of the footprint in degrees.
    pub(crate) fn footprint_degrees(&self) -> [f64; 4] {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for &(x, y) in &self.footprint {
            let (lat, lng) = (world_coords::unproject_latitude(y), world_coords::unproject_longitude(x));
            b = [b[0].min(lng), b[1].min(lat), b[2].max(lng), b[3].max(lat)];
        }
        b
    }
}

/// Every usable plate in `folder`, by id order.
pub(crate) fn load_plates(folder: &Path) -> Vec<Plate> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut ids: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".annotation.json").or_else(|| n.strip_suffix(".region.json")).map(str::to_string))
        .collect();
    ids.sort();
    ids.dedup();
    ids.iter()
        .filter_map(|id| match load_plate(folder, id) {
            Ok(plate) => Some(plate),
            Err(why) => {
                log_warn_problem!("map plate {id} skipped: {why}");
                None
            }
        })
        .collect()
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{} cannot be read ({e})", path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())))
}

fn json(path: &Path) -> Result<(Value, Vec<u8>), String> {
    let bytes = read(path)?;
    let value = serde_json::from_slice(&bytes).map_err(|e| format!("{} is not valid JSON ({e})", path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())))?;
    Ok((value, bytes))
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

fn pair(v: &Value) -> Option<(f64, f64)> {
    let a = v.as_array()?;
    Some((number(a.first()?)?, number(a.get(1)?)?))
}

/// The first string of a IIIF language map (`{"none": ["text"]}`) or a bare string.
fn label(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => map.values().find_map(|l| l.as_array()?.first()?.as_str().map(str::to_string)),
        _ => None,
    }
    .filter(|s| !s.trim().is_empty())
}

fn selector_polygon(annotation: &Value) -> Result<Vec<(f64, f64)>, String> {
    let svg = annotation["target"]["selector"]["value"].as_str().ok_or("the annotation has no SvgSelector")?;
    let after = svg.split("points=").nth(1).ok_or("the SvgSelector has no polygon")?;
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'').ok_or("the SvgSelector polygon is malformed")?;
    let list = after[1..].split(quote).next().unwrap_or("");
    let numbers: Vec<f64> = list.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(str::parse).collect::<Result<_, _>>().map_err(|_| "the SvgSelector polygon has a bad number")?;
    if numbers.len() < 6 || numbers.len() % 2 != 0 {
        return Err("the SvgSelector polygon needs at least three points".into());
    }
    Ok(numbers.chunks(2).map(|c| (c[0], c[1])).collect())
}

fn control_points(annotation: &Value) -> Vec<((f64, f64), (f64, f64))> {
    annotation["body"]["features"]
        .as_array()
        .map(|f| f.iter().filter_map(|f| Some((pair(&f["properties"]["resourceCoords"])?, pair(&f["geometry"]["coordinates"])?))).collect())
        .unwrap_or_default()
}

fn requested_order(id: &str, annotation: &Value) -> u8 {
    let t = &annotation["body"]["transformation"];
    match t["type"].as_str() {
        None | Some("polynomial") => t["options"]["order"].as_u64().filter(|o| (1..=3).contains(o)).unwrap_or(1) as u8,
        Some(other) => {
            log_note!("map plate {id} asks for a {other} transformation, which moss does not draw yet; using a first-order polynomial");
            1
        }
    }
}

const CHANGED_IMAGE: &str = "the library's image has changed since the annotation was made; re-georeference it, e.g. in the Allmaps Editor";

/// A plate is refused when its fetched image cannot be the `region` it names at `scale`, or when that region is not inside the image the annotation was made on: the control points are pixels of the old image, so a re-cut image would be warped to the wrong place with a residual that looks fine.
fn check_image_fits_annotation(annotation: &Value, region: [f64; 4], scale: f64, image: (u32, u32)) -> Result<(), String> {
    let source = &annotation["target"]["source"];
    if let (Some(w), Some(h)) = (number(&source["width"]), number(&source["height"])) {
        if region[0] < 0.0 || region[1] < 0.0 || region[0] + region[2] > w + 1.0 || region[1] + region[3] > h + 1.0 {
            return Err(format!("region [{}, {}, {}, {}] lies outside the annotation's own image size {w}x{h}; {CHANGED_IMAGE}", region[0], region[1], region[2], region[3]));
        }
    }
    let expected = ((region[2] * scale).round(), (region[3] * scale).round());
    let (actual_w, actual_h) = (f64::from(image.0), f64::from(image.1));
    if (actual_w - expected.0).abs() > 1.0 || (actual_h - expected.1).abs() > 1.0 {
        return Err(format!("the image is {actual_w}x{actual_h} px, but region {}x{} at scale {scale} should give {}x{}; {CHANGED_IMAGE}", region[2], region[3], expected.0, expected.1));
    }
    Ok(())
}

pub(super) fn load_plate(folder: &Path, id: &str) -> Result<Plate, String> {
    let (annotation, annotation_bytes) = json(&folder.join(format!("{id}.annotation.json")))?;
    let (region, region_bytes) = json(&folder.join(format!("{id}.region.json")))?;
    if region["version"].as_u64().is_some_and(|v| v != 1) {
        return Err("region.json has an unknown version".into());
    }
    let image_name = region["image"].as_str().map_or_else(|| format!("{id}.jpg"), str::to_string);
    if image_name.contains(['/', '\\']) || image_name.starts_with('.') {
        return Err("region.json `image` must be a file name in the same folder".into());
    }
    let image_path = folder.join(&image_name);
    let image_bytes = read(&image_path)?;
    let (iw, ih) = image::ImageReader::new(std::io::Cursor::new(&image_bytes))
        .with_guessed_format()
        .map_err(|e| format!("{image_name} is not an image ({e})"))?
        .into_dimensions()
        .map_err(|e| format!("{image_name} is not a readable image ({e})"))?;
    let r = region["region"].as_array().filter(|a| a.len() == 4).ok_or("region.json `region` must be [x, y, w, h]")?;
    let region_box: Vec<f64> = r.iter().filter_map(number).collect();
    if region_box.len() != 4 || region_box[2] <= 0.0 || region_box[3] <= 0.0 {
        return Err("region.json `region` must be [x, y, w, h] with a positive size".into());
    }
    let region_box = [region_box[0], region_box[1], region_box[2], region_box[3]];
    let Some(scale) = region["scale"].as_f64().filter(|s| *s > 0.0 && s.is_finite()) else {
        return Err("region.json `scale` must be a positive number".into());
    };
    check_image_fits_annotation(&annotation, region_box, scale, (iw, ih))?;

    let mask = selector_polygon(&annotation)?;
    let points = control_points(&annotation);
    let mut order = requested_order(id, &annotation);
    while order > 1 && points.len() < fit::terms_needed(order) {
        order -= 1;
        log_note!("map plate {id} has only {} control points; fitting a polynomial of order {order}", points.len());
    }
    if points.len() < fit::terms_needed(1) {
        return Err(format!("it has {} usable control points; at least 3 are needed", points.len()));
    }
    let merc: Vec<(f64, f64)> = points.iter().map(|&(_, (lng, lat))| mercator(lat, lng)).collect();
    let pixels: Vec<(f64, f64)> = points.iter().map(|&(px, _)| px).collect();
    let to_pixel = fit::fit(order, &merc, &pixels).ok_or("its control points do not determine a fit")?;
    let to_geo = fit::fit(order, &pixels, &merc).ok_or("its control points do not determine a fit")?;

    let squares: f64 = points
        .iter()
        .map(|&((u, v), (lng, lat))| {
            let (mx, my) = to_geo.eval(u, v);
            ground_metres(unmercator(mx, my), (lat, lng)).powi(2)
        })
        .sum();
    let rms_metres = (squares / points.len() as f64).sqrt();

    let mut footprint: Vec<(f64, f64)> = mask
        .iter()
        .map(|&(u, v)| {
            let (mx, my) = to_geo.eval(u, v);
            let (lat, lng) = unmercator(mx, my);
            world_coords::project(lat, lng)
        })
        .collect();
    footprint.push(footprint[0]);

    let text = |v: &Value| v.as_str().map(str::trim).unwrap_or("").to_string();
    let source = Some(text(&region["source"])).filter(|s| !s.is_empty()).or_else(|| annotation["target"]["source"]["id"].as_str().map(str::to_string)).unwrap_or_default();
    let credit = Some(text(&region["credit"]))
        .filter(|s| !s.is_empty())
        .or_else(|| annotation["target"]["source"]["provider"].as_array()?.first().and_then(|p| label(&p["label"])))
        .unwrap_or_default();
    let title = label(&annotation["target"]["source"]["label"]).or_else(|| label(&annotation["label"]));
    let mut all = annotation_bytes;
    all.extend_from_slice(&region_bytes);
    all.extend_from_slice(&image_bytes);
    Ok(Plate {
        id: id.to_string(),
        image_path,
        region: region_box,
        pixels_per_unit: (f64::from(iw) / region_box[2], f64::from(ih) / region_box[3]),
        mask,
        to_pixel,
        footprint,
        rms_metres,
        source,
        rights: text(&region["rights"]),
        georeferenced_by: Some(text(&region["georeferenced_by"])).filter(|t| !t.is_empty()),
        credit,
        title,
        digest: compute_binary_hash(&all),
    })
}

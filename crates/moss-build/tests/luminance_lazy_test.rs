//! A picture's luminance is read when a `:::hero` uses the picture, and only
//! then: a site of ordinary images holds none. This file holds one test so the
//! process-wide count is its own.

#[path = "support/site.rs"]
mod site;

use moss_build::build::markdown::hero_ink::grounds_built;
use site::Site;

fn png(shade: u8) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(64, 40, image::Rgb([shade, shade, shade]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn only_a_picture_a_hero_uses_has_its_luminance_read() {
    let before = grounds_built();
    let site = Site::new(&[("index.md", "---\ntitle: Home\n---\n\n![One](one.png)\n\n![Two](two.png)\n"), ("one.md", "---\ntitle: One\n---\n\n![Three](three.png)\n")]);
    for (name, shade) in [("one.png", 200), ("two.png", 120), ("three.png", 30)] {
        std::fs::write(site.root.join(name), png(shade)).unwrap();
    }
    site.rebuild();
    assert_eq!(grounds_built(), before, "ordinary images read no luminance");

    // An ordinary hero keeps its panel and measures nothing.
    site.write("index.md", "---\ntitle: Home\n---\n\n:::hero {image=one.png align=top}\n## Hello\n:::\n");
    site.rebuild();
    assert_eq!(grounds_built(), before, "a hero under a panel reads no luminance");

    site.write("index.md", "---\ntitle: Home\n---\n\n:::hero {image=one.png align=top panel=auto}\n## Hello\n:::\n\n![Two](two.png)\n");
    site.rebuild();
    assert!(grounds_built() > before, "a hero that measures reads its picture");
    assert!(grounds_built() - before <= 2, "and only that picture, once per build: {}", grounds_built() - before);
}

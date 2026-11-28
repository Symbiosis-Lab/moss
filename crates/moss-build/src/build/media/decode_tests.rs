#![cfg(target_vendor = "apple")]

use super::*;

const HEIC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/images/heic");

fn at(name: &str) -> PathBuf {
    Path::new(HEIC).join(name)
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2)
}

#[test]
fn a_heic_decodes_to_its_stored_pixels() {
    let img = sniff_decode(&at("quadrants-64x48.heic")).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (64, 48));
    assert!(near(img.get_pixel(4, 4).0, [255, 0, 0, 255]), "{:?}", img.get_pixel(4, 4));
    assert!(near(img.get_pixel(60, 44).0, [255, 255, 255, 255]), "{:?}", img.get_pixel(60, 44));
}

#[test]
fn a_translucent_heic_keeps_straight_alpha() {
    let img = sniff_decode(&at("translucent-32x32.heic")).unwrap().to_rgba8();
    let px = img.get_pixel(4, 4).0;
    assert!(px[3].abs_diff(128) <= 1 && px[0] >= 253 && px[1] <= 2 && px[2] <= 2, "{px:?}");
}

#[test]
fn a_heic_is_turned_upright_once() {
    let img = sniff_decode_oriented(&at("quadrants-64x48-orient6.heic")).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (48, 64));
    // A quarter turn clockwise brings the stored bottom-left (blue) to the top-left.
    assert!(near(img.get_pixel(4, 4).0, [0, 0, 255, 255]), "{:?}", img.get_pixel(4, 4));
}

#[test]
fn a_heic_claiming_a_huge_size_is_refused_before_decoding() {
    let mut bytes = fs_bytes("quadrants-64x48.heic");
    let at_ispe = bytes.windows(4).position(|w| w == b"ispe").unwrap();
    // `ispe`: version and flags, then width and height.
    bytes[at_ispe + 8..at_ispe + 12].copy_from_slice(&100_000u32.to_be_bytes());
    bytes[at_ispe + 12..at_ispe + 16].copy_from_slice(&100_000u32.to_be_bytes());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bomb.heic");
    std::fs::write(&path, bytes).unwrap();
    assert!(matches!(sniff_decode(&path), Err(image::ImageError::Limits(_))));
}

#[test]
fn a_heic_imageio_cannot_read_is_an_error_not_a_crash() {
    let mut bytes = fs_bytes("tiled-1100x700.heic");
    let at_grid = bytes.windows(4).position(|w| w == b"grid").unwrap();
    bytes[at_grid..at_grid + 4].copy_from_slice(b"xxxx");
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("unknown.heic");
    std::fs::write(&path, bytes).unwrap();
    assert!(sniff_decode(&path).is_err());
}

fn fs_bytes(name: &str) -> Vec<u8> {
    std::fs::read(at(name)).unwrap()
}

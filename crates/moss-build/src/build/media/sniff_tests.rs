use super::*;
use image::{DynamicImage, ImageBuffer, Rgb};

// ----- GIF / WebP animation sniffing -----

#[test]
fn is_animated_gif_on_static_gif() {
    // Build a 1x1 static GIF using the `image` crate's GIF encoder.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("static.gif");
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(1, 1, |_, _| Rgb([0, 0, 0]));
    let dyn_img = DynamicImage::ImageRgb8(buf);
    dyn_img
        .save_with_format(&path, image::ImageFormat::Gif)
        .unwrap();
    assert!(!is_animated_gif(&path), "static GIF should not be animated");
}

#[test]
fn is_animated_gif_on_handcrafted_multiframe() {
    // Hand-craft a minimal 2-frame GIF89a with two 0x2C Image Descriptors.
    // We don't need it to actually decode — is_animated_gif only scans
    // for the frame introducer byte count.
    let mut bytes: Vec<u8> = b"GIF89a".to_vec();
    bytes.extend_from_slice(&[1, 0, 1, 0]); // LSD width/height
    bytes.extend_from_slice(&[0, 0, 0]); // packed, bgcolor, aspect
    bytes.push(0x2C); // Image Descriptor 1
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(0x2C); // Image Descriptor 2
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(0x3B); // trailer

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("multi.gif");
    fs::write(&path, &bytes).unwrap();
    assert!(is_animated_gif(&path));
}

#[test]
fn is_animated_webp_on_static() {
    // Write a minimal static WebP header.
    let mut bytes: Vec<u8> = b"RIFF".to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 0]); // length
    bytes.extend_from_slice(b"WEBPVP8 ");
    bytes.extend_from_slice(&[0; 32]);

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("static.webp");
    fs::write(&path, &bytes).unwrap();
    assert!(!is_animated_webp(&path));
}

#[test]
fn is_animated_webp_on_anim_chunk() {
    let mut bytes: Vec<u8> = b"RIFF".to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(b"WEBP");
    bytes.extend_from_slice(b"VP8X");
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(b"ANIM");
    bytes.extend_from_slice(&[0; 10]);

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("anim.webp");
    fs::write(&path, &bytes).unwrap();
    assert!(is_animated_webp(&path));
}

// ----- CMYK JPEG detection -----

#[test]
fn an_rgb_jpeg_has_no_cmyk_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("rgb.jpg");
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(10, 10, |_, _| Rgb([128, 64, 32]));
    let dyn_img = DynamicImage::ImageRgb8(buf);
    dyn_img
        .save_with_format(&path, image::ImageFormat::Jpeg)
        .unwrap();
    assert!(!has_cmyk_profile(&path));
}

#[test]
fn a_cmyk_profile_is_found_past_the_first_few_kb() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cmyk.jpg");
    crate::build::media::image::tests::make_cmyk_jpeg(&path);
    assert!(has_cmyk_profile(&path));
}

#[test]
fn a_cmyk_jpeg_without_a_profile_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cmyk.jpg");
    fs::write(&path, crate::build::media::image::tests::CMYK_UNTAGGED).unwrap();
    assert!(!has_cmyk_profile(&path));
}

// ----- HEIF brand routing -----

fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
    let size = 16 + 4 * compatible.len();
    let mut b = (size as u32).to_be_bytes().to_vec();
    b.extend_from_slice(b"ftyp");
    b.extend_from_slice(major);
    b.extend_from_slice(&[0, 0, 0, 0]);
    for c in compatible {
        b.extend_from_slice(*c);
    }
    b
}

#[test]
fn a_heic_is_heif_by_its_brands_whatever_its_name() {
    assert!(heif_brands(&ftyp(b"heic", &[b"mif1", b"heic"])));
    assert!(heif_brands(&ftyp(b"mif1", &[b"heic"])));
    let tmp = tempfile::tempdir().unwrap();
    let named_jpg = tmp.path().join("photo.jpg");
    fs::write(&named_jpg, include_bytes!("../../../tests/fixtures/images/heic/quadrants-64x48.heic")).unwrap();
    assert!(is_heif(&named_jpg));
}

#[test]
fn an_avif_listing_mif1_is_not_heif() {
    assert!(!heif_brands(&ftyp(b"mif1", &[b"avif", b"mif1", b"miaf"])));
    assert!(!heif_brands(&ftyp(b"avif", &[b"mif1", b"msf1"])));
    assert!(!heif_brands(&ftyp(b"msf1", &[b"avis"])));
}

#[test]
fn a_jpeg_named_heic_is_not_heif() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("photo.heic");
    fs::write(&p, [0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0]).unwrap();
    assert!(!is_heif(&p));
}

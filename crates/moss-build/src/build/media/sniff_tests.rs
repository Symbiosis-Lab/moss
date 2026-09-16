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
fn is_cmyk_jpeg_on_rgb_jpeg() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("rgb.jpg");
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(10, 10, |_, _| Rgb([128, 64, 32]));
    let dyn_img = DynamicImage::ImageRgb8(buf);
    dyn_img
        .save_with_format(&path, image::ImageFormat::Jpeg)
        .unwrap();
    assert!(!is_cmyk_jpeg(&path));
}

#[test]
fn is_cmyk_jpeg_handcrafted_sof_components4() {
    // SOI 0xFFD8, then SOF0 0xFFC0 with components = 4.
    let mut bytes: Vec<u8> = vec![0xFF, 0xD8];
    bytes.push(0xFF);
    bytes.push(0xC0); // SOF0
    bytes.extend_from_slice(&[0x00, 0x11]); // length = 17
    bytes.push(0x08); // precision
    bytes.extend_from_slice(&[0x00, 0x10]); // height
    bytes.extend_from_slice(&[0x00, 0x10]); // width
    bytes.push(0x04); // component count = 4 (CMYK)
    bytes.extend_from_slice(&[0; 12]); // padding to satisfy length

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cmyk.jpg");
    fs::write(&path, &bytes).unwrap();
    assert!(is_cmyk_jpeg(&path));
}

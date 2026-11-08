//! Embeds the browser UI (`packages/moss-ui/dist-browser`) as a gzipped asset
//! table for moss-build to serve. Without a built bundle the table is empty
//! and `moss edit` refuses to serve, naming the build command, unless
//! `MOSS_REQUIRE_UI_BUNDLE=1` turns the missing bundle into a build failure.
use flate2::{write::GzEncoder, Compression};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../packages/moss-ui/dist-browser");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo:rustc-check-cfg=cfg(moss_ui_bundle_missing)");
    println!("cargo:rerun-if-changed={}", root.join("index.html").display());
    println!("cargo:rerun-if-env-changed=MOSS_REQUIRE_UI_BUNDLE");
    // A release build sets this, so a shipped binary never lacks the editor.
    if std::env::var_os("MOSS_REQUIRE_UI_BUNDLE").is_some_and(|v| v == "1")
        && !root.join("index.html").is_file()
    {
        panic!(
            "MOSS_REQUIRE_UI_BUNDLE=1 but {} has no index.html: run `pnpm --filter @symbiosis-lab/moss-ui build:browser` first",
            root.display()
        );
    }
    let mut source = String::new();
    if root.join("index.html").is_file() {
        source.push_str("const MISSING: Option<&str> = None;\nconst ASSETS: &[UiAsset] = &[\n");
        let mut files = Vec::new();
        collect(&root, &mut files);
        files.sort();
        for (index, path) in files.iter().enumerate() {
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path.strip_prefix(&root).unwrap().to_str().unwrap().replace('\\', "/");
            let packed = out.join(format!("ui-{index}.gz"));
            let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
            encoder.write_all(&fs::read(path).unwrap()).unwrap();
            fs::write(&packed, encoder.finish().unwrap()).unwrap();
            source.push_str(&format!(
                "UiAsset {{ path: {name:?}, bytes: include_bytes!({packed:?}), content_type: {:?}, gzip: true, immutable: {} }},\n",
                content_type(&name),
                name != "index.html" && is_hashed(&name),
            ));
        }
        source.push_str("];\n");
    } else {
        println!("cargo:rustc-cfg=moss_ui_bundle_missing");
        source.push_str("const MISSING: Option<&str> = Some(\"MOSS_UI_BUNDLE_MISSING\");\nconst ASSETS: &[UiAsset] = &[];\n");
    }
    fs::write(out.join("ui_bundle.rs"), source).unwrap();
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// A name segment of at least eight hex digits is the bundler's content hash.
fn is_hashed(name: &str) -> bool {
    name.rsplit('/')
        .next()
        .unwrap_or("")
        .split(['-', '.'])
        .any(|part| part.len() >= 8 && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn collect(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            collect(&entry.path(), files);
        } else if kind.is_file() {
            files.push(entry.path());
        } else {
            panic!("UI bundle must contain only regular files and directories");
        }
    }
}

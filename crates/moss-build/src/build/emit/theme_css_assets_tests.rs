use super::*;
use crate::build::assets::paths::compute_binary_hash;
use std::collections::HashMap;

fn files(entries: &[(&str, &[u8])]) -> HashMap<String, Vec<u8>> {
    entries.iter().map(|(k, v)| (k.to_string(), v.to_vec())).collect()
}

fn run(css: &str, theme: &HashMap<String, Vec<u8>>) -> ThemeCss {
    rewrite(css, |rel| theme.get(rel).cloned())
}

fn h(bytes: &[u8]) -> String {
    compute_binary_hash(bytes)
}

#[test]
fn a_quoted_font_url_gets_the_hashed_name_and_a_copy() {
    let theme = files(&[("fonts/a.woff2", b"FONT")]);
    let out = run("@font-face{src:url(\"fonts/a.woff2\") format(\"woff2\")}", &theme);
    let hashed = format!("fonts/a.{}.woff2", h(b"FONT"));
    assert_eq!(out.css, format!("@font-face{{src:url(\"{hashed}\") format(\"woff2\")}}"));
    assert_eq!(out.files.len(), 1);
    assert_eq!(out.files[0].0.as_str(), format!("_moss/theme/{hashed}"));
    assert_eq!(out.files[0].1, b"FONT");
}

#[test]
fn unquoted_single_quoted_and_dot_slash_forms_are_rewritten() {
    let theme = files(&[("img/g.png", b"PNG")]);
    let hashed = format!("img/g.{}.png", h(b"PNG"));
    for (written, expect) in [
        ("url(img/g.png)", format!("url({hashed})")),
        ("url( img/g.png )", format!("url( {hashed} )")),
        ("url('img/g.png')", format!("url('{hashed}')")),
        ("URL(./img/g.png)", format!("URL(./{hashed})")),
    ] {
        let out = run(&format!("a{{background:{written}}}"), &theme);
        assert_eq!(out.css, format!("a{{background:{expect}}}"), "{written}");
    }
}

#[test]
fn image_set_strings_mask_cursor_and_import_are_rewritten() {
    let theme = files(&[("a.png", b"1"), ("b.png", b"2"), ("m.svg", b"3"), ("c.cur", b"4"), ("more.css", b"5")]);
    let css = "a{background:image-set(\"a.png\" 1x, url(b.png) 2x);mask:url(m.svg);cursor:url(c.cur), auto}@import url(more.css);";
    let out = run(css, &theme);
    for (name, bytes) in [("a.png", b"1"), ("b.png", b"2"), ("m.svg", b"3"), ("c.cur", b"4"), ("more.css", b"5")] {
        let (stem, ext) = name.rsplit_once('.').unwrap();
        assert!(out.css.contains(&format!("{stem}.{}.{ext}", h(bytes))), "{name}: {}", out.css);
    }
    assert_eq!(out.files.len(), 5);
}

#[test]
fn fragment_and_query_follow_the_new_name() {
    let theme = files(&[("icons.svg", b"S"), ("f.woff", b"W")]);
    let out = run("a{background:url(icons.svg#arrow)}b{src:url(f.woff?v=2)}", &theme);
    assert!(out.css.contains(&format!("url(icons.{}.svg#arrow)", h(b"S"))), "{}", out.css);
    assert!(out.css.contains(&format!("url(f.{}.woff?v=2)", h(b"W"))), "{}", out.css);
}

#[test]
fn out_of_scope_urls_are_left_byte_for_byte() {
    let theme = files(&[("x.png", b"X"), ("assets/x.png", b"Y")]);
    let css = "a{background:url(data:image/png;base64,AAAA)}\
               b{background:url(https://example.com/x.png)}\
               c{background:url(//cdn.example.com/x.png)}\
               d{background:url(/assets/x.png)}\
               e{background:url(../outside.png)}\
               f{background:url(missing.png)}\
               g{background:url(#frag)}\
               h{content:\"url(x.png)\"}\
               /* url(x.png) */\
               i{background:url(noext)}";
    let out = run(css, &theme);
    assert_eq!(out.css, css);
    assert!(out.files.is_empty());
}

#[test]
fn a_file_referenced_twice_is_copied_once_and_same_names_in_two_folders_stay_apart() {
    let theme = files(&[("a/x.png", b"ONE"), ("b/x.png", b"TWO")]);
    let out = run("p{background:url(a/x.png)}q{background:url(./a/x.png)}r{background:url(b/x.png)}", &theme);
    assert_eq!(out.files.len(), 2);
    assert!(out.css.contains(&format!("a/x.{}.png", h(b"ONE"))));
    assert!(out.css.contains(&format!("b/x.{}.png", h(b"TWO"))));
}

#[test]
fn a_stylesheet_with_nothing_to_rewrite_comes_back_identical() {
    let css = "@layer themes{body{color:red}}\n/* none */\n";
    let out = run(css, &HashMap::new());
    assert_eq!(out.css, css);
    assert!(out.files.is_empty());
}

#[test]
fn changing_a_files_bytes_changes_the_stylesheet() {
    let css = "a{src:url(f.woff2)}";
    let one = run(css, &files(&[("f.woff2", b"v1")]));
    let two = run(css, &files(&[("f.woff2", b"v2")]));
    assert_ne!(one.css, two.css);
}

#[test]
fn the_entry_stylesheet_and_script_are_not_copied_under_a_second_name() {
    let theme = files(&[("style.css", b"S"), ("script.js", b"J")]);
    let css = "@import url(style.css);a{background:url(script.js)}";
    assert_eq!(run(css, &theme).css, css);
}

#[test]
fn a_reference_without_a_literal_extension_or_with_an_encoded_separator_is_left_and_not_published() {
    let theme = files(&[("a.png", b"1"), ("sub/b.png", b"2")]);
    let css = "p{background:url(a%2Epng)}q{background:url(sub%2Fb.png)}r{background:url(sub%5Cb.png)}";
    let out = run(css, &theme);
    assert_eq!(out.css, css);
    assert!(out.files.is_empty(), "nothing is published under a name no reference uses");
}

fn site_with_theme(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, bytes) in files {
        let path = dir.path().join(".moss/theme").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    dir
}

fn for_site_on(dir: &tempfile::TempDir, css: &str) -> ThemeCss {
    let evidence = crate::build::cloud_ledger::InputEvidence::new(dir.path());
    for_site(css, dir.path(), &evidence)
}

#[cfg(unix)]
#[test]
fn a_symlink_in_the_theme_folder_is_not_followed() {
    let dir = site_with_theme(&[("real.png", b"REAL")]);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.png"), b"SECRET").unwrap();
    let theme = dir.path().join(".moss/theme");
    std::os::unix::fs::symlink(outside.path().join("secret.png"), theme.join("link.png")).unwrap();
    std::os::unix::fs::symlink(outside.path(), theme.join("linked")).unwrap();

    let css = "a{background:url(link.png)}b{background:url(linked/secret.png)}c{background:url(real.png)}";
    let out = for_site_on(&dir, css);
    assert!(out.css.contains("url(link.png)") && out.css.contains("url(linked/secret.png)"), "{}", out.css);
    assert_eq!(out.files.len(), 1, "only the real file is published");
    assert_eq!(out.files[0].1, b"REAL");
}

#[test]
fn a_file_over_the_size_limit_keeps_its_plain_name() {
    let dir = site_with_theme(&[("small.png", b"S")]);
    let big = std::fs::File::create(dir.path().join(".moss/theme/big.mp4")).unwrap();
    big.set_len(MAX_HASHED_FILE_BYTES + 1).unwrap();

    let out = for_site_on(&dir, "a{background:url(big.mp4)}b{background:url(small.png)}");
    assert!(out.css.contains("url(big.mp4)"), "{}", out.css);
    assert_eq!(out.files.len(), 1);
}

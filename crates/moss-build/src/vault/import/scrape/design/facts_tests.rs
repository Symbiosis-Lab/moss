use super::*;

const BASE: &str = "https://studio.example/";

fn read_page(html: &str) -> Facts {
    read(html, BASE)
}

fn nav(facts: &Facts) -> Vec<(String, String)> {
    facts.nav.iter().map(|e| (e.label.clone(), e.url.clone())).collect()
}

#[test]
fn the_site_name_is_og_then_json_ld_then_the_home_titles_name() {
    let og = r#"<head><meta property="og:site_name" content="Studio Example"><title>Home | Other</title></head>"#;
    assert_eq!(read_page(og).site_name.as_deref(), Some("Studio Example"));

    let ld = r#"<head><title>Home</title><script type="application/ld+json">
        {"@context":"https://schema.org","@graph":[{"@type":"WebSite","name":"Studio Example"}]}
        </script></head>"#;
    assert_eq!(read_page(ld).site_name.as_deref(), Some("Studio Example"));

    // A home title leads with the name.
    let named = "<head><title>Studio Name | Costume design</title></head>";
    assert_eq!(read_page(named).site_name.as_deref(), Some("Studio Name"));

    // Unless the first segment is a generic word.
    let generic = "<head><title>Home | Studio Name</title></head>";
    assert_eq!(read_page(generic).site_name.as_deref(), Some("Studio Name"));

    // A title that is only a generic word names nothing.
    assert_eq!(read_page("<head><title>Welcome</title></head>").site_name, None);
}

#[test]
fn nav_is_the_header_nav_with_two_same_host_links_in_order() {
    let html = r##"<body>
        <nav><a href="/skip-one">Skip</a></nav>
        <header><nav><ul>
          <li><a href="/about">About</a></li>
          <li><a href="https://studio.example/work/">Work</a>
              <ul><li><a href="/work/chairs">Chairs</a></li></ul></li>
          <li><a href="https://elsewhere.example/shop">Shop</a></li>
          <li><a href="#top">Top</a></li>
          <li><a href="/about">About again</a></li>
          <li><a href="/contact"><img src="/c.png" alt=""><span> Contact </span></a></li>
        </ul></nav></header></body>"##;
    assert_eq!(
        nav(&read_page(html)),
        vec![
            ("About".to_string(), "https://studio.example/about".to_string()),
            ("Work".to_string(), "https://studio.example/work/".to_string()),
            ("Contact".to_string(), "https://studio.example/contact".to_string()),
        ],
        "submenu, external, fragment and repeated links are not the primary nav"
    );
}

#[test]
fn a_nav_outside_the_header_is_the_fallback_and_one_link_is_not_a_nav() {
    let outside = r#"<body><nav><a href="/a">A</a> <a href="/b">B</a></nav></body>"#;
    assert_eq!(read_page(outside).nav.len(), 2);

    let lonely = r#"<body><header><nav><a href="/a">A</a></nav></header></body>"#;
    assert!(read_page(lonely).nav.is_empty());
}

#[test]
fn the_logo_is_the_header_image_that_links_home_or_says_logo() {
    let linked = r#"<header><a href="/"><img src="/mark.png"></a>
        <img src="/photo.jpg"></header>"#;
    assert_eq!(read_page(linked).logo.as_deref(), Some("https://studio.example/mark.png"));

    let by_class = r#"<header><div class="site-logo"><img src="/brand.png"></div></header>"#;
    assert_eq!(read_page(by_class).logo.as_deref(), Some("https://studio.example/brand.png"));

    let by_itemprop = r#"<header><img itemprop="logo" src="/i.png"></header>"#;
    assert_eq!(read_page(by_itemprop).logo.as_deref(), Some("https://studio.example/i.png"));

    let not_a_logo = r#"<header><a href="/about"><img src="/team.jpg"></a></header>"#;
    assert_eq!(read_page(not_a_logo).logo, None);
}

#[test]
fn an_svg_logo_wins_over_an_earlier_raster_one() {
    let html = r#"<header><a href="/"><img src="/mark.png"></a>
        <a href="/"><img src="/mark.svg"></a></header>"#;
    assert_eq!(read_page(html).logo.as_deref(), Some("https://studio.example/mark.svg"));
}

#[test]
fn the_logo_falls_back_to_the_json_ld_organization() {
    let html = r#"<head><script type="application/ld+json">
        {"@type":"Organization","name":"Studio","logo":{"@type":"ImageObject","url":"/ld.png"}}
        </script></head><body><header></header></body>"#;
    assert_eq!(read_page(html).logo.as_deref(), Some("https://studio.example/ld.png"));
}

#[test]
fn the_favicon_prefers_svg_then_the_largest_png_then_ico() {
    let all = r#"<head>
        <link rel="shortcut icon" href="/f.ico">
        <link rel="icon" type="image/png" sizes="32x32" href="/f32.png">
        <link rel="apple-touch-icon" sizes="180x180" href="/f180.png">
        <link rel="icon" type="image/svg+xml" href="/f.svg"></head>"#;
    let icon = read_page(all).favicon.unwrap();
    assert_eq!((icon.url.as_str(), icon.kind), ("https://studio.example/f.svg", IconKind::Svg));

    let raster = r#"<head><link rel="icon" href="/f.ico">
        <link rel="icon" type="image/png" sizes="32x32" href="/f32.png">
        <link rel="apple-touch-icon" sizes="180x180" href="/f180.png"></head>"#;
    assert_eq!(read_page(raster).favicon.unwrap().url, "https://studio.example/f180.png");

    let ico = r#"<head><link rel="shortcut icon" href="/f.ico"><link rel="mask-icon" href="/m.svg"></head>"#;
    let icon = read_page(ico).favicon.unwrap();
    assert_eq!((icon.url.as_str(), icon.kind), ("https://studio.example/f.ico", IconKind::Ico));
}

#[test]
fn a_platform_default_icon_is_not_a_favicon_and_not_a_logo() {
    for default in [
        "https://studio.example/pfavico.ico",
        "https://static.parastorage.com/client/pfavico.ico",
        "https://assets.squarespace.com/universal/default-favicon.ico",
        "https://s0.wp.com/i/favicon.ico",
        "https://cdn.shopify.com/shopifycloud/shopify/assets/favicon.png",
    ] {
        let html = format!(r#"<head><link rel="icon" href="{default}"></head><header><a href="/"><img src="{default}"></a></header>"#);
        let facts = read_page(&html);
        assert_eq!(facts.favicon, None, "{default}");
        assert_eq!(facts.logo, None, "{default}");
    }
}

#[test]
fn a_bare_page_has_no_chrome() {
    let facts = read_page("<body><p>Just words.</p></body>");
    assert!(facts.nav.is_empty() && facts.logo.is_none() && facts.footer.is_none());
    assert_eq!(facts.favicon, None);
}

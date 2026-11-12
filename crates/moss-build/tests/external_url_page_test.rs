//! A page whose `external_url:` names an http(s) address lives on another
//! site: its built page links out there, and llms.txt gives that address.

#[path = "support/site.rs"]
mod site;

use site::Site;

const OUT: &str = r#"<p class="moss-external-link"><a href="https://www.archive.example.org/field/">archive.example.org"#;

fn site() -> Site {
    Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nWelcome.\n\n![[work/]]\n"),
        ("work/index.md", "---\ntitle: Work\n---\n"),
        ("work/field.md", "---\ntitle: Field Archive\nexternal_url: https://www.archive.example.org/field/\n---\n\nA survey kept by the archive.\n"),
        ("work/stub.md", "---\ntitle: Stub\nexternal_url: https://stub.example.net/\n---\n"),
        ("work/linked.md", "---\ntitle: Linked\nexternal_url: https://linked.example.com/a\n---\n\n[Read it](https://linked.example.com/a)\n"),
        ("work/folder/index.md", "---\ntitle: Folder\nexternal_url: https://folder.example.com/\n---\n\nA folder kept elsewhere.\n"),
        ("work/books.md", "---\ntitle: Books\nexternal_url: https://bücher.example/reihe/\n---\n"),
        ("work/contact.md", "---\ntitle: Contact\nexternal_url: \"mailto:hello@example.com\"\n---\n\nWrite to us.\n"),
    ])
}

#[test]
fn a_page_published_elsewhere_links_out_without_the_author_writing_it() {
    let site = site();

    let field = site.html("work/field/index.html");
    let (body, out) = (field.find("A survey kept").unwrap(), field.find(OUT).unwrap_or_else(|| panic!("{field}")));
    assert!(body < out, "the link follows the body: {field}");

    let stub = site.html("work/stub/index.html");
    assert!(stub.contains(r#"<a href="https://stub.example.net/">stub.example.net"#), "a body-less stub gets it too: {stub}");

    let folder = site.html("work/folder/index.html");
    assert!(folder.contains(r#"<a href="https://folder.example.com/">folder.example.com"#), "a folder page gets it too: {folder}");

    let linked = site.html("work/linked/index.html");
    assert!(linked.contains("Read it</a>") && !linked.contains("moss-external-link"), "a body that links there gets no second link: {linked}");

    let books = site.html("work/books/index.html");
    assert!(books.contains(">bücher.example<span"), "an international domain reads in its own script: {books}");

    let contact = site.html("work/contact/index.html");
    assert!(!contact.contains("moss-external-link"), "a nav-only address is no home elsewhere: {contact}");
}

#[test]
fn llms_txt_gives_a_page_published_elsewhere_its_address_there() {
    let llms = site().html("llms.txt");
    assert!(llms.contains("## Field Archive\n\n<https://www.archive.example.org/field/>\n"), "{llms}");
    assert!(llms.contains("- [Field Archive](https://www.archive.example.org/field/)"), "the listing row points there too: {llms}");
}

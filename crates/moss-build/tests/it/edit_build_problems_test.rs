//! An edit build replays the parse of every page it did not change, and still
//! reports the problems those pages have.

use crate::site;

use moss_build::build::BuildTrigger;
use site::Site;

#[test]
fn an_edit_to_another_page_still_reports_a_broken_pages_problems() {
    let site = Site::new(&[
        ("index.md", "Welcome."),
        ("page.md", "A link to [[No Such Page]] and an image ![[no-such-image.png]].\n"),
        ("other.md", "edit 0"),
    ]);
    for edit in 1..3 {
        site.write("other.md", &format!("edit {edit}"));
        let built = site::build_logged(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("other.md")]))
            .expect("edit build");
        let problems = built.problems.join("\n");
        assert!(problems.contains("No Such Page") && problems.contains("no-such-image.png"), "edit {edit}: {problems}");
        assert_eq!(built.cli_problems, 2, "edit {edit}: {problems}");
    }
}

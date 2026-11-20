use super::*;

#[test]
fn init_writes_the_root_pointer_when_the_folder_has_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(write_root_pointer(dir.path()).unwrap());
    let text = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(text.contains("Run `moss guide` before editing it.") && text.contains("`moss describe --json` lists under `paths`"), "{text}");
}

#[test]
fn init_leaves_a_root_agents_md_the_person_wrote_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let mine = "# My rules\n\nNever touch drafts/.\n";
    std::fs::write(dir.path().join("AGENTS.md"), mine).unwrap();
    assert!(!write_root_pointer(dir.path()).unwrap());
    assert_eq!(std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(), mine);
}

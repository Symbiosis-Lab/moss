use super::*;

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[test]
fn identical_final_bytes_report_no_divergence() {
    let stage = tempfile::tempdir().unwrap();
    // The bytes on both sides are POST-injection: the marker is gone. This
    // is the shape the old in-render comparison could never produce.
    let final_html = "<html><head><style>h1{}</style></head><body>hi</body></html>";
    write(stage.path(), "writings/a/index.html", final_html);

    let mut verify = CarryVerification::default();
    verify.record(
        "writings/a/index.html".to_string(),
        final_html.as_bytes().to_vec(),
    );

    let report = verify.compare_final_bytes(stage.path());
    assert_eq!(
        report,
        VerifyReport {
            identical: 1,
            diverged: 0,
            unreadable: 0
        }
    );
}

#[test]
fn a_real_content_change_is_reported_as_divergence() {
    let stage = tempfile::tempdir().unwrap();
    write(stage.path(), "index.html", "<body>NEW excerpt</body>");

    let mut verify = CarryVerification::default();
    verify.record(
        "index.html".to_string(),
        b"<body>old excerpt</body>".to_vec(),
    );

    let report = verify.compare_final_bytes(stage.path());
    assert_eq!(report.diverged, 1);
    assert_eq!(report.identical, 0);
}

#[test]
fn slot_injection_delta_alone_is_not_a_divergence() {
    // Regression guard for the false positive this module exists to fix:
    // the pre-injection bytes differ from the final bytes by a fixed block,
    // and comparing at the render phase flagged every page. Comparing the
    // POST-injection bytes on both sides, as we now do, is clean.
    let stage = tempfile::tempdir().unwrap();
    let pre_injection = "<head><!-- slot:head-end --></head>";
    let post_injection = "<head><style>h1{}</style></head>";
    write(stage.path(), "index.html", post_injection);

    let mut verify = CarryVerification::default();
    verify.record("index.html".to_string(), post_injection.as_bytes().to_vec());
    assert_eq!(verify.compare_final_bytes(stage.path()).diverged, 0);

    // And the comparison the old gate made, for contrast: it "diverges".
    let mut naive = CarryVerification::default();
    naive.record("index.html".to_string(), pre_injection.as_bytes().to_vec());
    assert_eq!(naive.compare_final_bytes(stage.path()).diverged, 1);
}

#[test]
fn a_missing_output_file_is_unreadable_not_identical() {
    let stage = tempfile::tempdir().unwrap();
    let mut verify = CarryVerification::default();
    verify.record("gone/index.html".to_string(), b"whatever".to_vec());

    let report = verify.compare_final_bytes(stage.path());
    assert_eq!(
        report,
        VerifyReport {
            identical: 0,
            diverged: 0,
            unreadable: 1
        }
    );
}

#[test]
fn first_difference_locator_points_at_the_changed_byte() {
    let described = describe_first_difference(b"abcdef", b"abcXef");
    assert!(
        described.starts_with("first difference at byte 3:"),
        "{described}"
    );
}
